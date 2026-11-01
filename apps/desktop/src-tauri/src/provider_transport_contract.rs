//! Real native HTTP dispatch contract tests using isolated loopback fixtures.
//! No paid model, external DNS, API credential or shell execution is used.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

#[test]
fn each_native_provider_protocol_dispatches_its_own_wire_format() {
    let fixtures = [
        (
            ProviderKind::OpenaiChat,
            "/v1/chat/completions",
            r#"{"choices":[{"message":{"content":"contract-ok"},"finish_reason":"stop"}]}"#,
            "messages",
            "bearer",
        ),
        (
            ProviderKind::OpenaiResponses,
            "/v1/responses",
            r#"{"status":"completed","output":[{"type":"message","status":"completed","content":[{"type":"output_text","text":"contract-ok"}]}]}"#,
            "input",
            "bearer",
        ),
        (
            ProviderKind::AnthropicMessages,
            "/v1/messages",
            r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"contract-ok"}]}"#,
            "messages",
            "anthropic",
        ),
        (
            ProviderKind::GoogleGemini,
            "/v1beta/models/fixture-model:generateContent",
            r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"contract-ok"}]}}]}"#,
            "contents",
            "gemini",
        ),
        (
            ProviderKind::OllamaNative,
            "/api/chat",
            r#"{"done":true,"message":{"role":"assistant","content":"contract-ok"}}"#,
            "messages",
            "none",
        ),
        (
            ProviderKind::CustomCompatible,
            "/v1/chat/completions",
            r#"{"choices":[{"message":{"content":"contract-ok"},"finish_reason":"stop"}]}"#,
            "messages",
            "none",
        ),
    ];
    for (kind, route, response_body, payload_key, authentication) in fixtures {
        let server = TcpListener::bind(("127.0.0.1", 0)).expect("loopback test listener");
        server.set_nonblocking(true).unwrap();
        let port = server.local_addr().unwrap().port();
        let response_body = response_body.to_owned();
        let expected_path = route.to_owned();
        let required_key = payload_key.to_owned();
        let server_thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match server.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("test provider request never arrived: {e}"),
                }
            };
            // Windows can inherit nonblocking mode on an accepted socket.
            // The bounded read timeout is enough after accept; avoid a flaky
            // WouldBlock race when reqwest connects before sending headers.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut data = Vec::new();
            let mut buffer = [0u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "truncated HTTP request");
                data.extend_from_slice(&buffer[..count]);
                assert!(data.len() < 100_000, "oversized fixture request");
                if let Some(pos) = data.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let headers = String::from_utf8_lossy(&data[..header_end]).to_lowercase();
            let request_line = headers.lines().next().unwrap_or_default();
            assert_eq!(
                request_line,
                format!("post {} http/1.1", expected_path.to_lowercase()),
                "wrong protocol route"
            );
            assert!(!headers.contains("proxy-authorization:"));
            match authentication {
                "bearer" => assert!(headers.contains("authorization: bearer fixture-key")),
                "anthropic" => {
                    assert!(headers.contains("x-api-key: fixture-key"));
                    assert!(headers.contains("anthropic-version:"));
                }
                "gemini" => assert!(headers.contains("x-goog-api-key: fixture-key")),
                _ => assert!(!headers.contains("authorization: bearer")),
            }
            let content_length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            while data.len() - header_end < content_length {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "truncated HTTP body");
                data.extend_from_slice(&buffer[..count]);
            }
            let body: Value =
                serde_json::from_slice(&data[header_end..header_end + content_length]).unwrap();
            assert!(
                body.get(&required_key).is_some(),
                "wire request omitted its native payload field"
            );
            if authentication == "gemini" {
                assert!(body.get("systemInstruction").is_some());
            }
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", response_body.len(), response_body).unwrap();
            stream.flush().unwrap();
        });
        let base = match kind {
            ProviderKind::OllamaNative => format!("http://127.0.0.1:{port}"),
            ProviderKind::GoogleGemini => format!("http://127.0.0.1:{port}/v1beta"),
            _ => format!("http://127.0.0.1:{port}/v1"),
        };
        let profile = ProviderProfile {
            id: ProviderId::new(),
            display_name: "Offline test fixture".into(),
            model_id: "fixture-model".into(),
            base_url: base,
            kind,
            credentials: CredentialSource::None,
        };
        let result = tauri::async_runtime::block_on(model_text_reply(
            &profile,
            if authentication == "none" {
                None
            } else {
                Some("fixture-key")
            },
            vec![
                ChatEntry {
                    role: ChatRole::System,
                    content: "Respond to the user's task".into(),
                },
                ChatEntry {
                    role: ChatRole::User,
                    content: "This is a loopback fixture, not a real AI".into(),
                },
            ],
        ));
        server_thread.join().expect("fixture server assertion");
        assert_eq!(result.unwrap(), "contract-ok", "wire type: {kind:?}");
    }
}
