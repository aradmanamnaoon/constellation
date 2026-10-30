# Providers and privacy boundary

Constellation is **local-first, not necessarily local inference**. The AI model
can live on a user-managed localhost server or a third-party HTTPS endpoint,
while any eventual tools must run through a separately approved, user-owned
execution engine. The model endpoint does not grant access to OS tools.

## Supported contract in the current Windows build

The only implemented transport is OpenAI-compatible, non-streaming chat:

- HTTPS base URLs or HTTP on literal loopback/localhost only;
- `POST <base-url>/chat/completions` with model and ordered chat messages;
- user-provided optional Bearer API key held in RAM only;
- nonempty `choices[0].message.content` in a successful JSON response;
- no following redirects, bounded network times and 1 MiB response cap;
- explicit provider selection before any room text goes to a model.

Examples of *potentially compatible* URLs: `http://127.0.0.1:11434/v1`
for a locally running API-compatible service, or your provider's documented
HTTPS `/v1` endpoint. These are configuration shapes, **not verified live
provider integrations**. A model's advertised capability, support for tools,
availability and pricing must be verified separately.

## Current limitations

There is no secure OS keyring persistence in the desktop interface, automatic
model discovery, tool-capability negotiation, streaming, rate/cost accounting,
multi-modal input, provider-specific error conversion, backoff budget UI,
or credential revocation screen. Do not enter production or patient secrets in
this experimental app. Saved provider **profiles** live in SQLite but secret
values do not. The old profile can be reconfigured with a fresh session key.

No provider content should be treated as verified research or executed tool
actions. The deterministic localhost fixture used in tests is not an AI model.
