//! Offline, protocol-specific request builders and response parsers.
//! No HTTP, credentials, environment, or filesystem access happens here.
//! DNS pinning, proxy/redirect prevention and response-size caps belong to the host.

use std::fmt;

use constellation_core::{ChatEntry, ChatRole, ProviderKind, ProviderProfile};
use serde_json::{Value, json};
use url::{Host, Url};

mod stream;
pub use stream::StreamDecoder;

pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_TOKENS: u32 = 512;
pub const MAX_TEXT_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authentication {
    Bearer,
    AnthropicKey,
    GeminiKey,
    OptionalBearer,
}

/// Secrets are intentionally supplied only by the native transport, never to
/// this structure. Debug also redacts private prompts and endpoint paths.
pub struct PreparedChat {
    pub endpoint: Url,
    pub body: Value,
    pub authentication: Authentication,
    pub protocol_headers: Vec<(&'static str, &'static str)>,
}

impl fmt::Debug for PreparedChat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedChat")
            .field("method", &"POST")
            .field("endpoint", &"[redacted]")
            .field("body", &"[redacted]")
            .field("authentication", &self.authentication)
            .field(
                "protocol_header_names",
                &self
                    .protocol_headers
                    .iter()
                    .map(|(key, _)| key)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterError {
    InvalidConfiguration,
    InvalidHistory,
    InvalidResponse,
    Incomplete,
    UnsupportedContent,
    HttpStatus(u16),
    TooLarge,
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidConfiguration => "Invalid provider protocol configuration",
            Self::InvalidHistory => "Invalid or empty model history",
            Self::InvalidResponse => "Model provider returned no valid text",
            Self::Incomplete => "Model provider returned incomplete or blocked output",
            Self::UnsupportedContent => {
                "Model provider returned unsupported tool or nontext output"
            }
            Self::HttpStatus(_) => "Model provider rejected request",
            Self::TooLarge => "Model input or output exceeds the size limit",
        };
        f.write_str(message)
    }
}

impl std::error::Error for AdapterError {}

fn base_url(profile: &ProviderProfile) -> Result<Url, AdapterError> {
    profile
        .validate()
        .map_err(|_| AdapterError::InvalidConfiguration)?;
    let raw = &profile.base_url;
    if raw.len() > 2048
        || raw.trim() != raw
        || raw.contains('%')
        || raw.contains('\\')
        || raw.split('/').any(|piece| piece == "." || piece == "..")
    {
        return Err(AdapterError::InvalidConfiguration);
    }
    let url = Url::parse(raw).map_err(|_| AdapterError::InvalidConfiguration)?;
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().contains("//")
        || url
            .path()
            .split('/')
            .any(|part| part == "." || part == "..")
        || url.port() == Some(0)
    {
        return Err(AdapterError::InvalidConfiguration);
    }
    let loopback = match url.host() {
        Some(Host::Domain(name)) => name == "localhost",
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    // An HTTPS IP literal bypasses DNS-name checks. Let the hardened native
    // transport pin public DNS and reject private DNS answers; only an exact
    // loopback literal/name is allowed as a local provider.
    let public_hostname = match url.host() {
        Some(Host::Domain(name)) => {
            name.contains('.')
                && !name.ends_with(".localhost")
                && !name.ends_with(".local")
                && !name.ends_with(".internal")
                && !name.ends_with(".home.arpa")
        }
        _ => false,
    };
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(AdapterError::InvalidConfiguration);
    }
    if !loopback && !public_hostname {
        return Err(AdapterError::InvalidConfiguration);
    }
    Ok(url)
}

fn appended(mut base: Url, suffix: &str) -> Result<Url, AdapterError> {
    if base.path().ends_with("/chat/completions")
        || base.path().ends_with("/responses")
        || base.path().ends_with("/messages")
        || base.path().ends_with("/api/chat")
    {
        return Err(AdapterError::InvalidConfiguration);
    }
    base.set_path(&format!("{}/{}", base.path().trim_end_matches('/'), suffix));
    Ok(base)
}

fn check_history(history: &[ChatEntry]) -> Result<(), AdapterError> {
    if history.is_empty() || history.len() > 64 {
        return Err(AdapterError::InvalidHistory);
    }
    let size = history
        .iter()
        .try_fold(0usize, |sum, item| sum.checked_add(item.content.len()));
    if size.is_none_or(|n| n > MAX_INPUT_BYTES) {
        return Err(AdapterError::TooLarge);
    }
    if history.iter().any(|item| item.content.trim().is_empty()) {
        return Err(AdapterError::InvalidHistory);
    }
    // A system instruction spliced into an agent conversation should not be
    // silently promoted to a top-level, higher-authority system instruction.
    let mut started_dialogue = false;
    for item in history {
        if item.role == ChatRole::System && started_dialogue {
            return Err(AdapterError::InvalidHistory);
        }
        if item.role != ChatRole::System {
            started_dialogue = true;
        }
    }
    // Multi-agent room history can end with another agent's assistant turn
    // while the same latest USER request is still awaiting this agent's answer.
    // Keep that history intact rather than rejecting all subsequent agents.
    Ok(())
}

fn role_name(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    }
}

pub fn prepare(
    profile: &ProviderProfile,
    history: &[ChatEntry],
) -> Result<PreparedChat, AdapterError> {
    check_history(history)?;
    let base = base_url(profile)?;
    let model = profile.model_id.as_str();
    if model.is_empty()
        || model.len() > 256
        || model.trim() != model
        || model.chars().any(char::is_control)
    {
        return Err(AdapterError::InvalidConfiguration);
    }
    let messages: Vec<Value> = history
        .iter()
        .map(|message| json!({"role":role_name(message.role),"content":message.content}))
        .collect();
    let (endpoint, body, authentication, protocol_headers) = match profile.kind {
        ProviderKind::OpenaiChat | ProviderKind::CustomCompatible => (
            appended(base, "chat/completions")?,
            json!({"model":model, "messages":messages, "max_tokens":MAX_OUTPUT_TOKENS, "stream":false}),
            Authentication::Bearer,
            vec![],
        ),
        ProviderKind::OpenaiResponses => {
            let system = history
                .iter()
                .filter(|m| m.role == ChatRole::System)
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let input: Vec<Value> = history
                .iter()
                .filter(|m| m.role != ChatRole::System)
                .map(|m| json!({"role":role_name(m.role),"content":m.content}))
                .collect();
            let mut body = json!({"model":model, "input":input,
                "max_output_tokens":MAX_OUTPUT_TOKENS,"store":false,"stream":false});
            if !system.is_empty() {
                body["instructions"] = json!(system);
            }
            (
                appended(base, "responses")?,
                body,
                Authentication::Bearer,
                vec![],
            )
        }
        ProviderKind::AnthropicMessages => {
            let system = history
                .iter()
                .filter(|m| m.role == ChatRole::System)
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let messages: Vec<Value> = history
                .iter()
                .filter(|m| m.role != ChatRole::System)
                .map(|m| json!({"role":role_name(m.role), "content":m.content}))
                .collect();
            if messages.is_empty() {
                return Err(AdapterError::InvalidHistory);
            }
            (
                appended(base, "messages")?,
                json!({"model":model, "max_tokens":MAX_OUTPUT_TOKENS,"system":system,"messages":messages}),
                Authentication::AnthropicKey,
                vec![("anthropic-version", "2026-10-21")],
            )
        }
        ProviderKind::GoogleGemini => {
            if model.is_empty()
                || model.len() > 256
                || !model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(AdapterError::InvalidConfiguration);
            }
            let system = history
                .iter()
                .filter(|m| m.role == ChatRole::System)
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let contents: Vec<Value> = history
                .iter()
                .filter(|m| m.role != ChatRole::System)
                .map(|m| {
                    json!({
                        "role": if m.role == ChatRole::Assistant {"model"} else {"user"},
                        "parts":[{"text":m.content}]
                    })
                })
                .collect();
            if contents.is_empty() {
                return Err(AdapterError::InvalidHistory);
            }
            (
                appended(base, &format!("models/{model}:generateContent"))?,
                json!({"contents":contents, "systemInstruction":{"parts":[{"text":system}]},
                    "generationConfig":{"maxOutputTokens":MAX_OUTPUT_TOKENS}}),
                Authentication::GeminiKey,
                vec![],
            )
        }
        ProviderKind::OllamaNative => (
            appended(base, "api/chat")?,
            json!({"model":model,"messages":messages,"stream":false,
                "options":{"num_predict":MAX_OUTPUT_TOKENS}}),
            Authentication::OptionalBearer,
            vec![],
        ),
    };
    Ok(PreparedChat {
        endpoint,
        body,
        authentication,
        protocol_headers,
    })
}

/// Prepare an opt-in streaming request. The native transport remains responsible
/// for authenticated HTTP, destination pinning, timeout, response status,
/// cancellation and delivering bounded chunks to `StreamDecoder`.
/// Unsupported protocols fail closed until their wire contracts are tested.
pub fn prepare_stream(
    profile: &ProviderProfile,
    history: &[ChatEntry],
) -> Result<PreparedChat, AdapterError> {
    if !matches!(
        profile.kind,
        ProviderKind::OpenaiChat
            | ProviderKind::CustomCompatible
            | ProviderKind::OpenaiResponses
            | ProviderKind::AnthropicMessages
            | ProviderKind::OllamaNative
    ) {
        return Err(AdapterError::UnsupportedContent);
    }
    let mut request = prepare(profile, history)?;
    request.body["stream"] = json!(true);
    Ok(request)
}

/// An optional native transport convenience; never includes the server body
/// in HTTP errors. The native client must still cap bytes before collecting.
pub fn parse_response(
    kind: ProviderKind,
    status: u16,
    bytes: &[u8],
) -> Result<String, AdapterError> {
    if !(200..300).contains(&status) {
        return Err(AdapterError::HttpStatus(status));
    }
    parse_text(kind, bytes)
}

/// Parse one *complete*, nonstreaming provider JSON response. Never interpret
/// tool calls, refusals, nontext content or incomplete output as success.
pub fn parse_text(kind: ProviderKind, bytes: &[u8]) -> Result<String, AdapterError> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AdapterError::TooLarge);
    }
    let response: Value =
        serde_json::from_slice(bytes).map_err(|_| AdapterError::InvalidResponse)?;
    if !response.is_object() || response.get("error").is_some_and(|e| !e.is_null()) {
        return Err(AdapterError::InvalidResponse);
    }
    let chunks: Vec<&str> = match kind {
        ProviderKind::OpenaiChat | ProviderKind::CustomCompatible => {
            let choices = response
                .get("choices")
                .and_then(Value::as_array)
                .ok_or(AdapterError::InvalidResponse)?;
            if choices.len() != 1 {
                return Err(AdapterError::UnsupportedContent);
            }
            let choice = &choices[0];
            if matches!(
                choice.get("finish_reason").and_then(Value::as_str),
                Some("length" | "content_filter" | "tool_calls")
            ) {
                return Err(AdapterError::Incomplete);
            }
            let message = choice.get("message").ok_or(AdapterError::InvalidResponse)?;
            if message
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| role != "assistant")
            {
                return Err(AdapterError::UnsupportedContent);
            }
            if message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
                || message.get("function_call").is_some_and(|v| !v.is_null())
                || message.get("refusal").is_some_and(|v| !v.is_null())
            {
                return Err(AdapterError::UnsupportedContent);
            }
            // A textual answer without an explicit successful finish reason
            // may be a truncated response, even if HTTP returned 200.
            if choice.get("finish_reason").and_then(Value::as_str) != Some("stop") {
                return Err(AdapterError::Incomplete);
            }
            vec![
                message
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or(AdapterError::InvalidResponse)?,
            ]
        }
        ProviderKind::OpenaiResponses => {
            if response.get("status").and_then(Value::as_str) != Some("completed") {
                return Err(AdapterError::Incomplete);
            }
            let mut chunks = Vec::new();
            if let Some(output) = response.get("output").and_then(Value::as_array) {
                for item in output {
                    match item.get("type").and_then(Value::as_str) {
                        Some("reasoning") => continue, // Never return private reasoning.
                        Some("message") => {
                            if item
                                .get("role")
                                .and_then(Value::as_str)
                                .is_some_and(|role| role != "assistant")
                            {
                                return Err(AdapterError::UnsupportedContent);
                            }
                        }
                        _ => return Err(AdapterError::UnsupportedContent),
                    }
                    if item.get("status").and_then(Value::as_str) != Some("completed") {
                        return Err(AdapterError::Incomplete);
                    }
                    let content = item
                        .get("content")
                        .and_then(Value::as_array)
                        .ok_or(AdapterError::InvalidResponse)?;
                    for block in content {
                        if block.get("type").and_then(Value::as_str) != Some("output_text") {
                            return Err(AdapterError::UnsupportedContent);
                        }
                        chunks.push(
                            block
                                .get("text")
                                .and_then(Value::as_str)
                                .ok_or(AdapterError::InvalidResponse)?,
                        );
                    }
                }
            } else {
                // Some compatible providers return the SDK convenience field.
                chunks.push(
                    response
                        .get("output_text")
                        .and_then(Value::as_str)
                        .ok_or(AdapterError::InvalidResponse)?,
                );
            }
            chunks
        }
        ProviderKind::AnthropicMessages => {
            if response
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| role != "assistant")
            {
                return Err(AdapterError::UnsupportedContent);
            }
            if !matches!(
                response.get("stop_reason").and_then(Value::as_str),
                Some("end_turn" | "stop_sequence")
            ) {
                return Err(AdapterError::Incomplete);
            }
            let mut chunks = Vec::new();
            for block in response
                .get("content")
                .and_then(Value::as_array)
                .ok_or(AdapterError::InvalidResponse)?
            {
                if block.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(AdapterError::UnsupportedContent);
                }
                chunks.push(
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or(AdapterError::InvalidResponse)?,
                );
            }
            chunks
        }
        ProviderKind::GoogleGemini => {
            let candidates = response
                .get("candidates")
                .and_then(Value::as_array)
                .ok_or(AdapterError::InvalidResponse)?;
            if candidates.len() != 1 {
                return Err(AdapterError::UnsupportedContent);
            }
            let candidate = &candidates[0];
            if candidate
                .pointer("/content/role")
                .and_then(Value::as_str)
                .is_some_and(|role| role != "model")
            {
                return Err(AdapterError::UnsupportedContent);
            }
            if candidate.get("finishReason").and_then(Value::as_str) != Some("STOP") {
                return Err(AdapterError::Incomplete);
            }
            let mut chunks = Vec::new();
            for part in candidate
                .pointer("/content/parts")
                .and_then(Value::as_array)
                .ok_or(AdapterError::InvalidResponse)?
            {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                chunks.push(
                    part.get("text")
                        .and_then(Value::as_str)
                        .ok_or(AdapterError::UnsupportedContent)?,
                );
            }
            chunks
        }
        ProviderKind::OllamaNative => {
            if response.get("done") != Some(&Value::Bool(true))
                || response
                    .get("done_reason")
                    .and_then(Value::as_str)
                    .is_some_and(|reason| reason != "stop")
            {
                return Err(AdapterError::Incomplete);
            }
            if response.pointer("/message/role").and_then(Value::as_str) != Some("assistant") {
                return Err(AdapterError::UnsupportedContent);
            }
            if response
                .pointer("/message/tool_calls")
                .is_some_and(|calls| !calls.is_null())
            {
                return Err(AdapterError::UnsupportedContent);
            }
            vec![
                response
                    .pointer("/message/content")
                    .and_then(Value::as_str)
                    .ok_or(AdapterError::InvalidResponse)?,
            ]
        }
    };
    if chunks.is_empty() {
        return Err(AdapterError::InvalidResponse);
    }
    let mut text = String::new();
    for chunk in chunks {
        if text.len().saturating_add(chunk.len()) > MAX_TEXT_BYTES {
            return Err(AdapterError::TooLarge);
        }
        text.push_str(chunk);
    }
    if text.trim().is_empty() {
        return Err(AdapterError::InvalidResponse);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_core::{CredentialSource, ProviderId};

    fn profile(kind: ProviderKind) -> ProviderProfile {
        ProviderProfile {
            id: ProviderId::new(),
            display_name: "Provider".into(),
            model_id: "test-model".into(),
            base_url: match kind {
                ProviderKind::OllamaNative => "http://127.0.0.1:11434",
                ProviderKind::GoogleGemini => "https://generativelanguage.googleapis.com/v1beta",
                _ => "https://example.com/v1",
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
                content: "Protect data".into(),
            },
            ChatEntry {
                role: ChatRole::User,
                content: "Hello".into(),
            },
        ]
    }

    #[test]
    fn request_payload_and_authentication_match_each_protocol() {
        let cases = [
            (
                ProviderKind::OpenaiChat,
                "/v1/chat/completions",
                Authentication::Bearer,
            ),
            (
                ProviderKind::OpenaiResponses,
                "/v1/responses",
                Authentication::Bearer,
            ),
            (
                ProviderKind::AnthropicMessages,
                "/v1/messages",
                Authentication::AnthropicKey,
            ),
            (
                ProviderKind::GoogleGemini,
                "/v1beta/models/test-model:generateContent",
                Authentication::GeminiKey,
            ),
            (
                ProviderKind::OllamaNative,
                "/api/chat",
                Authentication::OptionalBearer,
            ),
            (
                ProviderKind::CustomCompatible,
                "/v1/chat/completions",
                Authentication::Bearer,
            ),
        ];
        for (kind, expected_path, expected_auth) in cases {
            let prepared = prepare(&profile(kind), &history()).unwrap();
            assert_eq!(prepared.endpoint.path(), expected_path);
            assert_eq!(prepared.authentication, expected_auth);
            assert!(prepared.body.to_string().contains("Hello"));
            assert!(!format!("{prepared:?}").contains("secret-token"));
        }
    }

    #[test]
    fn parses_native_text_and_rejects_malformed_or_empty_responses() {
        let cases = [
            (
                ProviderKind::OpenaiChat,
                r#"{"choices":[{"finish_reason":"stop","message":{"content":"one"}}]}"#,
            ),
            (
                ProviderKind::OpenaiResponses,
                r#"{"status":"completed","output":[{"type":"message","status":"completed","content":[{"type":"output_text","text":"one"}]}]}"#,
            ),
            (
                ProviderKind::AnthropicMessages,
                r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"one"}]}"#,
            ),
            (
                ProviderKind::GoogleGemini,
                r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"one"}]}}]}"#,
            ),
            (
                ProviderKind::OllamaNative,
                r#"{"done":true,"message":{"role":"assistant","content":"one"}}"#,
            ),
            (
                ProviderKind::CustomCompatible,
                r#"{"choices":[{"finish_reason":"stop","message":{"content":"one"}}]}"#,
            ),
        ];
        for (kind, data) in cases {
            assert_eq!(parse_text(kind, data.as_bytes()).unwrap(), "one");
            assert!(parse_text(kind, b"{").is_err());
            assert!(parse_text(kind, b"{}").is_err());
        }
    }

    #[test]
    fn rejects_unsafe_endpoints_bad_models_and_oversized_history() {
        let mut p = profile(ProviderKind::OpenaiChat);
        for bad in [
            "http://example.com/v1",
            "https://alice:secret@example.com/v1",
            "https://example.com/v1?access_key=secret",
            "http://192.168.1.4/v1",
            "https://example.com/v1/%2e%2e",
            "https://example.com/v1/../v2",
            "https://example.com/v1/chat/completions",
        ] {
            p.base_url = bad.to_owned();
            assert!(prepare(&p, &history()).is_err(), "{bad}");
        }
        p = profile(ProviderKind::GoogleGemini);
        p.model_id = "../unexpected".into();
        assert!(prepare(&p, &history()).is_err());
        let long = vec![ChatEntry {
            role: ChatRole::User,
            content: "x".repeat(MAX_INPUT_BYTES + 1),
        }];
        assert_eq!(
            prepare(&profile(ProviderKind::OllamaNative), &long).unwrap_err(),
            AdapterError::TooLarge
        );
        assert_eq!(
            parse_text(
                ProviderKind::OpenaiChat,
                &vec![b'x'; MAX_RESPONSE_BYTES + 1]
            )
            .unwrap_err(),
            AdapterError::TooLarge
        );
    }
}
