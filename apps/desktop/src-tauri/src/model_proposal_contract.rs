//! Localhost fixture exercises the actual model HTTP transport, strict JSON
//! proposal parser and the trusted native file approval boundary. This is
//! intentionally a synthetic provider, not a real model capability test.
use super::*;
use constellation_core::{AgentId, RoomId, TaskId};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

#[test]
fn localhost_model_json_proposal_requires_independent_native_approval() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mock = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut part = [0u8; 4096];
        loop {
            let read = stream.read(&mut part).unwrap();
            assert!(read > 0, "fixture request ended before HTTP headers");
            request.extend_from_slice(&part[..read]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
            assert!(request.len() < 16_384, "unexpectedly long fixture headers");
        }
        assert!(request.starts_with(b"POST /v1/chat/completions HTTP/1.1"));
        let answer = serde_json::json!({
            "id": "mock-proposal",
            "choices": [{"index": 0, "message": {"role": "assistant", "content":
                r#"{"type":"tool","action":"create_file","path":"approved.txt","content":"model suggestion only"}"#
            }, "finish_reason":"stop"}]
        })
        .to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", answer.len(), answer).unwrap();
        stream.flush().unwrap();
    });
    let profile = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Synthetic localhost fixture".into(),
        model_id: "fixture-model".into(),
        base_url: format!("http://127.0.0.1:{port}/v1"),
        kind: ProviderKind::OpenaiChat,
        credentials: CredentialSource::None,
    };
    let agent = Agent {
        id: AgentId::new(),
        name: "Fixture agent".into(),
        role: "Coding Specialist".into(),
        provider_id: None,
        created_at_ms: 0,
    };
    let history = vec![ChatEntry {
        role: ChatRole::System,
        content: format!("{} Return a JSON file proposal", system_prompt_for(&agent)),
    }];
    let raw = tauri::async_runtime::block_on(model_text_reply(&profile, None, history)).unwrap();
    mock.join().unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let journal = tempfile::tempdir().unwrap();
    let mut host = tool_host::ToolHost::open(workspace.path(), journal.path(), [0x53; 32]).unwrap();
    let proposed = parse_model_decision(&raw).unwrap();
    let preview = prepare_trusted_model_file_action(&mut host, context(), proposed).unwrap();
    assert_eq!(preview["origin"], "model_proposal");
    assert_eq!(preview["permission_granted"], false);
    assert_eq!(preview["reviewed_content"], "model suggestion only");
    assert!(!workspace.path().join("approved.txt").exists());

    let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
    let ticket = preview["digest"].as_str().unwrap();
    assert!(host.pending_native_approval(id, "forged-ticket").is_err());
    assert!(!workspace.path().join("approved.txt").exists());
    let native = host.pending_native_approval(id, ticket).unwrap();
    assert_eq!(
        native.create_content.as_deref(),
        Some("model suggestion only")
    );
    let result = host
        .approve_and_execute(
            id,
            ticket,
            native_consent::NativeConsentProof::fixture_for_test(&id.to_string(), &native.digest),
        )
        .unwrap();
    assert_eq!(result["status"], "succeeded");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("approved.txt")).unwrap(),
        "model suggestion only"
    );
    assert!(host.deny(id).is_err(), "used approval must not be replayed");
}

#[test]
fn rejected_model_read_has_no_renderer_hash_and_no_file_content() {
    let workspace = tempfile::tempdir().unwrap();
    let journal = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("private.txt"), b"not granted").unwrap();
    let mut host = tool_host::ToolHost::open(workspace.path(), journal.path(), [0x54; 32]).unwrap();
    let proposal =
        parse_model_decision(r#"{"type":"tool","action":"read_file","path":"private.txt"}"#)
            .unwrap();
    let preview = prepare_trusted_model_file_action(&mut host, context(), proposal).unwrap();
    assert_eq!(preview["origin"], "model_proposal");
    assert!(preview.get("sha256").is_none());
    assert!(preview.get("byte_count").is_none());
    assert!(preview.get("reviewed_content").is_none());
    assert!(!preview.to_string().contains("not granted"));
    let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
    let denial = host.deny(id).unwrap();
    assert_eq!(denial["executed"], false);
    assert_eq!(
        std::fs::read(workspace.path().join("private.txt")).unwrap(),
        b"not granted"
    );
    assert!(
        host.pending_native_approval(id, preview["digest"].as_str().unwrap())
            .is_err()
    );
}
