# Project Constellation — engineering status

**Checkpoint:** 2026-10-30. **Classification:** experimental local-first
vertical slice, **NOT the full requested mission**, NOT production secure,
NOT yet publicly published. The spec remains the original attached
`Codex_Master_Prompt_Local_Agent_Collaboration_Platform.md` (1,237 lines).

## Verified work

- Original custom Tauri2/Svelte5 desktop app with Rust domain and native IPC.
- Rust SQLite state/event persistence, task lifecycle, idempotent operations,
  exact scope default-deny permission policy as a library, session-only keys.
- OpenAI-compatible provider contract; no hidden user-account or model
  download. Remote egress only after explicit provider selection and Ask.
- Two-agent group-chat path tested by Windows GUI automation with a labelled
  localhost **mock** provider: each selected agent replied once to the same
  new user prompt, and a repeated request skipped stored replies.
- Task creation/cancellation tested through real GUI: pending and cancelled
  states explicitly said **Not Executed**; no tool runner exists.
- 200 prompt-only role specialties (42 foundational and 158 expanded), with
  substantive output expectations and deterministic validation tests;
  prompt guidance embedded in native request system instructions; NO role tool
  grants or agent task execution.
- Read-only Markdown room export: unit tests and Tauri IPC code. Direct GUI
  clipboard behavior remains separately testable.
- Separate experimental filesystem/command/Git effect library with permission
  scopes, one-shot operation approvals and receipts, *not exposed to agents*.
  Its security tests are narrower than a true OS sandbox. Approval and receipt
  state are in memory only; path-to-handle TOCTOU, child process trees and
  network egress containment remain unresolved.

## Reproducible test commands

```sh
node --test catalog/roles.test.mjs
node skills-manifest/validate.mjs --verify-local
node --test skills-manifest/validate.test.mjs
cargo test --manifest-path crates/constellation-core/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-core/Cargo.toml --all-targets --offline -- -D warnings
cargo test --manifest-path crates/constellation-tools/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-tools/Cargo.toml --all-targets --offline -- -D warnings
cd apps/desktop
pnpm run check
pnpm run build
cargo test --manifest-path src-tauri/Cargo.toml --offline --lib
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
```

Windows GUI evidence: [`docs/qa/windows-native-smoke.md`](../qa/windows-native-smoke.md).
No CI run, real-model chat, macOS, Linux or Android installed app test may be
inferred from the above commands.

## Observed environment constraint

Windows D: drive had insufficient free space for incremental Rust debug
outputs. The native target debug directory was cleaned with **scoped**
`cargo clean --profile dev` in Constellation, preserving source/release
bundles, then debug output was relocated to the C: temporary directory with
`CARGO_TARGET_DIR` and `CARGO_INCREMENTAL=0`. See root README for commands.

## Major work remaining

Permissioned task execution, production provider tests and costs, prompts
and MCP tool policy/role permission boundaries, file attachment/citation
flow, OS sandbox, encrypted backup/sync, runner, Android, cross-platform
packages and signing, security audit, 200 verified roles, 200 verified
development integrations, hosted CI evidence and actual public release.

The source checkout must **not** be described as a completed release. This
checkpoint is useful for implementation recovery, not user acceptance.
