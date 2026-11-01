//! A provider's terminal markers are security-relevant: later text cannot
//! become a successful model answer, even when received in the same HTTP chunk.
use constellation_core::ProviderKind;
use constellation_provider_adapters::{AdapterError, MAX_RESPONSE_BYTES, StreamDecoder};

fn event(name: Option<&str>, payload: &str) -> String {
    match name {
        Some(name) => format!("event: {name}\ndata: {payload}\n\n"),
        None => format!("data: {payload}\n\n"),
    }
}

fn feed(kind: ProviderKind, input: &str) -> Result<String, AdapterError> {
    let mut decoder = StreamDecoder::new(kind)?;
    decoder.push(input.as_bytes())?;
    decoder.finish()
}

#[test]
fn openai_chat_stop_cannot_be_followed_by_more_model_content() {
    let text = event(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"trusted"},"finish_reason":null}]}"#,
    );
    let stop = event(
        None,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    );
    let late = event(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":" hidden suffix"},"finish_reason":null}]}"#,
    );
    let done = "data: [DONE]\n\n";
    for kind in [ProviderKind::OpenaiChat, ProviderKind::CustomCompatible] {
        assert_eq!(
            feed(kind, &(text.clone() + &stop + done)).unwrap(),
            "trusted"
        );
        assert_eq!(
            feed(kind, &(text.clone() + &stop + &late + done)).unwrap_err(),
            AdapterError::InvalidResponse,
        );
        // A terminal usage event with empty choices remains permissible.
        let usage = event(None, r#"{"choices":[],"usage":{"total_tokens":3}}"#);
        assert_eq!(
            feed(kind, &(text.clone() + &stop + &usage + done)).unwrap(),
            "trusted"
        );
    }
}

#[test]
fn anthropic_stop_reason_prohibits_late_content_block() {
    let begin = event(
        Some("content_block_start"),
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"trusted"}}"#,
    );
    let end = event(
        Some("content_block_stop"),
        r#"{"type":"content_block_stop","index":0}"#,
    );
    let stop_reason = event(
        Some("message_delta"),
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
    );
    let late = event(
        Some("content_block_start"),
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":" hidden suffix"}}"#,
    );
    let close_late = event(
        Some("content_block_stop"),
        r#"{"type":"content_block_stop","index":1}"#,
    );
    let finish = event(Some("message_stop"), r#"{"type":"message_stop"}"#);
    assert_eq!(
        feed(
            ProviderKind::AnthropicMessages,
            &(begin.clone() + &end + &stop_reason + &finish),
        )
        .unwrap(),
        "trusted"
    );
    assert_eq!(
        feed(
            ProviderKind::AnthropicMessages,
            &(begin + &end + &stop_reason + &late + &close_late + &finish),
        )
        .unwrap_err(),
        AdapterError::InvalidResponse
    );
}

#[test]
fn decoder_remains_failed_after_an_error_even_if_caller_attempts_finish() {
    for (kind, complete) in [
        (
            ProviderKind::OpenaiChat,
            event(None, r#"{"choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#)
                + &event(None, r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#)
                + "data: [DONE]\n\n",
        ),
        (
            ProviderKind::OpenaiResponses,
            event(
                Some("response.output_text.delta"),
                r#"{"type":"response.output_text.delta","delta":"ok"}"#,
            ) + &event(
                Some("response.completed"),
                r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            ),
        ),
        (
            ProviderKind::AnthropicMessages,
            event(
                Some("content_block_start"),
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"ok"}}"#,
            ) + &event(
                Some("content_block_stop"),
                r#"{"type":"content_block_stop","index":0}"#,
            ) + &event(
                Some("message_delta"),
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            ) + &event(Some("message_stop"), r#"{"type":"message_stop"}"#),
        ),
        (
            ProviderKind::OllamaNative,
            "{\"message\":{\"role\":\"assistant\",\"content\":\"ok\"},\"done\":true,\"done_reason\":\"stop\"}\n".into(),
        ),
    ] {
        let mut decoder = StreamDecoder::new(kind).unwrap();
        decoder.push(complete.as_bytes()).unwrap();
        let error = decoder.push(b"data: invalid\n\n").unwrap_err();
        assert_eq!(error, AdapterError::InvalidResponse);
        assert_eq!(decoder.push(b"").unwrap_err(), error);
        assert_eq!(decoder.finish().unwrap_err(), error);
    }
}

#[test]
fn trailing_duplicate_terminal_inside_one_transport_chunk_is_rejected() {
    let valid = event(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#,
    ) + &event(
        None,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    ) + "data: [DONE]\n\n";
    assert_eq!(
        feed(
            ProviderKind::OpenaiChat,
            &(valid.clone() + "data: [DONE]\n\n")
        )
        .unwrap_err(),
        AdapterError::InvalidResponse
    );
    assert_eq!(
        feed(ProviderKind::OpenaiChat, &(valid + "\n")).unwrap_err(),
        AdapterError::InvalidResponse
    );
}

#[test]
fn size_failure_is_irrecoverable_even_if_provider_sends_valid_completion_later() {
    let mut decoder = StreamDecoder::new(ProviderKind::OpenaiChat).unwrap();
    assert_eq!(
        decoder
            .push(&vec![b'x'; MAX_RESPONSE_BYTES + 1])
            .unwrap_err(),
        AdapterError::TooLarge
    );
    let valid = event(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#,
    ) + &event(
        None,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    ) + "data: [DONE]\n\n";
    assert_eq!(
        decoder.push(valid.as_bytes()).unwrap_err(),
        AdapterError::TooLarge
    );
    assert_eq!(decoder.finish().unwrap_err(), AdapterError::TooLarge);
}

#[test]
fn late_content_across_separate_http_chunks_poisoned_at_first_violation() {
    let mut decoder = StreamDecoder::new(ProviderKind::OpenaiChat).unwrap();
    let text = event(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"safe"},"finish_reason":null}]}"#,
    );
    let stop = event(
        None,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
    );
    assert_eq!(decoder.push(text.as_bytes()).unwrap(), ["safe"]);
    assert!(decoder.push(stop.as_bytes()).unwrap().is_empty());
    let late = event(
        None,
        r#"{"choices":[{"index":0,"delta":{"content":"unsafe"},"finish_reason":null}]}"#,
    );
    let middle = late.len() / 2;
    assert!(decoder.push(&late.as_bytes()[..middle]).unwrap().is_empty());
    assert_eq!(
        decoder.push(&late.as_bytes()[middle..]).unwrap_err(),
        AdapterError::InvalidResponse,
    );
    assert_eq!(
        decoder.push(b"data: [DONE]\n\n").unwrap_err(),
        AdapterError::InvalidResponse,
    );
    assert_eq!(decoder.finish().unwrap_err(), AdapterError::InvalidResponse);
}
