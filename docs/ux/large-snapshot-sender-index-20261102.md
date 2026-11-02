# Large-workspace snapshot: sender lookup optimization

**Scope:** Constellation Rust desktop view projection; no schema, authorization, file, model, network, or stored-history changes.

## Defect

The native `snapshot_as_view` projection previously performed a full scan of `snapshot.agents` for **each agent-authored message** to display its sender name. Given M messages and A agents, sender lookup cost was O(M × A) even after the core loaded its persisted snapshot. This is unnecessary work in large local conversation histories.

## Correction

The projection now constructs a typed `HashMap<AgentId, &str>` once and looks up agent names per message. Expected lookup cost is O(A + M). Message order, UUIDs, text, timestamps, provenance shape, and historical fallback `Agent` for removed/missing agent IDs are preserved. This does not truncate messages or silently change the project snapshot.

The projection is factored into `snapshot_to_view(&StudioSnapshot)` for direct synthetic testing independently of SQLite and provider dispatch. A regression supplies **10,000 messages from 200 agents**, verifies the exact first, last, and boundary message identities/names and full array cardinality, and separately verifies the missing-agent fallback. On the local Windows Rust test runner the focused test passed in approximately 0.07 seconds. No hard timing threshold is used because timing varies by machine.

**Not verified by this test:** full SQLite snapshot load duration, WebView2 IPC serialization at 10,000 messages, native 10,000-message interaction/scroll-search latency, whole-project RAM growth, or 10,000 tasks. The current renderer still receives full snapshots while rendering only 100 chat DOM message nodes per page. True end-to-end performance acceptance remains open and must use synthetic native test profiles with measured timings.
