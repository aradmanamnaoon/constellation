//! Native loopback HTTP streaming tests. These open a private localhost
//! listener and exercise real reqwest bytes, request framing and response
//! boundaries with no third-party model, API key or external egress.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

fn fixture(
    kind: ProviderKind,
    expected_route: &str,
    body: &str,
    content_type: &str,
) -> (ProviderProfile, thread::JoinHandle<()>) {
    let server = TcpListener::bind(("127.0.0.1", 0)).expect("test listener");
    server.set_nonblocking(true).unwrap();
    let port = server.local_addr().unwrap().port();
    let expected_route = expected_route.to_owned();
    let body = body.as_bytes().to_owned();
    let content_type = content_type.to_owned();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match server.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("native model stream did not connect: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 1024];
        let header_end = loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "truncated native streaming HTTP request");
            bytes.extend_from_slice(&buf[..n]);
            assert!(bytes.len() < 100_000, "unexpectedly large native request");
            if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break offset + 4;
            }
        };
        let head = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
        assert_eq!(
            head.lines().next().unwrap_or_default(),
            format!("post {} http/1.1", expected_route.to_lowercase())
        );
        assert!(!head.contains("proxy-authorization:"));
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .unwrap()
            .parse()
            .unwrap();
        while bytes.len() - header_end < length {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "truncated request JSON");
            bytes.extend_from_slice(&buf[..n]);
        }
        let request: Value =
            serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        assert_eq!(request.get("stream"), Some(&Value::Bool(true)));
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        // The rejection test intentionally advertises an invalid MIME type.
        // A correct client may close the socket as soon as the headers arrive
        // without reading the response body. Windows reports that as
        // ConnectionAborted (WSAECONNABORTED 10053), whereas Unix often
        // reports BrokenPipe or ConnectionReset. Do not mistake the expected
        // client-side refusal for a fixture/server failure in that one case.
        let reject_mime = matches!(
            content_type.as_str(),
            "application/json" | "text/event-stream-malformed"
        );
        let expected_early_close = |error: &std::io::Error| {
            reject_mime
                && matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                )
        };
        if reject_mime {
            // For a rejected media type the real client MUST refuse before
            // exposing any body text. Send a *complete* valid SSE body in the
            // same bounded write as its HTTP headers: the negative test will
            // still catch a client that accidentally streams that text, but
            // Windows cannot race a long sequence of tiny writes against the
            // expected immediate close after headers (WSAECONNABORTED).
            // The accepted-content-type tests below deliberately keep their
            // many small writes to exercise incremental decoder framing.
            let mut response = header.into_bytes();
            response.extend_from_slice(&body);
            if let Err(error) = stream.write_all(&response)
                && !expected_early_close(&error)
            {
                panic!("native rejected MIME fixture write failed: {error}");
            }
            return;
        }
        if let Err(error) = stream.write_all(header.as_bytes()) {
            if expected_early_close(&error) {
                return;
            }
            panic!("native model stream response header write failed: {error}");
        }
        // Multiple network writes may coalesce; the decoder must still accept
        // both split and joined event boundaries.
        for portion in body.chunks(17) {
            if let Err(error) = stream.write_all(portion) {
                if expected_early_close(&error) {
                    return;
                }
                panic!("native model stream response body write failed: {error}");
            }
        }
        if let Err(error) = stream.flush()
            && !expected_early_close(&error)
        {
            panic!("native model stream response flush failed: {error}");
        }
    });
    let base_url = match kind {
        ProviderKind::OllamaNative => format!("http://127.0.0.1:{port}"),
        _ => format!("http://127.0.0.1:{port}/v1"),
    };
    let profile = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Private loopback fixture".into(),
        model_id: "fixture-model".into(),
        base_url,
        kind,
        credentials: CredentialSource::None,
    };
    (profile, worker)
}

fn call(profile: &ProviderProfile) -> (Result<String, String>, String) {
    let mut provisional = String::new();
    let result = tauri::async_runtime::block_on(model_stream_reply(
        profile,
        if profile.kind == ProviderKind::AnthropicMessages {
            Some("fixture-key")
        } else {
            None
        },
        vec![ChatEntry {
            role: ChatRole::User,
            content: "A simulated text-only question".to_owned(),
        }],
        |delta| provisional.push_str(&delta),
    ));
    (result, provisional)
}

#[test]
fn native_stream_dispatches_real_loopback_http_for_five_protocols() {
    let cases = [
        (
            ProviderKind::OpenaiChat,
            "/v1/chat/completions",
            concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello \"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"🌍\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            ),
            "text/event-stream",
        ),
        (
            ProviderKind::CustomCompatible,
            "/v1/chat/completions",
            concat!(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello 🌍\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            ),
            "text/event-stream; charset=utf-8",
        ),
        (
            ProviderKind::OpenaiResponses,
            "/v1/responses",
            concat!(
                "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello 🌍\"}\n\n",
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
            ),
            "text/event-stream",
        ),
        (
            ProviderKind::AnthropicMessages,
            "/v1/messages",
            concat!(
                "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
                "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello 🌍\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            ),
            "text/event-stream",
        ),
        (
            ProviderKind::OllamaNative,
            "/api/chat",
            concat!(
                "{\"message\":{\"role\":\"assistant\",\"content\":\"hello \"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"🌍\"},\"done\":true,\"done_reason\":\"stop\"}\n"
            ),
            "application/x-ndjson",
        ),
    ];
    for (kind, path, body, content_type) in cases {
        let (profile, worker) = fixture(kind, path, body, content_type);
        let (result, provisional) = call(&profile);
        if let Err(problem) = &result {
            eprintln!("stream fixture {kind:?} rejected before completion: {problem}");
        }
        worker.join().expect("model stream fixture panicked");
        assert_eq!(result.unwrap(), "hello 🌍", "protocol: {kind:?}");
        assert_eq!(provisional, "hello 🌍", "protocol: {kind:?}");
    }
}

#[test]
fn provisional_bytes_are_never_a_successful_reply_without_terminal_event() {
    let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"uncommitted preview\"},\"finish_reason\":null}]}\n\n";
    let (profile, worker) = fixture(
        ProviderKind::OpenaiChat,
        "/v1/chat/completions",
        body,
        "text/event-stream",
    );
    let (result, preview) = call(&profile);
    worker.join().unwrap();
    assert_eq!(preview, "uncommitted preview");
    assert!(
        result.is_err(),
        "incomplete output must fail before persistence"
    );
    assert!(!result.unwrap_err().contains("uncommitted preview"));
}

#[test]
fn native_stream_rejects_mismatched_content_type_before_exposing_text() {
    let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"private text\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let (profile, worker) = fixture(
        ProviderKind::OpenaiChat,
        "/v1/chat/completions",
        body,
        "application/json",
    );
    let (result, preview) = call(&profile);
    worker.join().unwrap();
    assert!(result.is_err());
    assert!(
        preview.is_empty(),
        "bad content type leaked provisional text"
    );
    let (spoofed, worker) = fixture(
        ProviderKind::OpenaiChat,
        "/v1/chat/completions",
        body,
        "text/event-stream-malformed",
    );
    let (result, preview) = call(&spoofed);
    worker.join().unwrap();
    assert!(result.is_err(), "invalid MIME type was accepted as SSE");
    assert!(preview.is_empty(), "bad MIME type leaked provisional text");
}

#[test]
fn late_stream_failure_does_not_write_a_room_message_or_success_receipt() {
    let (_dir, state) = super::tests::team_state();
    let room = state.store.create_room("Stream safety").unwrap();
    let agent = state.store.add_agent("Reviewer", "A").unwrap();
    state
        .store
        .append_message(room.id, MessageSender::User, "Local fixture prompt")
        .unwrap();
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"unsafe to commit\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\n"
    );
    let (profile, worker) = fixture(
        ProviderKind::OpenaiChat,
        "/v1/chat/completions",
        body,
        "text/event-stream",
    );
    state.store.save_provider_profile(profile.clone()).unwrap();
    let preview = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let response = tauri::async_runtime::block_on(run_team_batch_with(
        &state,
        room.id,
        &[agent.id],
        profile.id,
        |_, history| {
            let profile = profile.clone();
            let preview = std::sync::Arc::clone(&preview);
            async move {
                model_stream_reply(&profile, None, history, move |delta| {
                    preview.lock().unwrap().push_str(&delta);
                })
                .await
            }
        },
    ))
    .unwrap();
    worker.join().unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].status, TeamReplyStatus::Failed);
    // Reqwest may coalesce the valid delta and terminal error in one chunk.
    // In that case push rejects the whole chunk before emitting any preview.
    let provisional = preview.lock().unwrap().clone();
    assert!(provisional.is_empty() || provisional == "unsafe to commit");
    assert!(
        !response.results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("unsafe to commit"),
        "remote text must not leak into an error receipt"
    );
    let saved = state.store.snapshot().unwrap().messages;
    assert_eq!(saved.len(), 1, "model preview must not become a room reply");
    assert!(matches!(saved[0].sender, MessageSender::User));
}

#[test]
fn room_stream_cancel_is_private_one_use_and_prevents_pending_model_dispatch() {
    let (_dir, state) = super::tests::team_state();
    let room = state.store.create_room("Cancellation").unwrap();
    let unrelated = state.store.create_room("Unrelated").unwrap();
    let agent = state.store.add_agent("Reviewer", "A").unwrap();
    state
        .store
        .append_message(room.id, MessageSender::User, "Do not send on cancel")
        .unwrap();
    let provider = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Never contacted".into(),
        model_id: "fixture".into(),
        base_url: "http://127.0.0.1:11434/v1".into(),
        kind: ProviderKind::OpenaiChat,
        credentials: CredentialSource::None,
    };
    state.store.save_provider_profile(provider.clone()).unwrap();

    let (lease, cancellation) =
        register_room_stream(&state.room_stream_cancellations, room.id).unwrap();
    assert!(register_room_stream(&state.room_stream_cancellations, room.id).is_err());
    assert!(!cancel_room_stream(&state.room_stream_cancellations, unrelated.id).unwrap());
    assert!(cancel_room_stream(&state.room_stream_cancellations, room.id).unwrap());
    let batch = tauri::async_runtime::block_on(run_team_batch_with(
        &state,
        room.id,
        &[agent.id],
        provider.id,
        |_, _| {
            let cancellation = cancellation.clone();
            async move {
                await_cancellable_task_request(
                    async { panic!("cancelled stream must not poll a billable request") },
                    cancellation,
                )
                .await
            }
        },
    ))
    .unwrap();
    assert_eq!(batch.results.len(), 1);
    assert_eq!(batch.results[0].status, TeamReplyStatus::Failed);
    assert_eq!(state.store.snapshot().unwrap().messages.len(), 1);
    drop(lease);
    assert!(!cancel_room_stream(&state.room_stream_cancellations, room.id).unwrap());
    let (next_lease, next) =
        register_room_stream(&state.room_stream_cancellations, room.id).unwrap();
    assert!(
        !*next.borrow(),
        "old cancellation must not poison next request"
    );
    drop(next_lease);
}

#[test]
fn cancellation_interrupts_an_actual_pending_http_stream_after_partial_text() {
    let (_dir, state) = super::tests::team_state();
    let room = state.store.create_room("Interrupted stream").unwrap();
    let agent = state.store.add_agent("Reviewer", "A").unwrap();
    state
        .store
        .append_message(room.id, MessageSender::User, "Stop this local fixture")
        .unwrap();
    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = server.local_addr().unwrap().port();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let server_thread = thread::spawn(move || {
        let (mut socket, _) = server.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let count = socket.read(&mut buf).unwrap();
            assert!(count != 0, "client disconnected before request");
            request.extend_from_slice(&buf[..count]);
            if request.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        let chunk = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"preview only\"},\"finish_reason\":null}]}\n\n";
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
        socket
            .write_all(format!("{:X}\r\n", chunk.len()).as_bytes())
            .unwrap();
        socket.write_all(chunk).unwrap();
        socket.write_all(b"\r\n").unwrap();
        socket.flush().unwrap();
        // Keep the remote server waiting. HTTP has not completed and only the
        // user's Stop signal can release the native request future promptly.
        let _ = release_rx.recv_timeout(Duration::from_secs(3));
    });
    let provider = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Cancellable loopback fixture".into(),
        model_id: "fixture".into(),
        base_url: format!("http://127.0.0.1:{port}/v1"),
        kind: ProviderKind::OpenaiChat,
        credentials: CredentialSource::None,
    };
    state.store.save_provider_profile(provider.clone()).unwrap();
    let (_lease, cancellation) =
        register_room_stream(&state.room_stream_cancellations, room.id).unwrap();
    let active_signals = &state.room_stream_cancellations;
    let target_room = room.id;
    let preview = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let started = Instant::now();
    let batch = tauri::async_runtime::block_on(run_team_batch_with(
        &state,
        room.id,
        &[agent.id],
        provider.id,
        |_, history| {
            let preview = std::sync::Arc::clone(&preview);
            let cancellation = cancellation.clone();
            let profile = provider.clone();
            async move {
                await_cancellable_task_request(
                    model_stream_reply(&profile, None, history, move |part| {
                        preview.lock().unwrap().push_str(&part);
                        cancel_room_stream(active_signals, target_room).unwrap();
                    }),
                    cancellation,
                )
                .await
            }
        },
    ))
    .unwrap();
    let _ = release_tx.send(());
    server_thread.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(preview.lock().unwrap().as_str(), "preview only");
    assert_eq!(batch.results[0].status, TeamReplyStatus::Failed);
    assert!(
        batch.results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("cancelled")
    );
    let messages = state.store.snapshot().unwrap().messages;
    assert_eq!(messages.len(), 1, "cancelled preview cannot be persisted");
    assert!(matches!(messages[0].sender, MessageSender::User));
}
