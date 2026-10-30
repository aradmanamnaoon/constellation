# Constellation implementation checkpoint — 2026-10-30

> **Superseded for Windows installer evidence:** This was an earlier source
> checkpoint. The updated team-chat NSIS installer was subsequently built and
> installed, and native GUI copy/transcript/persistence tests were observed.
> See [the updated dated Windows smoke record](windows-native-smoke.md).
> Open cross-platform, production-provider, sandbox and release gates below
> remain unresolved.

This report concerns the local Windows working tree at `D:\coding\codex\constellation`.
It records a **development snapshot**, not a release acceptance certificate.
The repository had no initial commit or remote configured during this check.
Changes were also being made concurrently in the shared working tree, so repeat
the commands against a fixed commit before promotion or public release.

## Changes now present

- Rust core owns SQLite data, audit events, provider metadata, and task lifecycle.
- Native Svelte/Tauri chat supports room-local message saving; selecting a
  provider and asking selected agents or a team is a separate action that
  explicitly discloses the outbound room context.
- Multi-agent response orchestration is sequential and bounded to eight
  selected agents. Its unit tests exercise replay, partial failures, stale
  prompts and later user messages. Native tests do **not** substitute for
  real-model or installed-app tests.
- Task creation and cancellation are accessible from the desktop interface;
  tasks remain pending and no shell or tool executor starts them.
- Changing a provider profile to an empty API key clears a prior session key.
  The retained single-agent native command now requires explicit agent and
  provider IDs, with no implicit latest-provider fallback.
- The UI keeps unsent text associated with the selected room; async message
  completion does not clear another room's draft.
- Message-sending retries reuse stable native operation and message IDs for
  the same text within the active UI session, reducing duplicates after
  uncertain IPC failures.
- Several low-contrast secondary text styles were darkened. On viewports up to
  760px, the closed workspace sidebar is hidden from keyboard focus via CSS;
  this is source-level work, not a completed screen-reader acceptance test.

## Checks actually observed

| Area | Command | Observed |
| --- | --- | --- |
| Rust core | `cargo test --offline` in `crates/constellation-core` | 13 tests passed (3 export + 3 security + 7 persistence) |
| Native Rust | `cargo fmt --all --check` in `apps/desktop/src-tauri` | Pass |
| Native Rust | `cargo clippy --all-targets --offline -- -D warnings` | Pass |
| Native Rust | `cargo test --offline --lib` | 8 tests passed |
| Svelte/TypeScript | `pnpm run check` in `apps/desktop` | 0 errors, 0 warnings |
| Frontend bundle | `pnpm run build` | Pass |

These results come from local commands. They cannot establish correct app
behavior on macOS, Linux, Android, or actual remote model providers.

The [earlier Windows native smoke report](windows-native-smoke.md) documents
room creation, an agent, persisted messages, and a deterministic local test
provider reply in a **development** WebView2 app. That run precedes the latest
team-reply and task-UI edits. A previous NSIS installer in
`apps/desktop/src-tauri/target/release/bundle/nsis` must likewise not be
represented as a verified artifact of the latest source revision.

## Outstanding acceptance work

1. Build a clean, fixed-revision Windows installer and perform actual
   installed GUI tests for task create/cancel, room draft switching, provider
   selection, team replies, keyboard navigation and error recovery.
2. Verify provider routing against real authorized endpoints and hostile
   local HTTP fixtures, including timeouts, non-JSON responses, oversized
   payloads, token and request budgets, cancellation, and inaccessible hosts.
3. Preserve operation IDs through uncertain app restarts, and document or
   implement crash-safe draft recovery.
4. Audit accessibility contrast, keyboard focus on hidden sidebars, modal
   focus isolation, 200% scaling, Persian RTL and IME input.
5. Implement a permission-enforcing sandboxed local tool runner, then add
   task execution receipts before claiming Claude Code class capability.
6. Deliver and vet 200 substantive role definitions and 200 distinct real
   skill/MCP integrations with provenance, usage tests and permission controls.
7. Verify Linux/macOS installers, Android restrictions, optional user-owned
   runner/sync, security/license gates, and hosted CI on immutable builds.

The exhaustive requirements inventory remains in
`docs/quality/acceptance-ledger.md`. Do not mark broad release gates `PASS`
based on this checkpoint.
