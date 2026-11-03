# Large-workspace snapshot: sender lookup optimization

**Scope:** Constellation Rust desktop view projection; no schema, authorization, file, model, network, or stored-history changes.

## Defect

The native `snapshot_as_view` projection previously performed a full scan of `snapshot.agents` for **each agent-authored message** to display its sender name. Given M messages and A agents, sender lookup cost was O(M × A) even after the core loaded its persisted snapshot. This is unnecessary work in large local conversation histories.

## Correction

The projection now constructs a typed `HashMap<AgentId, &str>` once and looks up agent names per message. Expected lookup cost is O(A + M). Message order, UUIDs, text, timestamps, provenance shape, and historical fallback `Agent` for removed/missing agent IDs are preserved. This does not truncate messages or silently change the project snapshot.

The projection is factored into `snapshot_to_view(&StudioSnapshot)` for direct synthetic testing independently of SQLite and provider dispatch. A regression supplies **10,000 messages from 200 agents**, verifies the exact first, last, and boundary message identities/names and full array cardinality, and separately verifies the missing-agent fallback. On the local Windows Rust test runner the focused test passed in approximately 0.07 seconds. No hard timing threshold is used because timing varies by machine.

**Not verified by this test:** full SQLite snapshot load duration, WebView2 IPC serialization at 10,000 messages, native 10,000-message interaction/scroll-search latency, whole-project RAM growth, or 10,000 tasks. The current renderer still receives full snapshots while rendering only 100 chat DOM message nodes per page. True end-to-end performance acceptance remains open and must use synthetic native test profiles with measured timings.

## Clean-source Windows regression evidence

Built exact revision `4e20b89e491814692f3bee7593e6e11126607a46` from the clean detached Windows checkout using `pnpm --dir apps/desktop tauri build --bundles nsis`. The resulting executable SHA-256 is `EC4D0395D74130C4385B9D94782A4ECD44FD2FD653DE0E02608E86DEA2A18D40`; the **uninstalled, unsigned** NSIS bundle SHA-256 is `27C17A2CB31DAC793F2EF6A6EFCB3363DE3E525A067C8F0B31F31164A815F4E9`.

The exact binary passed separate private-desktop UIA runs: `4b3b8e5965924ae7b2a451f0f3e67f77` (command palette positive/no-match, task creation and filtering, room-team accessible membership save with SQLite evidence), `b70f20af891b4c1ea6187c4bd9c0c2f5` (actual 200% WebView page zoom: measured heading 26→50 px; all six top-level destinations accessible), and `65b420e24c104b308491c85afae81db9` (private two-launch persisted draft recovery, local message save and composer clear). All exited 0, used synthetic local data, closed owned app processes, and did not contact providers or install into live HKCU. The app-source regression suite passed **134/134 Rust** desktop tests, **165/165 JavaScript** tests, Svelte zero diagnostics, and strict desktop Clippy.

These tests confirm no observed regression in the exercised workflows; they do **not** turn the synthetic in-memory 10k snapshot test into a 10k native UI stress receipt. No final release is authorized by these tests alone.
