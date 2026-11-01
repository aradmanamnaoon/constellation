use constellation_core::{
    ChatEntry, ChatRole, CredentialSource, ProviderId, ProviderKind, ProviderProfile,
};
use constellation_provider_adapters::{
    AdapterError, Authentication, MAX_INPUT_BYTES, MAX_RESPONSE_BYTES, parse_response, parse_text,
    prepare,
};
use serde_json::{Value, json};

fn profile(kind: ProviderKind) -> ProviderProfile {
    ProviderProfile {
        id: ProviderId::new(),
        display_name: "Test adapter".into(),
        model_id: "model-with-safe-name".into(),
        base_url: match kind {
            ProviderKind::AnthropicMessages => "https://api.anthropic.com/v1",
            ProviderKind::GoogleGemini => "https://generativelanguage.googleapis.com/v1beta",
            ProviderKind::OllamaNative => "http://localhost:11434",
            _ => "https://providers.example.com/v1",
        }
        .into(),
        kind,
        credentials: CredentialSource::None,
    }
}

fn history() -> Vec<ChatEntry> {
    vec![
        ChatEntry {
            role: ChatRole::System,
            content: "Follow instructions".into(),
        },
        ChatEntry {
            role: ChatRole::User,
            content: "First question".into(),
        },
        ChatEntry {
            role: ChatRole::Assistant,
            content: "First answer".into(),
        },
        ChatEntry {
            role: ChatRole::User,
            content: "Next question".into(),
        },
    ]
}

fn as_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

#[test]
fn variants_serialize_to_exact_frontend_and_persistent_names() {
    for (kind, wire) in [
        (ProviderKind::OpenaiChat, "openai_chat"),
        (ProviderKind::OpenaiResponses, "openai_responses"),
        (ProviderKind::AnthropicMessages, "anthropic_messages"),
        (ProviderKind::GoogleGemini, "google_gemini"),
        (ProviderKind::OllamaNative, "ollama_native"),
        (ProviderKind::CustomCompatible, "custom_compatible"),
    ] {
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{wire}\""));
        assert_eq!(
            serde_json::from_str::<ProviderKind>(&format!("\"{wire}\"")).unwrap(),
            kind
        );
    }
}

#[test]
fn openai_chat_and_custom_compatible_use_expected_text_only_payloads() {
    for kind in [ProviderKind::OpenaiChat, ProviderKind::CustomCompatible] {
        let request = prepare(&profile(kind), &history()).unwrap();
        assert_eq!(
            request.endpoint.as_str(),
            "https://providers.example.com/v1/chat/completions"
        );
        assert_eq!(request.authentication, Authentication::Bearer);
        assert_eq!(
            request.body["messages"][0],
            json!({"role":"system","content":"Follow instructions"})
        );
        assert_eq!(
            request.body["messages"][2],
            json!({"role":"assistant","content":"First answer"})
        );
        assert_eq!(request.body["max_tokens"], 512);
        assert_eq!(request.body["stream"], false);
        assert!(request.body.get("tools").is_none());
    }
}

#[test]
fn openai_responses_separates_system_instructions_and_disables_remote_storage() {
    let request = prepare(&profile(ProviderKind::OpenaiResponses), &history()).unwrap();
    assert_eq!(request.endpoint.path(), "/v1/responses");
    assert_eq!(request.body["instructions"], "Follow instructions");
    assert_eq!(request.body["input"].as_array().unwrap().len(), 3);
    assert_eq!(
        request.body["input"][1],
        json!({"role":"assistant","content":"First answer"})
    );
    assert_eq!(request.body["max_output_tokens"], 512);
    assert_eq!(request.body["store"], false);
    assert_eq!(request.body["stream"], false);
    assert!(request.body.get("tools").is_none());
}

#[test]
fn anthropic_messages_uses_system_outside_messages_and_explicit_auth_metadata() {
    let request = prepare(&profile(ProviderKind::AnthropicMessages), &history()).unwrap();
    assert_eq!(
        request.endpoint.as_str(),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(request.authentication, Authentication::AnthropicKey);
    assert!(
        request
            .protocol_headers
            .contains(&("anthropic-version", "2026-10-21"))
    );
    assert_eq!(request.body["system"], "Follow instructions");
    assert_eq!(request.body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(
        request.body["messages"][1],
        json!({"role":"assistant","content":"First answer"})
    );
    assert_eq!(request.body["max_tokens"], 512);
}

#[test]
fn gemini_uses_generate_content_path_model_role_and_generation_settings() {
    let request = prepare(&profile(ProviderKind::GoogleGemini), &history()).unwrap();
    assert_eq!(
        request.endpoint.path(),
        "/v1beta/models/model-with-safe-name:generateContent"
    );
    assert_eq!(request.authentication, Authentication::GeminiKey);
    assert_eq!(
        request.body["systemInstruction"],
        json!({"parts":[{"text":"Follow instructions"}]})
    );
    assert_eq!(request.body["contents"].as_array().unwrap().len(), 3);
    assert_eq!(
        request.body["contents"][1],
        json!({"role":"model","parts":[{"text":"First answer"}]})
    );
    assert_eq!(request.body["generationConfig"]["maxOutputTokens"], 512);
    assert!(request.body.get("tools").is_none());
}

#[test]
fn native_ollama_has_local_nonstreaming_chat_no_auth_requirement() {
    let request = prepare(&profile(ProviderKind::OllamaNative), &history()).unwrap();
    assert_eq!(request.endpoint.as_str(), "http://localhost:11434/api/chat");
    assert_eq!(request.authentication, Authentication::OptionalBearer);
    assert_eq!(
        request.body["messages"][0],
        json!({"role":"system","content":"Follow instructions"})
    );
    assert_eq!(request.body["options"]["num_predict"], 512);
    assert_eq!(request.body["stream"], false);
}

#[test]
fn debug_redacts_model_prompt_and_endpoint_data() {
    let mut messages = history();
    messages.last_mut().unwrap().content = "private patient history".into();
    let request = prepare(&profile(ProviderKind::AnthropicMessages), &messages).unwrap();
    let debug = format!("{request:?}");
    for secret in [
        "private patient history",
        "api.anthropic.com",
        "model-with-safe-name",
    ] {
        assert!(!debug.contains(secret));
    }
    assert!(debug.contains("[redacted]"));
}

#[test]
fn invalid_endpoints_and_local_network_pivots_are_denied() {
    for unsafe_url in [
        "http://public.example.com/v1",
        "https://alice:password@public.example.com/v1",
        "https://public.example.com/v1?token=secret",
        "https://public.example.com/v1#fragment",
        "https://public.example.com/v1%2Fextra",
        "https://public.example.com/v1/../admin",
        "https://public.example.com/v1//admin",
        "https://10.0.0.1/v1",
        "https://host.local/v1",
        "https://host.internal/v1",
        "http://192.168.1.1/v1",
        "https://public.example.com/v1\\z",
        "https://public.example.com/v1/chat/completions",
    ] {
        let mut profile = profile(ProviderKind::OpenaiChat);
        profile.base_url = unsafe_url.into();
        assert!(prepare(&profile, &history()).is_err(), "{unsafe_url}");
    }
}

#[test]
fn exact_loopback_base_and_custom_path_prefix_are_allowed() {
    for base in [
        "http://localhost:11434",
        "http://127.0.0.1:11434",
        "http://[::1]:11434",
        "https://127.0.0.1:11434",
        "https://[::1]:11434",
    ] {
        let mut p = profile(ProviderKind::OllamaNative);
        p.base_url = base.into();
        assert_eq!(
            prepare(&p, &history()).unwrap().endpoint.path(),
            "/api/chat"
        );
    }
    let mut p = profile(ProviderKind::CustomCompatible);
    p.base_url = "https://gateway.example.com/api/customer/v1".into();
    assert_eq!(
        prepare(&p, &history()).unwrap().endpoint.path(),
        "/api/customer/v1/chat/completions"
    );
}

#[test]
fn oversized_history_and_late_system_are_rejected_but_team_history_is_supported() {
    let p = profile(ProviderKind::CustomCompatible);
    assert_eq!(prepare(&p, &[]).unwrap_err(), AdapterError::InvalidHistory);
    assert_eq!(
        prepare(
            &p,
            &[ChatEntry {
                role: ChatRole::User,
                content: String::new()
            }]
        )
        .unwrap_err(),
        AdapterError::InvalidHistory
    );
    let long = vec![ChatEntry {
        role: ChatRole::User,
        content: "x".repeat(MAX_INPUT_BYTES + 1),
    }];
    assert_eq!(prepare(&p, &long).unwrap_err(), AdapterError::TooLarge);
    let mut wrong = history();
    wrong.push(ChatEntry {
        role: ChatRole::System,
        content: "late override".into(),
    });
    assert_eq!(
        prepare(&p, &wrong).unwrap_err(),
        AdapterError::InvalidHistory
    );
    let mut wrong = history();
    wrong.push(ChatEntry {
        role: ChatRole::Assistant,
        content: "assistant last".into(),
    });
    assert!(prepare(&p, &wrong).is_ok());
    let overlong = vec![
        ChatEntry {
            role: ChatRole::User,
            content: "a".into()
        };
        65
    ];
    assert_eq!(
        prepare(&p, &overlong).unwrap_err(),
        AdapterError::InvalidHistory
    );
}

#[test]
fn rejects_model_id_path_traversal_and_invalid_unicode_control() {
    let mut p = profile(ProviderKind::GoogleGemini);
    for model in ["../traversal", "models/example", "with spaces", "a%2Fb", ""] {
        p.model_id = model.into();
        assert_eq!(
            prepare(&p, &history()).unwrap_err(),
            AdapterError::InvalidConfiguration
        );
    }
    let mut p = profile(ProviderKind::OpenaiChat);
    p.model_id = "model\nX-Header: injected".into();
    assert_eq!(
        prepare(&p, &history()).unwrap_err(),
        AdapterError::InvalidConfiguration
    );
}

#[test]
fn parses_all_six_protocols_real_text_and_preserves_unicode() {
    let cases = [
        (
            ProviderKind::OpenaiChat,
            json!({"choices":[{"finish_reason":"stop","message":{"content":"Hello دنیا"}}]}),
            "Hello دنیا",
        ),
        (
            ProviderKind::CustomCompatible,
            json!({"choices":[{"finish_reason":"stop","message":{"content":"Compatible"}}]}),
            "Compatible",
        ),
        (
            ProviderKind::OpenaiResponses,
            json!({"status":"completed","output":[
            {"type":"reasoning","summary":[]},
            {"type":"message","status":"completed","content":[{"type":"output_text","text":"Responses"}]}]}),
            "Responses",
        ),
        (
            ProviderKind::AnthropicMessages,
            json!({"stop_reason":"end_turn","content":[
            {"type":"text","text":"Anthropic"}]}),
            "Anthropic",
        ),
        (
            ProviderKind::GoogleGemini,
            json!({"candidates":[{"finishReason":"STOP","content":{"parts":[
            {"thought":true,"text":"not user-facing"},{"text":"Gemini"}]}}]}),
            "Gemini",
        ),
        (
            ProviderKind::OllamaNative,
            json!({"done":true,"message":{"role":"assistant","content":"Ollama"}}),
            "Ollama",
        ),
    ];
    for (kind, response, expected) in cases {
        assert_eq!(parse_text(kind, &as_bytes(&response)).unwrap(), expected);
        assert_eq!(
            parse_response(kind, 200, &as_bytes(&response)).unwrap(),
            expected
        );
    }
}

#[test]
fn multiple_text_parts_concatenate_without_omitting_or_fabricating_text() {
    let anthropic = json!({"stop_reason":"end_turn","content":[{"type":"text","text":"Part A "},{"type":"text","text":"Part B"}]});
    assert_eq!(
        parse_text(ProviderKind::AnthropicMessages, &as_bytes(&anthropic)).unwrap(),
        "Part A Part B"
    );
    let openai = json!({"status":"completed","output":[{"type":"message","status":"completed","content":[
        {"type":"output_text","text":"First"},{"type":"output_text","text":" second"}]}]});
    assert_eq!(
        parse_text(ProviderKind::OpenaiResponses, &as_bytes(&openai)).unwrap(),
        "First second"
    );
}

#[test]
fn invalid_json_response_size_and_status_do_not_echo_remote_key_or_body() {
    let kind = ProviderKind::OpenaiChat;
    let error = br#"{"error":{"message":"TOKEN_SHOULD_NOT_APPEAR_IN_LOGS"}}"#;
    assert_eq!(
        parse_response(kind, 403, error),
        Err(AdapterError::HttpStatus(403))
    );
    assert!(
        !parse_response(kind, 403, error)
            .unwrap_err()
            .to_string()
            .contains("TOKEN_SHOULD_NOT_APPEAR_IN_LOGS")
    );
    assert_eq!(parse_text(kind, error), Err(AdapterError::InvalidResponse));
    assert_eq!(
        parse_text(kind, b"not json"),
        Err(AdapterError::InvalidResponse)
    );
    assert_eq!(parse_text(kind, b"[]"), Err(AdapterError::InvalidResponse));
    assert_eq!(
        parse_text(kind, &vec![b'a'; MAX_RESPONSE_BYTES + 1]),
        Err(AdapterError::TooLarge)
    );
    let large = json!({"choices":[{"finish_reason":"stop","message":{"content":"x".repeat(128 * 1024 + 1)}}]});
    assert_eq!(
        parse_text(kind, &as_bytes(&large)),
        Err(AdapterError::TooLarge)
    );
}

#[test]
fn rejects_tool_calls_refusal_and_incomplete_output_without_claiming_success() {
    let fixtures = [
        (
            ProviderKind::OpenaiChat,
            json!({"choices":[{"finish_reason":"tool_calls",
            "message":{"content":null,"tool_calls":[{"function":{"name":"shell"}}]}}]}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::OpenaiChat,
            json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::OpenaiChat,
            json!({"choices":[{"message":{"refusal":"No","content":""}}]}),
            AdapterError::UnsupportedContent,
        ),
        (
            ProviderKind::OpenaiResponses,
            json!({"status":"incomplete","output":[{"type":"message","content":[
            {"type":"output_text","text":"partial"}]}]}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::OpenaiResponses,
            json!({"status":"completed","output":[{"type":"function_call","name":"shell"}]}),
            AdapterError::UnsupportedContent,
        ),
        (
            ProviderKind::AnthropicMessages,
            json!({"stop_reason":"max_tokens","content":[{"type":"text","text":"partial"}]}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::AnthropicMessages,
            json!({"stop_reason":"tool_use","content":[{"type":"tool_use","name":"shell"}]}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::GoogleGemini,
            json!({"candidates":[{"finishReason":"SAFETY",
            "content":{"parts":[{"text":"blocked"}]}}]}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::OllamaNative,
            json!({"done":false,"message":{"content":"partial"}}),
            AdapterError::Incomplete,
        ),
        (
            ProviderKind::OllamaNative,
            json!({"done":true,"message":{"role":"assistant","tool_calls":[{"function":{"name":"shell"}}],"content":"unsafe"}}),
            AdapterError::UnsupportedContent,
        ),
    ];
    for (kind, body, expected) in fixtures {
        assert_eq!(
            parse_text(kind, &as_bytes(&body)),
            Err(expected),
            "{kind:?}"
        );
    }
}

#[test]
fn profile_deserialization_does_not_accept_plaintext_api_key_property() {
    let p = profile(ProviderKind::CustomCompatible);
    let mut as_json = serde_json::to_value(&p).unwrap();
    as_json["api_key"] = json!("should-be-denied");
    assert!(serde_json::from_value::<ProviderProfile>(as_json).is_err());
}
