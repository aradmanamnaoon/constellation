# Synthetic 20,000-record SQLite snapshot regression

**Date:** November 2, 2026. **Scope:** Test-only addition to `apps/desktop/src-tauri/src/lib.rs`; production data schema, IPC APIs, platform/security mechanisms, UI, and provider dispatch are unchanged.

The earlier 10,000-message/200-agent fixture tested only in-memory `StudioSnapshot` projection. This new test exercises a real local SQLite database with 10,000 messages and 10,000 pending tasks in one room, one synthetic agent, and deterministic text. It bulk-inserts rows into a disposable fixture using one SQLite transaction; these test inserts intentionally bypass normal audited user/task creation and are **not** evidence for approval or audit behavior.

The test calls the actual `StudioStore::snapshot()` and `snapshot_as_view()` path, checks both 10,000-entry arrays plus representative boundary/midpoint message content, sender names and task instructions, closes the store, and reopens the same database to verify durable row counts. No model/provider requests or native file approvals are made.

**Observed Windows test:** `sqlite_snapshot_with_ten_thousand_messages_and_tasks_keeps_all_rows` PASS; total snapshot read and projection took **0.218 seconds** in this isolated run. This is a single-machine observation, not an SLA or a latency assertion. After adding the regression: desktop Rust **135/135 PASS**, strict offline Clippy PASS, JavaScript **169/169 PASS**, and `svelte-check` zero errors/warnings.

**Remaining acceptance:** IPC payload transmission into WebView2, DOM/search/scroll latency with real 10k messages and 10k tasks, memory high-water, provider history bounding, platform-installed UX, and 200% high-DPI responsive interaction under such data volume are **not yet verified**. Direct test SQL rows do not establish provenance for real user-created records. No release gate is closed solely by this regression.
