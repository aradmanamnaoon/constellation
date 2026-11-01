use constellation_core::{
    ChatEntry, ChatRole, CredentialSource, ProviderId, ProviderKind, ProviderProfile,
};
use constellation_provider_adapters::{AdapterError, StreamDecoder, prepare_stream};

fn sse(event: Option<&str>, json: &str) -> String {
    match event {
        Some(name) => format!("event: {name}\ndata: {json}\n\n"),
        None => format!("data: {json}\n\n"),
    }
}

fn feed(kind: ProviderKind, stream: &str) -> Result<String, AdapterError> {
    let mut decoder = StreamDecoder::new(kind)?;
    // Byte-by-byte delivery crosses UTF-8 and SSE field boundaries.
    for byte in stream.bytes() {
        decoder.push(&[byte])?;
    }
    decoder.finish()
}

#[test]
fn chat_deltas_only_complete_after_stop_and_done() {
    let mut stream = sse(
        None,
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hi "},"finish_reason":null}]}"#,
    );
    stream += &sse(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"🌍"},"finish_reason":null}]}"#,
    );
    stream += &sse(
        None,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    );
    stream += "data: [DONE]\n\n";
    assert_eq!(feed(ProviderKind::OpenaiChat, &stream).unwrap(), "Hi 🌍");
    assert_eq!(
        feed(ProviderKind::CustomCompatible, &stream).unwrap(),
        "Hi 🌍"
    );
    assert_eq!(
        feed(ProviderKind::OpenaiChat, &stream[..stream.len() - 2]).unwrap_err(),
        AdapterError::Incomplete
    );
    let without_stop = sse(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"partial"},"finish_reason":null}]}"#,
    ) + "data: [DONE]\n\n";
    assert_eq!(
        feed(ProviderKind::OpenaiChat, &without_stop).unwrap_err(),
        AdapterError::Incomplete
    );
}

#[test]
fn chat_refusal_tools_truncation_and_server_error_fail_closed() {
    let first = sse(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"Looks okay"},"finish_reason":null}]}"#,
    );
    for bad in [
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0}]},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{"refusal":"blocked"},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#,
        r#"{"error":{"message":"private remote error"}}"#,
    ] {
        let error = feed(ProviderKind::OpenaiChat, &(first.clone() + &sse(None, bad))).unwrap_err();
        assert!(matches!(
            error,
            AdapterError::UnsupportedContent
                | AdapterError::Incomplete
                | AdapterError::InvalidResponse
        ));
    }
}

#[test]
fn responses_stream_accepts_text_events_and_requires_completed_status() {
    let text = sse(Some("response.created"), r#"{"type":"response.created"}"#)
        + &sse(
            Some("response.output_text.delta"),
            r#"{"type":"response.output_text.delta","delta":"Done"}"#,
        )
        + &sse(
            Some("response.completed"),
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
        );
    assert_eq!(feed(ProviderKind::OpenaiResponses, &text).unwrap(), "Done");
    let denied = sse(
        Some("response.output_item.added"),
        r#"{"type":"response.output_item.added","item":{"type":"function_call"}}"#,
    );
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &denied).unwrap_err(),
        AdapterError::UnsupportedContent
    );
    let incomplete = sse(
        Some("response.incomplete"),
        r#"{"type":"response.incomplete"}"#,
    );
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &incomplete).unwrap_err(),
        AdapterError::Incomplete
    );
    let late_tool = sse(
        Some("response.output_text.delta"),
        r#"{"type":"response.output_text.delta","delta":"premature"}"#,
    ) + &sse(
        Some("response.completed"),
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call"}]}}"#,
    );
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &late_tool).unwrap_err(),
        AdapterError::UnsupportedContent
    );
    let malformed = sse(
        Some("response.completed"),
        r#"{"type":"response.created","response":{"status":"completed"}}"#,
    );
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &malformed).unwrap_err(),
        AdapterError::InvalidResponse
    );
    let mismatched_terminal = sse(
        Some("response.output_text.delta"),
        r#"{"type":"response.output_text.delta","delta":"streamed answer"}"#,
    ) + &sse(
        Some("response.completed"),
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"different answer"}]}]}}"#,
    );
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &mismatched_terminal).unwrap_err(),
        AdapterError::InvalidResponse
    );
    let matching_terminal = sse(
        Some("response.output_text.delta"),
        r#"{"type":"response.output_text.delta","delta":"verified"}"#,
    ) + &sse(
        Some("response.completed"),
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"verified"}]}]}}"#,
    );
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &matching_terminal).unwrap(),
        "verified"
    );
    let reasoning_then_text = sse(
        Some("response.output_item.added"),
        r#"{"type":"response.output_item.added","item":{"type":"reasoning"}}"#,
    ) + &sse(
        Some("response.output_item.done"),
        r#"{"type":"response.output_item.done","item":{"type":"reasoning"}}"#,
    ) + &matching_terminal;
    assert_eq!(
        feed(ProviderKind::OpenaiResponses, &reasoning_then_text).unwrap(),
        "verified",
        "a reasoning item must not leak private internal text or block answer"
    );
}

#[test]
fn anthropic_handles_thinking_without_exposing_it_and_rejects_tool_use() {
    let mut stream = sse(Some("message_start"), r#"{"type":"message_start"}"#);
    stream += &sse(
        Some("content_block_start"),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
    );
    stream += &sse(
        Some("content_block_delta"),
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private reasoning"}}"#,
    );
    stream += &sse(
        Some("content_block_stop"),
        r#"{"type":"content_block_stop","index":0}"#,
    );
    stream += &sse(
        Some("content_block_start"),
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
    );
    stream += &sse(
        Some("content_block_delta"),
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Safe answer"}}"#,
    );
    stream += &sse(
        Some("content_block_stop"),
        r#"{"type":"content_block_stop","index":1}"#,
    );
    stream += &sse(
        Some("message_delta"),
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
    );
    stream += &sse(Some("message_stop"), r#"{"type":"message_stop"}"#);
    assert_eq!(
        feed(ProviderKind::AnthropicMessages, &stream).unwrap(),
        "Safe answer"
    );
    let bad = sse(
        Some("content_block_start"),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use"}}"#,
    );
    assert_eq!(
        feed(ProviderKind::AnthropicMessages, &bad).unwrap_err(),
        AdapterError::UnsupportedContent
    );
}

#[test]
fn anthropic_preserves_text_from_content_block_start_and_prevents_duplicate_blocks() {
    let start = sse(
        Some("content_block_start"),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Prefix "}}"#,
    );
    let delta = sse(
        Some("content_block_delta"),
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"suffix"}}"#,
    );
    let end = sse(
        Some("content_block_stop"),
        r#"{"type":"content_block_stop","index":0}"#,
    );
    let complete = start.clone()
        + &delta
        + &end
        + &sse(
            Some("message_delta"),
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
        )
        + &sse(Some("message_stop"), r#"{"type":"message_stop"}"#);
    assert_eq!(
        feed(ProviderKind::AnthropicMessages, &complete).unwrap(),
        "Prefix suffix"
    );
    assert_eq!(
        feed(ProviderKind::AnthropicMessages, &(start.clone() + &start)).unwrap_err(),
        AdapterError::InvalidResponse
    );
}

#[test]
fn ollama_ndjson_requires_done_and_no_tools() {
    let ok = "{\"message\":{\"role\":\"assistant\",\"content\":\"Hello \"},\"done\":false}\n{\"message\":{\"role\":\"assistant\",\"content\":\"world\"},\"done\":true,\"done_reason\":\"stop\"}\n";
    assert_eq!(feed(ProviderKind::OllamaNative, ok).unwrap(), "Hello world");
    assert_eq!(
        feed(
            ProviderKind::OllamaNative,
            "{\"message\":{\"role\":\"assistant\",\"content\":\"partial\"},\"done\":false}\n"
        )
        .unwrap_err(),
        AdapterError::Incomplete
    );
    assert_eq!(
        feed(
            ProviderKind::OllamaNative,
            "{\"message\":{\"role\":\"assistant\",\"tool_calls\":[{}]},\"done\":true}\n"
        )
        .unwrap_err(),
        AdapterError::UnsupportedContent
    );
    assert_eq!(
        feed(
            ProviderKind::OllamaNative,
            "{\"message\":{\"role\":\"user\",\"content\":\"spoofed\"},\"done\":true}\n"
        )
        .unwrap_err(),
        AdapterError::UnsupportedContent
    );
}

#[test]
fn rejects_oversized_chunks_and_data_after_terminal_event() {
    let mut decoder = StreamDecoder::new(ProviderKind::OpenaiChat).unwrap();
    assert_eq!(
        decoder.push(&vec![b'x'; 1024 * 1024 + 1]).unwrap_err(),
        AdapterError::TooLarge
    );
    let mut decoder = StreamDecoder::new(ProviderKind::OpenaiChat).unwrap();
    assert_eq!(
        decoder.push(&vec![b'x'; 128 * 1024 + 1]).unwrap_err(),
        AdapterError::TooLarge
    );
    let complete = sse(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"safe"},"finish_reason":"stop"}]}"#,
    ) + "data: [DONE]\n\n"
        + &sse(
            None,
            r#"{"choices":[{"index":0,"delta":{"content":"unexpected"},"finish_reason":null}]}"#,
        );
    assert_eq!(
        feed(ProviderKind::OpenaiChat, &complete).unwrap_err(),
        AdapterError::InvalidResponse
    );
}

#[test]
fn prepare_stream_enables_only_implemented_protocols() {
    let history = [ChatEntry {
        role: ChatRole::User,
        content: "Hello".into(),
    }];
    for kind in [
        ProviderKind::OpenaiChat,
        ProviderKind::CustomCompatible,
        ProviderKind::OpenaiResponses,
        ProviderKind::AnthropicMessages,
        ProviderKind::OllamaNative,
    ] {
        let profile = ProviderProfile {
            id: ProviderId::new(),
            display_name: "Test".into(),
            model_id: "sample".into(),
            base_url: "https://example.com/v1".into(),
            kind,
            credentials: CredentialSource::None,
        };
        let prepared = prepare_stream(&profile, &history).unwrap();
        assert_eq!(prepared.body["stream"], true);
    }
    let unsupported = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Test".into(),
        model_id: "sample".into(),
        base_url: "https://example.com/v1".into(),
        kind: ProviderKind::GoogleGemini,
        credentials: CredentialSource::None,
    };
    assert_eq!(
        prepare_stream(&unsupported, &history).unwrap_err(),
        AdapterError::UnsupportedContent
    );
}
