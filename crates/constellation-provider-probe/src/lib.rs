//! Explicit, bounded, OpenAI-style chat endpoint probe.
//! Preparation is pure. Only an explicit awaited probe performs network I/O.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::header::{ACCEPT, ACCEPT_ENCODING, CONTENT_TYPE};
use serde_json::{json, Value};
use tokio::net::lookup_host;
use tokio::time::timeout;
use url::{Host, Url};

const MAX_URL_BYTES: usize = 2048;
const MAX_MODEL_BYTES: usize = 256;
const MAX_KEY_BYTES: usize = 8192;
const MAX_REPLY_BYTES: usize = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(2);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(7);

/// Caller-supplied values only. Do not derive Debug for secrets.
pub struct ProbeRequest<'a> {
    pub base_url: &'a str,
    pub model_id: &'a str,
    pub api_key: Option<&'a str>,
}

impl fmt::Debug for ProbeRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProbeRequest")
            .field("base_url", &"[redacted]")
            .field("model_id", &"[redacted]")
            .field("api_key", &"[redacted]")
            .finish()
    }
}

/// Results never include remote response text, URLs or credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeStatus {
    Compatible,
    InvalidConfiguration,
    Unreachable,
    Unauthorized,
    BadJson,
    Unsupported,
    RateLimited,
    ProviderError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeResult {
    pub status: ProbeStatus,
    /// HTTP status if received; no response body or headers are exposed.
    pub http_status: Option<u16>,
}

impl ProbeResult {
    fn new(status: ProbeStatus, http_status: Option<u16>) -> Self {
        Self {
            status,
            http_status,
        }
    }
}

#[derive(Clone)]
pub struct PreparedProbe {
    endpoint: Url,
    model_id: String,
    host: TargetHost,
}

impl fmt::Debug for PreparedProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedProbe")
            .field("endpoint", &"[redacted]")
            .field("model_id", &"[redacted]")
            .field("host", &"[redacted]")
            .finish()
    }
}

#[derive(Clone)]
enum TargetHost {
    LoopbackName,
    Literal(IpAddr),
    PublicDomain(String),
}

/// Validates exact configured base URL without DNS or network effects.
pub fn prepare(request: &ProbeRequest<'_>) -> Result<PreparedProbe, ProbeStatus> {
    if request.model_id.is_empty()
        || request.model_id.len() > MAX_MODEL_BYTES
        || request.model_id.trim() != request.model_id
        || request.model_id.chars().any(char::is_control)
    {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    if request.api_key.is_some_and(|key| {
        key.len() > MAX_KEY_BYTES
            || key.is_empty()
            || key.bytes().any(|byte| !byte.is_ascii_graphic())
    }) {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    let (endpoint, host) = validated_completion_target(request.base_url)?;
    Ok(PreparedProbe {
        endpoint,
        model_id: request.model_id.to_owned(),
        host,
    })
}

/// Validated canonical OpenAI-style chat completion URL for this exact base.
/// This function is pure. Callers of the shared client factory MUST dispatch
/// only to this returned URL: a generic reqwest Client is not a URL sandbox.
pub fn completion_endpoint(base_url: &str) -> Result<Url, ProbeStatus> {
    validated_completion_target(base_url).map(|(url, _)| url)
}

fn validated_completion_target(base_url: &str) -> Result<(Url, TargetHost), ProbeStatus> {
    if base_url.len() > MAX_URL_BYTES
        || base_url.trim() != base_url
        || base_url.chars().any(char::is_control)
    {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    // Validate raw input before Url parsing normalizes dot segments and
    // backslashes that would silently change the configured path.
    if base_url.contains('\\') {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    let raw_after_scheme = base_url
        .split_once("://")
        .ok_or(ProbeStatus::InvalidConfiguration)?
        .1;
    let raw_authority = raw_after_scheme.split('/').next().unwrap_or_default();
    // WHATWG URL parsing accepts octal, hex and integer spellings for IPv4
    // (e.g. 2130706433 or 127.1 -> 127.0.0.1). Never let obfuscated hosts
    // acquire the special localhost allowance, even though URL normalizes
    // them to the same destination.
    if raw_authority.contains('%') {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    let raw_path = raw_after_scheme
        .find('/')
        .map_or("", |start| &raw_after_scheme[start..]);
    if raw_path
        .split('/')
        .any(|piece| piece == "." || piece == ".." || piece.contains('%'))
    {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    let mut url = Url::parse(base_url).map_err(|_| ProbeStatus::InvalidConfiguration)?;
    if let Some(Host::Ipv4(parsed)) = url.host() {
        let raw_ipv4 = raw_authority.split(':').next().unwrap_or_default();
        if raw_ipv4.parse::<Ipv4Addr>() != Ok(parsed) {
            return Err(ProbeStatus::InvalidConfiguration);
        }
    }
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() == Some(0)
        || !matches!(url.scheme(), "http" | "https")
    {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    let host = match url.host() {
        Some(Host::Domain("localhost")) => TargetHost::LoopbackName,
        Some(Host::Domain(name)) => {
            if name.is_empty() || name.ends_with(".local") || name.ends_with(".internal") {
                return Err(ProbeStatus::InvalidConfiguration);
            }
            TargetHost::PublicDomain(name.to_owned())
        }
        Some(Host::Ipv4(ip)) => {
            if !ip.is_loopback() && !is_public_ipv4(ip) {
                return Err(ProbeStatus::InvalidConfiguration);
            }
            TargetHost::Literal(IpAddr::V4(ip))
        }
        Some(Host::Ipv6(ip)) => {
            if !ip.is_loopback() && !is_public_ipv6(ip) {
                return Err(ProbeStatus::InvalidConfiguration);
            }
            TargetHost::Literal(IpAddr::V6(ip))
        }
        None => return Err(ProbeStatus::InvalidConfiguration),
    };
    let is_loopback = matches!(host, TargetHost::LoopbackName)
        || matches!(host, TargetHost::Literal(ip) if ip.is_loopback());
    if url.scheme() == "http" && !is_loopback {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    // Reject ambiguous prefixes. URL parsing has already canonicalized dots
    // and encoded separators, so inspect the original too (see later tests).
    let path = url.path();
    if path.contains('%')
        || path.contains('\\')
        || path.contains("//")
        || path.split('/').any(|piece| piece == "." || piece == "..")
        || path.ends_with("/chat/completions")
    {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    url.set_path(&format!("{}/chat/completions", path.trim_end_matches('/')));
    Ok((url, host))
}

/// Makes exactly one small completion request; no implicit background probes.
///
/// A successful probe means only that the configured endpoint returned an
/// OpenAI-style text response for this supplied model. Remote probes may
/// incur inference cost. This is not a tool, streaming or safety guarantee.
pub async fn probe(request: ProbeRequest<'_>) -> ProbeResult {
    let prepared = match prepare(&request) {
        Ok(value) => value,
        Err(status) => return ProbeResult::new(status, None),
    };
    match timeout(TOTAL_TIMEOUT, perform_probe(&prepared, request.api_key)).await {
        Ok(outcome) => outcome,
        Err(_) => ProbeResult::new(ProbeStatus::Unreachable, None),
    }
}

/// Build a bounded HTTP client for one explicitly configured endpoint.
///
/// Resolves and pins public remote DNS addresses; forbids all ambient proxies
/// and redirects. This operation may perform DNS I/O, so call it only after an
/// explicit user action. It sends NO HTTP requests by itself.
///
/// IMPORTANT: reqwest Client is generic: clients MUST send to
/// completion_endpoint(base_url) ONLY, not to caller-supplied arbitrary URLs.
/// This factory controls transport for that host, not all possible Client URLs.
pub async fn build_safe_client(base_url: &str) -> Result<reqwest::Client, ProbeStatus> {
    let (endpoint, host) = validated_completion_target(base_url)?;
    match timeout(TOTAL_TIMEOUT, build_client_for_target(&endpoint, &host)).await {
        Ok(result) => result,
        Err(_) => Err(ProbeStatus::Unreachable),
    }
}

fn checked_remote_addresses(addrs: &[SocketAddr]) -> Result<(), ProbeStatus> {
    if addrs.is_empty() {
        return Err(ProbeStatus::Unreachable);
    }
    if addrs.iter().any(|addr| !is_public_ip(addr.ip())) {
        return Err(ProbeStatus::InvalidConfiguration);
    }
    Ok(())
}

/// Shared network policy for both one-shot probes and real model requests.
async fn build_client_for_target(
    endpoint: &Url,
    host: &TargetHost,
) -> Result<reqwest::Client, ProbeStatus> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(0);
    let port = endpoint.port_or_known_default().unwrap_or(443);
    match host {
        TargetHost::LoopbackName => {
            builder = builder.resolve_to_addrs(
                "localhost",
                &[
                    SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                    SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
                ],
            );
        }
        TargetHost::PublicDomain(host) => {
            let resolved = match timeout(RESOLVE_TIMEOUT, lookup_host((host.as_str(), port))).await
            {
                Ok(Ok(addrs)) => addrs.collect::<Vec<_>>(),
                _ => return Err(ProbeStatus::Unreachable),
            };
            checked_remote_addresses(&resolved)?;
            builder = builder.resolve_to_addrs(host, &resolved);
        }
        TargetHost::Literal(_) => {} // reqwest connects to this exact parsed IP.
    }
    builder
        .build()
        .map_err(|_| ProbeStatus::InvalidConfiguration)
}

async fn perform_probe(prepared: &PreparedProbe, key: Option<&str>) -> ProbeResult {
    let client = match build_client_for_target(&prepared.endpoint, &prepared.host).await {
        Ok(client) => client,
        Err(status) => return ProbeResult::new(status, None),
    };
    let body = json!({
        "model": prepared.model_id,
        "messages": [{"role": "user", "content": "Reply OK."}],
        "max_tokens": 1,
        "stream": false
    });
    let mut request = client
        .post(prepared.endpoint.clone())
        .header(ACCEPT, "application/json")
        .header(ACCEPT_ENCODING, "identity")
        .header(CONTENT_TYPE, "application/json")
        .json(&body);
    if let Some(secret) = key {
        request = request.bearer_auth(secret);
    }
    let mut response = match request.send().await {
        Ok(response) => response,
        Err(_) => return ProbeResult::new(ProbeStatus::Unreachable, None),
    };
    let code = response.status().as_u16();
    let status = match code {
        401 | 403 => ProbeStatus::Unauthorized,
        429 => ProbeStatus::RateLimited,
        500..=599 => ProbeStatus::ProviderError,
        200..=299 => {
            if response
                .content_length()
                .is_some_and(|length| length > MAX_REPLY_BYTES as u64)
            {
                return ProbeResult::new(ProbeStatus::Unsupported, Some(code));
            }
            let mut bytes = Vec::new();
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        if chunk.len() > MAX_REPLY_BYTES.saturating_sub(bytes.len()) {
                            return ProbeResult::new(ProbeStatus::Unsupported, Some(code));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    Ok(None) => break,
                    Err(_) => return ProbeResult::new(ProbeStatus::Unreachable, Some(code)),
                }
            }
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => {
                    if value
                        .pointer("/choices/0/message/content")
                        .and_then(Value::as_str)
                        .is_some_and(|content| !content.trim().is_empty())
                    {
                        ProbeStatus::Compatible
                    } else {
                        ProbeStatus::Unsupported
                    }
                }
                Err(_) => ProbeStatus::BadJson,
            }
        }
        _ => ProbeStatus::Unsupported, // Includes 3xx: no Location followed.
    };
    ProbeResult::new(status, Some(code))
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    // Conservative special-use IPv4 exclusions; new IANA ranges need review.
    !(a == 0
        || a == 10
        || a == 127
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    // Global unicast only, excluding special addresses, docs and tunneling.
    (segments[0] & 0xe000) == 0x2000
        && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
        && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        && segments[0] != 0x2002
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn public_address_filter_rejects_private_shared_reserved_and_tunneled_ips() {
        for candidate in [
            "127.0.0.1",
            "10.1.1.1",
            "172.31.1.2",
            "192.168.0.8",
            "169.254.169.254",
            "100.64.1.1",
            "0.1.2.3",
            "192.0.2.1",
            "198.18.3.4",
            "198.51.100.4",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:c0a8:0101::1",
        ] {
            let parsed = candidate.parse::<IpAddr>().unwrap();
            assert!(!is_public_ip(parsed), "should reject {candidate}");
        }
        for candidate in ["1.1.1.1", "8.8.8.8", "2001:4860:4860::8888"] {
            assert!(is_public_ip(candidate.parse().unwrap()), "{candidate}");
        }
    }

    #[test]
    fn debug_never_prints_credentials_or_endpoint() {
        let request = ProbeRequest {
            base_url: "https://secret-host.invalid/v1",
            model_id: "secret-model",
            api_key: Some("ultra-private-secret"),
        };
        let rendered = format!("{request:?}");
        for secret in ["secret-host", "secret-model", "ultra-private-secret"] {
            assert!(!rendered.contains(secret));
        }
    }

    #[test]
    fn dns_rebinding_policy_rejects_mixed_private_and_public_answers() {
        let public_v4 = SocketAddr::from(([8, 8, 8, 8], 443));
        let private_v4 = SocketAddr::from(([10, 2, 3, 4], 443));
        let loopback_v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, 443));
        let public_v6 =
            SocketAddr::from(("2001:4860:4860::8888".parse::<Ipv6Addr>().unwrap(), 443));
        assert_eq!(checked_remote_addresses(&[]), Err(ProbeStatus::Unreachable));
        assert_eq!(checked_remote_addresses(&[public_v4, public_v6]), Ok(()));
        assert_eq!(
            checked_remote_addresses(&[public_v4, private_v4]),
            Err(ProbeStatus::InvalidConfiguration)
        );
        assert_eq!(
            checked_remote_addresses(&[public_v6, loopback_v6]),
            Err(ProbeStatus::InvalidConfiguration)
        );
    }

    #[test]
    fn dns_policy_rejects_every_non_public_answer_independent_of_order_and_port() {
        let public_v4 = SocketAddr::from(([9, 9, 9, 9], 443));
        let public_v6 =
            SocketAddr::from(("2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap(), 8443));
        let forbidden = [
            "0.0.0.0",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.2",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.254",
            "192.0.0.9",
            "192.0.2.77",
            "192.88.99.1",
            "192.168.255.254",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.77",
            "203.0.113.77",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:10.0.0.1",
            "fc00::1",
            "fd12:3456::2",
            "fe80::abcd",
            "ff02::1",
            "2001:db8::42",
            "2002:0a00:0001::1",
        ];
        for ip in forbidden {
            let address = SocketAddr::new(ip.parse().unwrap(), 8123);
            for answers in [
                vec![address],
                vec![public_v4, address],
                vec![address, public_v4],
                vec![public_v6, address, public_v4],
            ] {
                assert_eq!(
                    checked_remote_addresses(&answers),
                    Err(ProbeStatus::InvalidConfiguration),
                    "mixed DNS result must reject {ip} regardless of order"
                );
            }
        }
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "172.15.255.254",
            "172.32.0.1",
            "198.20.0.1",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
        ] {
            assert_eq!(
                checked_remote_addresses(&[SocketAddr::new(public.parse().unwrap(), 443)]),
                Ok(()),
                "public address should pass the conservative policy: {public}"
            );
        }
        // Resolving no addresses must not silently fall back to system DNS.
        assert_eq!(checked_remote_addresses(&[]), Err(ProbeStatus::Unreachable));
    }
}
