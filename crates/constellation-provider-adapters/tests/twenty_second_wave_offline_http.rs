//! Real loopback HTTP fixtures exercise adapter preparation and response
//! parsing without contacting a provider, loading secrets or using the native
//! transport. These do not assert native DNS, TLS, timeouts or cancellation.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

use constellation_core::{
    ChatEntry, ChatRole, CredentialSource, ProviderId, ProviderKind, ProviderProfile,
};
use constellation_provider_adapters::{AdapterError, parse_response, parse_text, prepare};
use serde_json::{Value, json};

fn valid_responses() -> [(ProviderKind, Value); 6] {
    [
        (
            ProviderKind::OpenaiChat,
            json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"safe"}}]}),
        ),
        (
            ProviderKind::CustomCompatible,
            json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"safe"}}]}),
        ),
        (
            ProviderKind::OpenaiResponses,
            json!({"status":"completed","output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"safe"}]}]}),
        ),
        (
            ProviderKind::AnthropicMessages,
            json!({"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"safe"}]}),
        ),
        (
            ProviderKind::GoogleGemini,
            json!({"candidates":[{"finishReason":"STOP","content":{"role":"model","parts":[{"text":"safe"}]}}]}),
        ),
        (
            ProviderKind::OllamaNative,
            json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":"safe"}}),
        ),
    ]
}

fn terminal_mut(value: &mut Value, kind: ProviderKind) -> &mut Value {
    match kind {
        ProviderKind::OpenaiChat | ProviderKind::CustomCompatible => {
            &mut value["choices"][0]["finish_reason"]
        }
        ProviderKind::OpenaiResponses => &mut value["status"],
        ProviderKind::AnthropicMessages => &mut value["stop_reason"],
        ProviderKind::GoogleGemini => &mut value["candidates"][0]["finishReason"],
        ProviderKind::OllamaNative => &mut value["done"],
    }
}

/// A small complete HTTP/1.1 exchange over an actual ephemeral loopback port.
/// The independent fixture server never calls an external endpoint.
fn local_http_fixture(kind: ProviderKind, status: u16, reply: &Value) -> Vec<u8> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("local fixture bind");
    let addr = listener.local_addr().unwrap();
    let profile = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Local fixture".into(),
        model_id: "fixture-model".into(),
        base_url: format!("http://{addr}"),
        kind,
        credentials: CredentialSource::None,
    };
    let request = prepare(
        &profile,
        &[ChatEntry {
            role: ChatRole::User,
            content: "test".into(),
        }],
    )
    .unwrap();
    let endpoint_path = request.endpoint.path().to_owned();
    let response = serde_json::to_vec(reply).unwrap();
    let server = std::thread::spawn(move || {
        let (mut connection, _) = listener.accept().expect("fixture accept");
        connection
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        connection
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut prefix = [0u8; 5];
        connection.read_exact(&mut prefix).unwrap();
        assert_eq!(&prefix, b"POST ");
        let mut rest_of_request = Vec::new();
        connection.read_to_end(&mut rest_of_request).unwrap();
        assert!(rest_of_request.windows(4).any(|bytes| bytes == b"\r\n\r\n"));
        let headers = format!(
            "HTTP/1.1 {status} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        connection.write_all(headers.as_bytes()).unwrap();
        let cut = response.len() / 2;
        connection.write_all(&response[..cut]).unwrap();
        connection.write_all(&response[cut..]).unwrap();
    });

    assert!(
        request
            .endpoint
            .as_str()
            .starts_with(&format!("http://{addr}"))
    );
    let mut connection = TcpStream::connect(addr).expect("fixture connect");
    connection
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    connection
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let body = serde_json::to_vec(&request.body).unwrap();
    let wire_request = format!(
        "POST {endpoint_path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    connection.write_all(wire_request.as_bytes()).unwrap();
    connection.write_all(&body).unwrap();
    connection.shutdown(Shutdown::Write).unwrap();
    let mut wire_response = Vec::new();
    connection.read_to_end(&mut wire_response).unwrap();
    server.join().expect("fixture thread");

    let header_end = wire_response
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("complete HTTP headers");
    let start_line = wire_response[..header_end]
        .split(|byte| *byte == b'\r')
        .next()
        .unwrap();
    assert!(start_line.starts_with(format!("HTTP/1.1 {status} ").as_bytes()));
    let decoded = wire_response[header_end + 4..].to_vec();
    assert_eq!(decoded.len(), reply.to_string().len());
    decoded
}

#[test]
fn actual_loopback_http_fixtures_require_explicit_success_per_provider() {
    for (kind, mut response) in valid_responses() {
        let completed = local_http_fixture(kind, 200, &response);
        assert_eq!(parse_response(kind, 200, &completed), Ok("safe".into()));

        *terminal_mut(&mut response, kind) = Value::Null;
        let missing = local_http_fixture(kind, 200, &response);
        assert_eq!(
            parse_response(kind, 200, &missing),
            Err(AdapterError::Incomplete),
            "{kind:?} must reject an absent terminal state even when HTTP succeeds"
        );
    }
}

#[test]
fn malformed_terminal_values_and_partial_outputs_cannot_be_committed() {
    for (kind, mut response) in valid_responses() {
        *terminal_mut(&mut response, kind) = json!("provider_defined_unknown");
        assert_eq!(
            parse_text(kind, &serde_json::to_vec(&response).unwrap()),
            Err(AdapterError::Incomplete),
            "{kind:?}"
        );
    }
    let incomplete_response_item = json!({
        "status":"completed",
        "output":[{"type":"message","role":"assistant","status":"in_progress",
            "content":[{"type":"output_text","text":"premature"}]}]
    });
    assert_eq!(
        parse_text(
            ProviderKind::OpenaiResponses,
            &serde_json::to_vec(&incomplete_response_item).unwrap()
        ),
        Err(AdapterError::Incomplete)
    );
}

#[test]
fn extra_candidates_spoofed_roles_and_tools_are_rejected() {
    let cases = [
        (
            ProviderKind::OpenaiChat,
            json!({"choices":[{"finish_reason":"stop","message":{"role":"user","content":"spoofed"}}]}),
        ),
        (
            ProviderKind::CustomCompatible,
            json!({"choices":[
                {"finish_reason":"stop","message":{"role":"assistant","content":"safe"}},
                {"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{}]}}
            ]}),
        ),
        (
            ProviderKind::OpenaiResponses,
            json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"partial"}]}]}),
        ),
        (
            ProviderKind::AnthropicMessages,
            json!({"stop_reason":"end_turn","role":"user","content":[{"type":"text","text":"spoofed"}]}),
        ),
        (
            ProviderKind::GoogleGemini,
            json!({"candidates":[
                {"finishReason":"STOP","content":{"role":"model","parts":[{"text":"safe"}]}},
                {"finishReason":"SAFETY","content":{"role":"model","parts":[{"text":"blocked"}]}}
            ]}),
        ),
        (
            ProviderKind::OllamaNative,
            json!({"done":true,"message":{"role":"assistant","content":"safe","tool_calls":[]}}),
        ),
    ];
    for (kind, response) in cases {
        let bytes = serde_json::to_vec(&response).unwrap();
        assert!(
            matches!(
                parse_text(kind, &bytes),
                Err(AdapterError::UnsupportedContent | AdapterError::Incomplete)
            ),
            "{kind:?}"
        );
    }
}

#[test]
fn http_errors_have_priority_and_never_reflect_private_remote_details() {
    let sensitive = json!({"error":{"message":"DO_NOT_DISCLOSE_FIXTURE_TOKEN"}});
    let bytes = local_http_fixture(ProviderKind::OllamaNative, 429, &sensitive);
    let err = parse_response(ProviderKind::OllamaNative, 429, &bytes).unwrap_err();
    assert_eq!(err, AdapterError::HttpStatus(429));
    assert!(!err.to_string().contains("DO_NOT_DISCLOSE_FIXTURE_TOKEN"));
}
