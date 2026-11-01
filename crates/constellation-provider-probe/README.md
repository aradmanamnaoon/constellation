# Constellation provider compatibility probe

Explicit opt-in Rust library for one minimal OpenAI-compatible
chat completion request. The native desktop command can invoke this library
when the user selects a saved provider and presses Test text chat.
Creating or validating a configuration is offline. Only awaiting
`probe(...)` makes a probe HTTP request.

## Usage

```rust
use constellation_provider_probe::{prepare, probe, ProbeRequest, ProbeStatus};

let request = ProbeRequest {
    base_url: "http://127.0.0.1:11434/v1",
    model_id: "already-configured-model",
    api_key: None,
};
// Pure validation; no DNS lookup or HTTP requests.
let _prepared = prepare(&request)?;
// Only invoke after explicit user confirmation:
let result = probe(request).await;
if result.status == ProbeStatus::Compatible {
    println!("One OpenAI-style text chat response was observed");
}
```

The snippet runs inside an existing async runtime. No provider enumeration,
automatic startup probes, model downloads, ambient credential lookup or app
retry policy is performed. A caller must provide the exact configured base
URL, model identifier, and optional session-only Bearer token.

## Shared transport for native chat

The same endpoint validator and DNS-pinned resolver now serve both the
compatibility probe and ordinary model-request callers:

```rust
use constellation_provider_probe::{build_safe_client, completion_endpoint};

// Only invoke after the user explicitly selects and authorizes this provider.
let base_url = "https://provider.example/v1";
let client = build_safe_client(base_url).await?;
let endpoint = completion_endpoint(base_url)?;
// Send any model request ONLY to this validated endpoint.
// The caller owns request-body bounds, response limits and credential policy.
let request = client.post(endpoint);
```

`build_safe_client(base_url: &str) -> Result<reqwest::Client, ProbeStatus>`
may perform one DNS lookup but never sends an HTTP request. The factory and
`probe()` call the same validation and resolver, including strict public-IP
checks, DNS pinning, no ambient proxy and no redirects.
`completion_endpoint(base_url: &str) -> Result<url::Url, ProbeStatus>`
uses the identical parser to prevent route drift between probe and runtime.
**The returned reqwest Client is generic and is not itself a sandbox.** A
caller can misuse it by sending to a different URL; real native call sites
must use the validated endpoint and never accept an independent URL later.

The factory uses a 2-second DNS deadline and 7-second overall creation
deadline, and the client uses 2-second connect and 5-second request timeouts.
The 16 KiB output limit applies to the probe's response parser, so production
model callers must retain their separate output limits and user cost budgets.

## Single request and trust boundaries

One POST is made to the configured base prefix plus `/chat/completions`,
such as `https://host.example/v1/chat/completions`. Its JSON payload is:

```json
{
  "model": "<configured model>",
  "messages": [{"role": "user", "content": "Reply OK."}],
  "max_tokens": 1,
  "stream": false
}
```

Remote calls can incur token charges and disclose this fixed prompt and model
identifier to the chosen provider. Bearer credentials are sent only to the
specified origin over HTTPS or exact loopback HTTP. Credentials, URLs, response
content, and transport errors are omitted from returned outcomes and Debug.
Only a status enum and optional numeric HTTP status are returned.

| Outcome | What was actually observed |
| --- | --- |
| `Compatible` | HTTP 2xx, valid JSON and a nonempty OpenAI-style chat message content string |
| `InvalidConfiguration` | Invalid URL/model/key, private network target or forbidden scheme; no request sent |
| `Unreachable` | DNS, TLS, connect, timeout or truncated-response transport failure |
| `Unauthorized` | HTTP 401/403 |
| `BadJson` | HTTP success with invalid or empty JSON |
| `Unsupported` | Redirect, unsupported response/status, oversized body, or missing expected chat shape |
| `RateLimited` | HTTP 429 |
| `ProviderError` | HTTP 5xx |

`Compatible` verifies only this small, non-streaming text exchange.
It does not prove tool calling, provider privacy, streaming, multimodal
features, long contexts, reliable reasoning or arbitrary API compatibility.
With only one requested completion token, some real providers can return an
empty response and be classified as unsupported.

## Network restrictions

- HTTP requires exact `localhost`, IPv4 loopback, or IPv6 loopback.
  Otherwise use HTTPS with normal TLS certificate verification.
- Remote literal IPs are filtered against special/private/reserved ranges.
  Remote DNS results are inspected together; any non-public answer rejects
  the request. Accepted addresses are pinned to the HTTP client so DNS
  cannot rebind between validation and connection.
- No HTTP redirects are followed, and no ambient OS proxy is used. URL
  userinfo, query, fragment, ambiguous encoded/dot paths, zero ports, and
  embedded ASCII control characters are rejected. Non-canonical IPv4 host
  spellings such as `2130706433`, `0x7f000001`, and `127.1` are explicitly
  denied before URL normalization can disguise their true destination.
- Timeouts: DNS 2s, connect 2s, HTTP 5s, full operation 7s. The probe makes
  only one application-level request. Validated response size: 16 KiB max.
- The public-IP predicate is conservative; newly allocated special-purpose
  ranges require future review. DNS lookup itself uses the host resolver.

The user-facing application should request explicit consent before probing a
remote host and avoid sending user documents or prior conversation turns.

## Local verification

Run from the Constellation project root:

```powershell
cargo fmt --manifest-path crates/constellation-provider-probe/Cargo.toml --all --check
cargo test --manifest-path crates/constellation-provider-probe/Cargo.toml --offline --locked
cargo clippy --manifest-path crates/constellation-provider-probe/Cargo.toml --all-targets --offline --locked -- -D warnings
```

The independent `Cargo.lock` pins dependencies. All integration
fixtures use an ephemeral localhost server; no external provider is contacted.
Tests cover response classification, request shape, denied redirects,
private-network rejection, mixed public/private DNS policy, shared factory
endpoint validation, explicit factory non-dispatch, limits, timeouts and
credential redaction. The proxy test places fake `HTTP_PROXY` / `HTTPS_PROXY`
and `ALL_PROXY` values within its own single-test integration binary
using two ephemeral localhost listeners (origin and unwanted proxy); it
restores the original environment and verifies the unwanted proxy receives
no request. Address-policy tests feed synthetic mixed public/private DNS
answer sets in multiple orders, without relying on external DNS.

The complete suite currently contains **20 Rust tests** (4 unit, 15 localhost
fixtures and one proxy regression); all are offline and use fake credentials.
Production DNS/TLS behavior remains to be verified separately with a
controlled, authorized host; no live remote-provider claim is made. Tests do
not demonstrate resistance to every OS DNS resolver, network-layer redirect
or caller who misuses the returned generic reqwest client with another URL.
