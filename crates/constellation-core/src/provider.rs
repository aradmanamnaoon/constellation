use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::{Host, Url};

use crate::{CoreError, ProviderId, Result};

/// Wire protocol, independent of provider brand or where inference runs.
/// All old saved profiles deserialize as OpenAI-compatible chat, preserving
/// existing user data and preventing implicit migration to a new endpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    OpenaiChat,
    OpenaiResponses,
    AnthropicMessages,
    GoogleGemini,
    OllamaNative,
    CustomCompatible,
}

/// Only references to credentials may be persisted. No API key value is a
/// property of a provider profile or adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialSource {
    None,
    Environment { variable: String },
    OsKeyring { service: String, account: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfile {
    pub id: ProviderId,
    pub display_name: String,
    pub model_id: String,
    /// Base endpoint including optional API version prefix (e.g. https://host/v1).
    pub base_url: String,
    #[serde(default)]
    pub kind: ProviderKind,
    pub credentials: CredentialSource,
}

impl ProviderProfile {
    pub fn validate(&self) -> Result<()> {
        require_text(&self.display_name, "provider name")?;
        require_text(&self.model_id, "model ID")?;
        match &self.credentials {
            CredentialSource::None => {}
            CredentialSource::Environment { variable } => {
                let mut chars = variable.chars();
                let first_ok = chars
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase() || c == '_');
                if variable.len() > 128
                    || !first_ok
                    || !chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    return Err(CoreError::InvalidInput(
                        "credential environment variable must be an identifier".into(),
                    ));
                }
            }
            CredentialSource::OsKeyring { service, account } => {
                require_text(service, "keyring service")?;
                require_text(account, "keyring account")?;
            }
        };
        endpoint_url(&self.base_url)?;
        Ok(())
    }
}

fn require_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err(CoreError::InvalidInput(format!("invalid {label}")));
    }
    Ok(())
}

fn endpoint_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw)
        .map_err(|_| CoreError::InvalidInput("invalid provider endpoint URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CoreError::InvalidInput(
            "provider endpoint must not contain credentials, query or fragment".into(),
        ));
    }
    let loopback = match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(CoreError::InvalidInput(
            "remote provider endpoints require HTTPS; HTTP is limited to localhost".into(),
        ));
    }
    Ok(url)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatEntry {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatCompletionRequest {
    pub messages: Vec<ChatEntry>,
}

/// Conservative upper bound for a single OpenAI-compatible text completion.
/// A provider may reject or ignore this parameter; this is not a cost quote.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 512;

/// An uncredentialed request description for a transport selected by the app.
/// Never log or persist runtime authorization headers with this structure.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedRequest {
    pub endpoint: Url,
    pub body: Value,
    pub credential_source: CredentialSource,
}

pub trait ProviderAdapter: Send + Sync {
    fn profile(&self) -> &ProviderProfile;
    fn prepare_chat(&self, request: &ChatCompletionRequest) -> Result<PreparedRequest>;
}

pub struct OpenAiCompatibleAdapter {
    profile: ProviderProfile,
}

impl OpenAiCompatibleAdapter {
    pub fn new(profile: ProviderProfile) -> Result<Self> {
        profile.validate()?;
        if !matches!(
            profile.kind,
            ProviderKind::OpenaiChat | ProviderKind::CustomCompatible
        ) {
            return Err(CoreError::InvalidInput(
                "provider is not an OpenAI-compatible chat protocol".into(),
            ));
        }
        Ok(Self { profile })
    }
}

impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn profile(&self) -> &ProviderProfile {
        &self.profile
    }

    fn prepare_chat(&self, request: &ChatCompletionRequest) -> Result<PreparedRequest> {
        if request.messages.is_empty() {
            return Err(CoreError::InvalidInput("no chat messages".into()));
        }
        let base = endpoint_url(&self.profile.base_url)?;
        let mut path = base.path().trim_end_matches('/').to_string();
        path.push_str("/chat/completions");
        let mut endpoint = base;
        endpoint.set_path(&path);
        Ok(PreparedRequest {
            endpoint,
            body: json!({
                "model": self.profile.model_id,
                "messages": request.messages,
                "max_tokens": DEFAULT_MAX_OUTPUT_TOKENS
            }),
            credential_source: self.profile.credentials.clone(),
        })
    }
}
