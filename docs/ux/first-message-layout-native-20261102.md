# First-message layout: screenshot-discovered native UX correction

**Date:** November 2, 2026. **Source revision tested:** `3fccd0410c05f37dbce81e8841e5bff98a2e0198` from a separate clean detached Windows checkout, not the dirty feature worktree.

## Defect and fix

In the previous clean Windows native screenshot, the saved first message shared the scroll region with a compact room introduction. Auto-scrolling to the new message left the introduction partially cut off at the top of the transcript, duplicating the room name already present in the fixed toolbar. Previous screenshot: `target/ux-readiness-2ae3ad5/target/qa-native-clean-draft-2ae3ad5-20261102/draft-locally-sent-compact-room-5f38ce3d28fd4530bf0d3837460c9bec.png`.

The room introduction is now rendered only when the room contains zero messages. Once conversation begins, the fixed toolbar retains the room name, the sidebar remains the room/team entry point, and the transcript gives priority to search and actual messages. The compact-introduction CSS was removed instead of keeping orphaned styles. A source regression in `tests/chat-scroll-ux.test.mjs` asserts the zero-message condition and removal of compact state.

## Exact native observations

- Clean source Windows build command: `pnpm --dir apps/desktop tauri build --bundles nsis`; build PASS from `3fccd04`.
- Windows executable SHA-256: `F17C2244D2711B77A86FDD799ED2A05E97838352D9D76CA856F8E8077CB0C1FC`.
- Local NSIS bundle SHA-256: `E75F597D93F4EF56B80A62F56D2B9554988E992744D3227A4D29CEB88D8F87D3`. Built only; **no installation, upgrade, uninstall or release** was performed.
- Private Win32 desktop native run ID: `4eb51e705f554969a9e1d09ef3533649`. The two-launch test exercised isolated SQLite draft recovery, saved one local message, cleared the composer, and closed the QA process. Exit code 0, blockers empty.
- Separate native task/room-team GUI regression against the **same executable**: run `94236a13168f41de9859264bf2c895c9`, exit 0. Room/task prerequisites, task draft return, safe disabled Run, task filters, room team accessible checkbox, explicit save and audited SQLite membership persistence passed; `errors=[]`, `process_closed=true`, private synthetic SQLite profile confirmed.
- Current screenshot: `target/ux-readiness-2ae3ad5/target/qa-native-clean-first-message-3fccd04-20261102/draft-locally-sent-compact-room-4eb51e705f554969a9e1d09ef3533649.png`. Visually inspected: conversation search and first message visible from their beginning; no clipped or duplicate introduction; main sidebar/toolbar/composer remain usable at 1468×947.
- Source checks after the change: 164/164 JavaScript tests PASS; `svelte-check` 0 warnings, 0 errors; source gate PASS.

**Limits:** This validates one actual Windows WebView2 1468×947 native layout, not 200% page zoom, physical IME keyboards, screen readers, 10,000-message native performance, Windows installer lifecycle, Linux newest-source GUI, macOS or Android production security. These remain release blockers.
