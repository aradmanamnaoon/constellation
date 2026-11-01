# Constellation security policy

## Current status

Constellation is an **experimental, unreleased** local-first application.
Security and interoperability are actively under review. A passing unit test,
an isolated desktop screenshot or an unsigned package is not a production
security certification. See [`docs/mission/status.md`](docs/mission/status.md)
and [`docs/security/initial-threat-model.md`](docs/security/initial-threat-model.md)
for current findings and open release gates.

## Reporting vulnerabilities

Do not publish exploit details, live tokens, project files, database copies or
other users' sensitive data in public issues. When an official GitHub repository
and its private vulnerability-reporting mechanism are available, use **Security
→ Report a vulnerability** on that repository. Until that channel is verified,
coordinate a private reporting channel with the maintainers before sending
sensitive material. No vulnerability disclosure address or response service has
yet been verified for this unreleased project.

Describe the affected OS and version, minimal reproducible trigger, expected
vs observed impact, and whether any real secrets or network access were
involved. Prefer synthetic fixtures; redact paths/usernames/credentials.

## Sensitive operations and trust boundaries

The local Rust executor requires explicit scoped approvals and one-use native
consent for the currently exposed manual read/create file actions. It uses a
bounded file size and stores authenticated audit records. SHA-bound reads reject
post-review changes. This is **not** an OS-level filesystem or network sandbox.
Model responses, skill/MCP descriptions and retrieved documents are untrusted.
Do not run unknown provider/model output as a shell command, install arbitrary
third-party connectors or enable unaudited network capabilities.

Cloud sync and a remote runner are opt-in, incomplete features with protocol
fixtures but without the live multi-device/runner release acceptance. Avoid
putting production secrets or irreplaceable research data into experimental
builds. Maintain independent backups and verify restores with test data.

## Dependencies and releases

The project retains pinned dependencies where supported and local CycloneDX
SBOM/audit outputs under `docs/security/`. Reports reflect named source
snapshots; they are not current signed release attestations. No stable
public version support window, artifact-signing key, coordinated disclosure
timeline or guarantee of fixes is yet established.
