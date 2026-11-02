# Serialized workspace refresh (2026-11-02)

## Reliability defect

The desktop renderer calls `studio_bootstrap` after room, agent, task, message, provider and recovery actions. Multiple overlapping calls could complete out of order; a delayed earlier response could overwrite a newer snapshot in the UI, rewind visible task/chat state, reset selected room/provider controls, or distort unread counters and message scroll reconciliation. This is a renderer consistency issue, not a SQLite data-loss mechanism.

## Correction

The renderer's refresh operation now enters a tiny `SerialAsyncQueue`. It serializes **the whole refresh critical section**: evaluate current selection and scroll anchor, invoke the native SQLite snapshot, adopt the result, reconcile selection, settle the DOM and restore chat position. Every caller awaits its own refresh in invocation order. A failing job releases the queue rather than indefinitely blocking subsequent refreshes. The helper owns no browser storage, timers, network, provider retries or native permissions.

The queue only guards refresh work in one renderer; it is not a cross-window lock or transaction protocol. Native SQLite revision/approval protections remain authoritative. It does not make full-snapshot 10,000-message IPC bounded, nor does it prevent a user from changing views while a single refresh is in flight. Performance tradeoff: overlapping snapshot calls now run sequentially rather than redundantly in parallel; responsiveness under heavy load still requires native profile measurements.

## Verification

`tests/workspace-refresh-serialization.test.mjs` covers delayed first response vs second, failure recovery, 300 concurrent callers with a peak concurrency of one and source wiring of the full serialized native-and-DOM section. Initial validation: 169/169 JavaScript tests PASS; Svelte zero diagnostics. Native GUI and production binary testing are separate acceptance gates and must not be inferred from these tests.
