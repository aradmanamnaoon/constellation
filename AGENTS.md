# Constellation engineering instructions

Source of truth: the user's 30 October 2026, 1,237-line Codex Master Mission
for a local-first multi-agent collaboration platform. The full original is
attached to the initiating ChatGPT conversation. This repository is the
implementation workspace, not a replacement for the specification.

## Product invariants

1. Rust owns domain state, permissions, agent execution, scheduling, provider
   routing, persistence, sync/cryptography and any device communication.
   Svelte is a presentation layer; never execute a tool directly in the UI.
2. Work executes on the user's machine by default; optional remote execution
   must use an explicitly paired and authorized user-controlled runner.
3. Models may be external or local, independently of execution location.
4. An agent is not allowed to gain OS/tool privileges because a model asked.
   Default deny; narrowly scoped approvals and honest logs.
5. Never expose/sync credentials by default. Do not print secrets in logs or
   commit local data, keys, caches, downloads, installers or private content.
6. The interface must be serene, responsive, understandable without AI jargon,
   accessible by keyboard and screen reader, and tested as a *real GUI*.
7. 200 real role definitions and 200 vetted, installed/exercised skills/MCP
   development integrations are separate requested deliverables. Never pad or
   fabricate either count.
8. Public source and releases require actual verified CI/build/platform tests,
   security/license checks and correct checksums; no public release from a
   mock-only success. Preserve repo/history without force-push or destructive
   cleaning.

## Working boundaries

- This project lives at `D:\coding\codex\constellation` on the connected PC.
- Do not change sibling repositories (especially `researchweave`).
- `crates/constellation-core` is authoritative for durable state and policy.
- `apps/desktop/src-tauri` is the only webview/native IPC bridge.
- `apps/desktop/src` contains Svelte UI and styling only.
- `docs/quality/acceptance-ledger.md` records unverified acceptance rather
  than marketing claims.

## Immediate verification

```text
cargo test --manifest-path crates/constellation-core/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-core/Cargo.toml --all-targets --offline -- -D warnings
cd apps/desktop
pnpm install --frozen-lockfile
pnpm run check
pnpm run build
cd src-tauri
cargo check
```

Keep code changes reviewable, add negative/security tests for permissions,
cancellation and replay, use actual installed GUI sessions before marking
UX gates complete, and identify unimplemented capabilities plainly.
