# Providers and privacy boundary

Constellation is **local-first, not necessarily local inference**. The AI model
can live on a user-managed localhost server or a third-party HTTPS endpoint,
while any eventual tools must run through a separately approved, user-owned
execution engine. The model endpoint does not grant access to OS tools.

## Supported protocols in the current Rust implementation

The runtime translates messages into distinct API wire formats, rather than
assuming every service speaks OpenAI Chat Completions. All six offer bounded
nonstreaming text requests; five additionally offer experimental streaming:

| Choose in Settings | Request contract | Key policy |
| --- | --- | --- |
| OpenAI Chat | `POST <base>/chat/completions` | Bearer token |
| OpenAI Responses | `POST <base>/responses` | Bearer token |
| Anthropic Messages | `POST <base>/messages` with `anthropic-version` header | Anthropic API key |
| Google Gemini | `POST <base>/models/{model}:generateContent` | `x-goog-api-key` |
| Native Ollama | `POST <base>/api/chat` with `stream: false` | Key not required for localhost |
| Custom compatible | OpenAI-style `/chat/completions` for LM Studio, vLLM, llama.cpp and compatible gateways | Provider-specific, usually optional locally |

All five distinct protocol families and custom chat behavior passed offline
native Rust dispatch tests using **deterministic localhost HTTP fixtures**.
This is not proof of account/model availability or real provider correctness.
Rooms includes an opt-in **Show a live text preview** checkbox. OpenAI Chat,
OpenAI Responses, Anthropic Messages, native Ollama and custom-compatible
endpoints use a bounded SSE or NDJSON parser through a private native channel;
Gemini continues using the completed-response path. Previews are provisional:
a refusal, truncation, incomplete disconnect or malformed terminal response
invalidates the request and cannot become a saved assistant message. The
native host does not persist a response until the complete protocol frame
has been verified. Real localhost HTTP fixture tests cover five streaming
formats, final-status handling, and rejection of bad content types. Rooms also
offers **Stop live response** after native stream registration; cancellation
drops the in-flight model future and prevents dispatch of remaining
individually routed agents. A pre-dispatch cancellation passed a native
database regression. A remote model's compatibility with this streaming
protocol is **unverified**.
Relevant official API documentation:
[Anthropic](https://platform.claude.com/docs/en/api/overview),
[Gemini](https://ai.google.dev/api/generate-content),
[Ollama](https://docs.ollama.com/api/chat),
[OpenAI](https://platform.openai.com/docs/api-reference/responses).

- HTTPS base URLs or HTTP on literal loopback/localhost only;
- actual provider-specific request body, header, endpoint and response parsing;
- default session-only API keys never stored in SQLite;
- optional explicit opt-in to Windows Credential Manager or macOS Keychain;
  the SQLite profile holds only a fixed-namespace, provider-ID-bound reference.
  Other platforms fail closed instead of storing secrets in plaintext;
- no following redirects, pinned DNS (for remote hosts), proxies disabled,
  bounded timeouts and 1 MiB response limit;
- explicit provider selection before any room text goes to a model.

Examples of *potentially compatible* URLs: `http://127.0.0.1:11434/v1`
for a locally running API-compatible service, or your provider's documented
HTTPS `/v1` endpoint. These are configuration shapes, **not verified live
provider integrations**. A model's advertised capability, support for tools,
availability and pricing must be verified separately.

**“Any AI” requires protocol compatibility, not brand whitelisting.** Any
model served through an implemented protocol can be configured with its exact
model ID and endpoint; a truly new, undocumented or proprietary API format
needs a vetted additional protocol adapter. There is no truthful way to
guarantee universal support without knowing a provider's transport, auth,
request and response semantics. No automatic provider fallback is allowed.

## Current limitations

The Windows/macOS OS vault option requires an explicit checkbox and API key
entry; a throwaway Windows OS vault create/load/delete test has passed, but
fresh installed GUI persistence/restart/revocation testing remains open.
There is no automatic model discovery, tool-capability negotiation, verified live-provider streaming, monetary rate/cost accounting,
multi-modal input, production-provider certification, backoff budget UI,
or credential revocation screen. Do not enter production or patient secrets in
this experimental app. Saved provider **profiles** live in SQLite; secret
values do not. Other software running as the same OS user may have access to
credential-manager entries. A profile can also be reconfigured with a fresh
session key.

No provider content should be treated as verified research or executed tool
actions. The deterministic localhost fixture used in tests is not an AI model.
