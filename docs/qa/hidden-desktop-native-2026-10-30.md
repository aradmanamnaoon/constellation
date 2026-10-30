# Non-visible real Windows GUI launch — 2026-10-30

**Purpose:** Literally start the real Constellation release executable while
preventing the application's window from appearing on, or requesting focus on,
the user's interactive desktop. **This is a local QA observation, not a public
release acceptance certificate.**

## Method

- `tests/windows-hidden-desktop-smoke.ps1` creates an independent unselected
  `WinSta0\ConstellationQA_<random>` Win32 desktop with `CreateDesktopW`.
- `CreateProcessW` starts the real release binary on that desktop via
  `STARTUPINFO.lpDesktop`; the test **never calls `SwitchDesktop`**.
- A fresh STA thread bound to the QA desktop with `SetThreadDesktop` reads
  UI Automation descendants from the actual Tauri/WebView2 window.
- On exit, the test terminates **only its own process** and closes its desktop
  handle. It does not use screenshots from the user's visible desktop.

## Final repeatable passing run

Command:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests\windows-hidden-desktop-smoke.ps1 -Executable 'C:\Users\saba\AppData\Local\Temp\constellation-release-target\release\constellation-desktop.exe' -TimeoutSeconds 30
```

| Observed measurement | Value |
| --- | --- |
| Native QA process ID | 19680 |
| Actual window title | `Constellation` |
| UI Automation descendants | **148** |
| Expected controls | `Rooms`, `Tasks`, `Library`, `Settings`, plus `Home`, `Evidence`, agent names in the observed tree |
| Foreground HWND before/after | **Unchanged** |
| App process during inspection | **Alive** |
| Isolated QA desktop | `ConstellationQA_eac2f633cdcb4fddb969cff14781a465` |
| Script status | Exit **0** |
| Release executable bytes | 8,505,344 |
| Release executable SHA-256 | `7908674F231CD682BCE6C089A5569ADC5F05D58EBEC47289C90F3ACBF350BB1A` |
| Built Windows NSIS installer bytes | 3,128,298 |
| Installer SHA-256 | `AF378F50A692F653090919F746053538BA36110758E48F3FE91F2915DC10999A` |
| Executable and installer signatures | **NotSigned** |

**Important limitation:** Attempts to *invoke* WebView2 UIA controls across the
non-selected Windows desktop stalled or timed out and are **not** counted as
working clicks. The default script remains read-only; interactive automation
must be enabled explicitly with `-TryInteractiveUia`. Native Rust contract tests
independently exercised the six provider transports against loopback fixtures,
but that is not equivalent to clicking through those flows on this invisible
desktop. No live paid AI calls occurred. Full installed UI interaction,
cross-platform packaging, OS tool sandbox, 200 exercised MCP tools and public
release acceptance remain outstanding.
