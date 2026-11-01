# Constellation provider adapters

An **offline, text-only request and response translation crate** for six real
provider API families. It does not perform HTTP, access credentials, download
models, execute model tools, or control the local machine.

## Canonical native API

This crate uses the authoritative core types ProviderProfile, ProviderKind,
ChatEntry and ChatRole, rather than a conflicting second schema.

| Serialized ProviderKind | Wire route suffix | Native auth strategy |
| --- | --- | --- |
| openai_chat | /chat/completions | Bearer |
| openai_responses | /responses | Bearer |
| anthropic_messages | /messages | AnthropicKey (x-api-key) |
| google_gemini | /models/{model}:generateContent | GeminiKey (x-goog-api-key) |
| ollama_native | /api/chat | OptionalBearer |
| custom_compatible | /chat/completions | Bearer |

Public functions:

- prepare(&ProviderProfile, &[ChatEntry]) -> Result<PreparedChat, AdapterError>
- parse_text(ProviderKind, &[u8]) -> Result<String, AdapterError>
- parse_response(ProviderKind, u16, &[u8]) -> Result<String, AdapterError>
- prepare_stream(&ProviderProfile, &[ChatEntry]) -> Result<PreparedChat, AdapterError>
- StreamDecoder::new(ProviderKind), push(&[u8]), finish()

PreparedChat provides a validated endpoint URL, JSON body, auth strategy and
protocol headers (static names/values). No API key value enters this type.
Its Debug implementation redacts prompt text and endpoint paths. Credential
resolution must happen at dispatch in the native HTTP transport.

The adapters support a system prefix and user/assistant history, including
group-room history ending with another assistant's reply. For models that
reject assistant prefill, the caller must create an unambiguous new user turn;
the crate never claims the provider accepted such a history.

The OpenAI Responses request separates system instructions, sets store=false,
and omits tools. Gemini uses systemInstruction, contents, and model-role
assistant messages. Ollama uses stream=false and num_predict. Anthropic uses
the supported legacy x-api-key header through the native auth strategy and
anthropic-version 2026-10-21; current Anthropic also supports Authorization
Bearer, which requires a separately audited native integration change.
Workspace-specific Anthropic credentials may need an additional workspace ID.

## Security and integration boundaries

The adapters reject non-HTTPS remote provider endpoints, private IP literals
outside loopback, credentials in URLs, query/fragment components, encoded
escapes and path traversal. These URL checks **cannot** defend against DNS
rebinding, malicious proxy configuration, network redirects, or a remote
server resolving a different IP. The native transport MUST resolve and pin
allowed addresses, reject private/metadata DNS responses, disable inherited
proxies and HTTP redirects, use TLS for remote providers, and enforce the
original approved scheme/host/path. Do not automatically retry potentially
billable model requests after ambiguous network failures.

Responses are bounded to 1 MiB JSON and 128 KiB decoded text; request history
is capped at 64 messages and 64 KiB text. The parser fails on known truncation,
blocked or refused output, tool calls and nontext results. Provider-supplied
error messages are never reflected in AdapterError.

The opt-in `prepare_stream` and incremental `StreamDecoder` implement strict
bounded text-only streaming for OpenAI Chat/compatible SSE, OpenAI Responses
SSE, Anthropic Messages SSE and Ollama NDJSON. A terminal stop/completion event
is required before `finish()` can return text. HTTP chunks may split UTF-8
code points or SSE lines. The parser refuses errors, incomplete generations,
tool-use events, refusals, malformed frames and responses over 1 MiB. It
restricts returned text to 128 KiB. Anthropic thinking deltas are discarded.
Partial text deltas returned by `push` are provisional: a later server error
invalidates the entire reply. The trusted native host must never persist
partial output as a successful model response. The decoder now latches
every failed `push`; subsequent calls and `finish` return the same error.
OpenAI Chat refuses non-usage choice events after `finish_reason: stop`,
Anthropic refuses additional content after its stop reason, and all protocols
reject bytes after a terminal event even within the same transport chunk.
These checks reject delayed or duplicate text without silently resuming a
failed stream. Gemini streaming needs a
separately validated endpoint (`streamGenerateContent`) and currently fails
closed. The native Tauri host now exposes an **explicit opt-in** stream reply
command for Rooms, reusing its existing pinned-endpoint transport policy and
passing provisional deltas over a per-invocation private Tauri channel. A
reply is saved only after the decoder reports safe terminal completion; the
user can choose the established nonstreaming reply path instead. Five native
loopback HTTP and cancellation tests cover all five protocol variants,
rejected late errors and pre-dispatch cancellation. Actual third-party stream
compatibility, installed GUI interaction, mid-response HTTP cancellation,
retries, model failover and sustained-job durability remain outstanding P13 work.

This crate does not claim compatibility with every vendor, paid credentials,
tool execution, multimodal requests, arbitrary LAN HTTP servers, or live
provider behavior. Its tests are deterministic and offline.

## Verification

From this crate directory:

    cargo fmt --all --check
    cargo test --offline
    cargo clippy --all-targets --offline -- -D warnings

Source protocol references checked October 30, 2026:

- Anthropic: https://platform.claude.com/docs/en/api/typescript/messages/create
- Anthropic authentication: https://platform.claude.com/docs/en/api/overview
- Gemini: https://ai.google.dev/api/generate-content
- OpenAI Responses: https://developers.openai.com/api/reference/typescript/resources/beta/subresources/responses/methods/create
- Ollama chat: https://github.com/ollama/ollama/blob/main/docs/api.md
