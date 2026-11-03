# Windows keyboard, focus and accessibility: scoped native QA

**Scope and date:** November 2, 2026. UX worktree Git HEAD at test start: 0c8e9c822e3a85889a7f170b7eedd9708a24e8fa. Tested actual executable: apps/desktop/src-tauri/target/release/constellation-desktop.exe, SHA-256 **15B03E9CBC4FBF2F77E85C9F0F8440C63FEB9868D83A852BCAE7C560D33059C8**, 10,316,288 bytes. Hash describes the real executable, not proof that the dirty working tree was reproducibly rebuilt. No code build, real user data, provider access, active-desktop interaction, or releases.

## Live native private-desktop tests

Each run used the existing helper-hidden-desktop.ps1 containment on a uniquely created non-input WinSta0 desktop, verified the owned HWND was on that desktop, used native --isolated-data-dir, set private TEMP/TMP and WebView2 data, verified a private SQLite database and terminated only the launched process/contained descendants.

| Test | Result |
| --- | --- |
| Existing windows-installed-uia-smoke.ps1 -TestCommandPalette; run 4c836017ea3e47e598491798ca01470d | **PASS**: 60 actual UIA descendants, navigation controls, genuine InvokePattern palette open, ValuePattern no-result and Evidence searches, observed matching listbox, errors empty |
| Native HWND and visual evidence | **PASS**: actual 800×560 owned HWND, PrintWindow screenshot with 57 distinct sampled colors and process closed |
| New tests/ux-keyboard-private-desktop-20261102.ps1; run 8af977194d324ecc847c888bb0f798ec | **PASS**: hidden desktop and private database, actual UIA modal, combobox, Evidence listbox result, process termination, errors empty |
| Focus return after UIA Close dialog | **PASS**: source Search workspace button had HasKeyboardFocus=true before UIA opening, and again after UIA close |
| Input focus immediately after UIA opening | **NOT RESOLVED**: actual combobox Current.HasKeyboardFocus=false at sampled instant although accessible and ValuePattern worked. Could be transient/asynchronous or a real focus defect; one observation cannot prove either |
| Source keyboard and ARIA contract | **PASS 5/5**: node --test tests/ux-keyboard-contract-20261102.test.mjs, static evidence only |

Evidence files: apps/desktop/src-tauri/target/ux-keyboard-private-uia-20261102/installed-window-4c836017ea3e47e598491798ca01470d.png and apps/desktop/src-tauri/target/ux-keyboard-focus-20261102/ux-keyboard-focus-8af977194d324ecc847c888bb0f798ec.json. The runs produced real private-WebView2 UIA output and no user-profile access.

## Physical keyboard and 200% page zoom

**Not tested:** physical Ctrl+K, ArrowUp, ArrowDown, Enter, Escape. The hidden desktop is deliberately not the input desktop. Using Win32 SendInput or SendKeys could target the real user's active desktop. Sending PostMessage WM_KEYDOWN to a Chromium HWND would not establish physical keyboard processing or modifier state. No unsafe keyboard injection occurred. The UIA interaction above must NOT be represented as proof of physical keyboard events.

The source in apps/desktop/src/routes/+page.svelte contains Ctrl+K handling, Escape dismissal and modal previousFocus restoration; apps/desktop/src/lib/WorkspaceCommandPalette.svelte contains arrow wrap, Enter activation, accessible combobox/listbox/option semantics and active-descendant/selection attributes. Static tests assert source presence, not native operation.

**True 200% WebView page zoom: NOT PROVEN.** Setting force-device-scale-factor=2 changes device scaling, and resizing a native HWND to 800×560 changes window dimensions. Neither establishes browser zoom factor or 200% CSS reflow. No debugging-port bypass or application-source zoom API changes were performed.

**Accessibility scope:** Real UIA shows discoverable named controls and focus return after UIA close; these do not prove screen-reader announcements, physical keyboard order, true 200% reflow, contrast criteria, or full WCAG conformance. The observed combobox focus=false requires independent repetition.

## Safe completion path

Use a disposable interactive VM or isolated interactive Windows session that is never the user's real desktop. Deliver real keyboard Ctrl+K and verify focus in the combobox; ArrowUp/Down should change active option announcements; Enter should activate exactly one command; Escape must close the dialog and return focus to its opener. Verify Tab/Shift+Tab trapping and screen-reader semantics. For actual 200% page zoom, use the browser zoom command or WebView2 zoom API and independently measure zoom/reflow using trusted WebView metrics plus visual evidence, not forced DPI or window resizing.

Prime announced ongoing Windows Rust compilation and low RAM after these two runs. No new GUI processes were launched after the warning.

**Files added in assigned scope:** tests/ux-keyboard-contract-20261102.test.mjs, tests/ux-keyboard-private-desktop-20261102.ps1 and this report. All existing dirty app/CI files preserved. No Git staging, commit, push, or release.
