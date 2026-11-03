# Serialized workspace refresh (2026-11-02)

## Reliability defect

The desktop renderer calls `studio_bootstrap` after room, agent, task, message, provider and recovery actions. Multiple overlapping calls could complete out of order; a delayed earlier response could overwrite a newer snapshot in the UI, rewind visible task/chat state, reset selected room/provider controls, or distort unread counters and message scroll reconciliation. This is a renderer consistency issue, not a SQLite data-loss mechanism.

## Correction

The renderer's refresh operation now enters a tiny `SerialAsyncQueue`. It serializes **the whole refresh critical section**: evaluate current selection and scroll anchor, invoke the native SQLite snapshot, adopt the result, reconcile selection, settle the DOM and restore chat position. Every caller awaits its own refresh in invocation order. A failing job releases the queue rather than indefinitely blocking subsequent refreshes. The helper owns no browser storage, timers, network, provider retries or native permissions.

The queue only guards refresh work in one renderer; it is not a cross-window lock or transaction protocol. Native SQLite revision/approval protections remain authoritative. It does not make full-snapshot 10,000-message IPC bounded, nor does it prevent a user from changing views while a single refresh is in flight. Performance tradeoff: overlapping snapshot calls now run sequentially rather than redundantly in parallel; responsiveness under heavy load still requires native profile measurements.

## Verification

`tests/workspace-refresh-serialization.test.mjs` covers delayed first response vs second, failure recovery, 300 concurrent callers with a peak concurrency of one and source wiring of the full serialized native-and-DOM section. Initial validation: 169/169 JavaScript tests PASS; Svelte zero diagnostics. Native GUI and production binary testing are separate acceptance gates and must not be inferred from these tests.

## Clean Windows native regression

Pinned source `54a79fa0c679cf734e0e1e321157953f05e33107` passed the clean Tauri release build. Executable SHA-256: `2F2B4F106B7A25B6A6A7770710BF02CCC217C040CADF3AADEBDFC5722D022214`. Unsigned, **uninstalled** NSIS SHA-256: `23ECBE8AABB6B9F413FD64E676DCC18AA2B7F773D8B4B3C75CCE85ECF8CCE1EF`.

Private Windows native UIA receipts using that exact binary: `593ebfd8e7214588bf9bd7525a7a1f94` (command palette, saved task/filter, room-team membership and persisted audit PASS); `161415d7998d4563baac482354840bff` (two-launch saved draft recovery, atomic local send and cleared composer PASS); `7a0cb0d74537438184dd1cb47abca2c0` (real native WebView 200% page zoom 26-to-50 physical UIA heading pixels, all six top-level sections reached, PASS). All three runs used isolated synthetic data, private Win32 desktops, and closed their owned processes without provider calls.

The first zoom-probe attempts were correctly rejected because the requested evidence path fell outside the QA harness's allowlisted application-target directory. Rerunning with the correct target directory passed using the unchanged executable. No actual Windows installer installation, 10,000-message native UI stress, hardware IME/keyboard, screen reader, or macOS/Android test is claimed.
