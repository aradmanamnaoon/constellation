# Project Constellation — engineering status

**Checkpoint:** 2026-10-30. **Classification:** experimental local-first
vertical slice, **NOT the full requested mission**, NOT production secure,
NOT yet publicly published. The spec remains the original attached
`Codex_Master_Prompt_Local_Agent_Collaboration_Platform.md` (1,237 lines).

**Source build identity:** local implementation commit
`b229c5f1461d4455f1a1d41b6ff46e870e89ddd1`. Its Windows NSIS artifact
was built successfully and installed after the 200-role catalogue expansion.
See [the exact installer hash, installed executable hash, NotSigned status and
native UIA search evidence](../qa/windows-native-smoke.md). This is local
reproducibility evidence, not a public GitHub release.

**Subsequent hardened task revision:** source commit
`c878520e4921dc08a0a444c501b6e01d092aaae2` adds input bounds and
task-cancellation handling. Its own x64 NSIS installer was built offline,
installed and smoke-tested. The latest installer is the 2,990,183-byte file
with SHA-256 `3eef254bd03fa754e0f9a4d778744eab8f4255fe3a7c26da42b33596c4e66f19`.
See the dated QA record for the corresponding installed executable identity.

## Verified work

- Original custom Tauri2/Svelte5 desktop app with Rust domain and native IPC.
- Rust SQLite state/event persistence, task lifecycle, idempotent operations,
  exact scope default-deny permission policy as a library, session-only keys.
- OpenAI-compatible provider contract; no hidden user-account or model
  download. Remote egress only after explicit provider selection and Ask.
- Two-agent group-chat path tested by Windows GUI automation with a labelled
  localhost **mock** provider: each selected agent replied once to the same
  new user prompt, and a repeated request skipped stored replies.
- Task creation, selected-model text execution and persisted result tested
  through the real installed Windows GUI using a labelled loopback fixture.
  Atomic task completion, cancellation and interrupted-request recovery are
  protected by Rust store tests. No OS coding-tool execution is wired.
- 200 prompt-only role specialties (42 foundational and 158 expanded), with
  substantive output expectations and deterministic validation tests;
  prompt guidance embedded in native request system instructions; NO role tool
  grants or autonomous coding execution.
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

Permissioned OS-tool task execution, production provider tests and costs, prompts
and MCP tool policy/role permission boundaries, file attachment/citation
flow, OS sandbox, encrypted backup/sync, runner, Android, cross-platform
packages and signing, security audit, 200 verified roles, 200 verified
development integrations, hosted CI evidence and actual public release.

The source checkout must **not** be described as a completed release. This
checkpoint is useful for implementation recovery, not user acceptance.
