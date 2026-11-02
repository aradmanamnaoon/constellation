# Native WebView page zoom and modal focus — Windows evidence

**Date:** November 2, 2026. **Source revision:** `ac33a80a848702755dd1022c2e875ad6145245cb`, built from the separate clean detached Windows checkout, not from the dirty feature tree.

## Implementation

- Added `studio_set_ui_zoom` as a local Tauri WebviewWindow command backed by the runtime `set_zoom` API. Rust rejects any percentage outside exactly 100, 125, 150, 175 and 200. It does not change display DPI, use CSS transforms, invoke providers, or grant native tool/file permission.
- Added accessible Settings buttons with pressed state, explicit native confirmation, failure message, and a local-only retained preference. The initial 100% state remains unchanged if the native command fails. Tauri documents that native WebView zoom is unavailable on Android, so this does not constitute Android parity.
- Updated modal focus to wait for Svelte layout settlement, ignore a destroyed dialog and avoid restoring outside focus when one modal replaces another. No physical keyboard events are injected into the user's input desktop.

## Native Windows results

The local Tauri release build and unsigned NSIS bundle both completed successfully.

- Exact executable SHA-256: `612E1B4AAD75EEFF6B8AF0F18E34DCD52852EB541D9F6A2E3EAEC0002123CEF9`.
- Local installer SHA-256: `A2F61CB27A06F7501722262656B05D57BA890EE401B0BC6723DD497AD8F3F460`. **Built but not installed or published.**
- Successful private-Win32-desktop UI Automation run: `515e15063bc943409745d67170bd1250`. It opened a real Tauri WebView2, invoked the palette and Settings, used the supported UIA TogglePattern for the 200% control, observed the native-acknowledged `Current page zoom: 200%` status, and independently measured physical UIA heading height **26 px → 50 px** (ratio **1.923×**). Errors empty, synthetic app profile created, process closed.
- Additional run `d4fcfa48f09b4a04b3b85bfb7a7fed46` repeated the exact 200% heading measurement and successfully invoked all **six top-level destinations** — Home, Rooms, Tasks, Evidence, Library and Settings — while real native 200% zoom remained active. Each control was found onscreen via UI Automation (not the user's keyboard/mouse); the synthetic app process closed and errors were empty. This confirms navigation reachability at this one window size, not complete per-screen responsive acceptance.
- Modal open/close through UIA and previous-focus restoration **passed**. Initial palette combobox `HasKeyboardFocus` was **false** at sampling, even though it was accessible and searchable. Do not describe this as proven initial-focus accessibility.
- Earlier attempts correctly failed closed while the hidden-desktop helper lacked a zoom flag and while the UIA probe assumed InvokePattern; the real Chromium toggle button exposed TogglePattern/ScrollItemPattern. The final script supports these observed patterns without physical mouse, keyboard, SendInput or unsafe fallback.
- Windows source checks: **165/165 JavaScript PASS**, Svelte 0 errors/warnings, Rust native zoom preset test PASS, strict desktop Clippy PASS.

## Remaining limits

The true 200% scaling test verifies one named heading and the native acknowledgement, not every window/control's responsive reflow, screen reader narration, IME, hardware shortcut navigation, contrast, or accessibility conformance. Windows NSIS installed lifecycle requires an independent Windows guest SID/HKCU. No macOS Intel/Apple Silicon, Android native security, or latest Ubuntu package acceptance is asserted. Full release gates remain open and the PR must remain draft.
