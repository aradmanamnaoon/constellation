# Windows native smoke verification — 2026-10-30

## Updated multi-agent Windows installer — second QA pass

The original 0.1.0 installer below was superseded **locally** by a new unsigned
development build containing two-agent chat, planning Tasks and per-role prompt
instructions. This is not a public or signed release, nor proof of compatibility
with a live commercial or local AI model.

| Verification | Observed result | Evidence |
| --- | --- | --- |
| Updated `pnpm tauri build --bundles nsis` | PASS | Final release profile completed and generated 1 Windows x64 NSIS setup |
| Updated installer size | 2,954,136 bytes | `Constellation_0.1.0_x64-setup.exe` |
| Updated installer SHA-256 | `e7af0f3c3c592b255b035ddd58922fde7e0091f1ca755bc2543c0a5b8d330f1f` | Direct Windows `Get-FileHash -Algorithm SHA256` on built artifact |
| Install to isolated `qa-installed-team` folder | PASS | Setup `/S /D=...`, exit code `0`, installed `constellation-desktop.exe` |
| Installed executable SHA-256 | `c85769bf84230a2caf83ea1c439ed8c341f60c86ed15444ea819f3f523b7756e` | 7,952,896-byte installed executable |
| Installed launch and accessibility hierarchy | PASS | PID 26276; native HWND 3673190; once WebView2 loaded, 111 UIA controls |
| Persisted local workspace | PASS | Room `QA Room Oct 30`, agents `Researcher` and `Literature Scout`, and latest QA message visible in installed binary |
| Updated explicit provider UX | PASS (dev GUI) | Provider starts unselected and `Ask` disabled. Selecting the test fixture enables manual Ask action |
| Two-agent team reply | PASS (dev **and latest installed** GUI, deterministic fixture only) | In the installed executable, saved `INSTALLED APPLICATION GROUP CONTRACT TEST`, explicitly chose `Contract fixture not AI · fixture-model`, selected Researcher and Literature Scout, invoked `Ask team (2)` and observed two separate labelled fixture messages and two `Replied` statuses |
| Team retry | PASS (dev GUI, deterministic fixture only) | Repeating a same-prompt team request changed results to `Skipped (already answered)` without extra messages |
| Task lifecycle | PASS (dev GUI) | Real UIA entered instructions, persisted `Pending · Not Executed`, invoked `Cancel task`, found `Cancelled · Not Executed`, no longer offered Cancel button |
| Installed room transcript copy | PASS | Invoked real `Room details → Copy transcript`; UI announced copied Markdown. Clipboard contained 1,253 characters with the room name, user QA prompt, attributed agents and earlier test message; no file write claimed |
| Installed provider-unavailable error | PASS (negative contract smoke) | Stopped the localhost fixture and confirmed port `127.0.0.1:18767` refused connections. Through the **installed** GUI, saved `OFFLINE CONTRACT TEST: this request should fail safely.`, clicked `Ask selected`; after the pending indicator cleared, UI showed `Researcher — Failed — Could not reach the model provider`, not a fabricated answer. Exact network error handling across all providers remains unverified. |

The [updated installed-window capture](screenshots/windows-team-installed-2026-10-30.png)
shows the native interface and successful transcript-copy notice. The
[two-agent developer GUI capture](screenshots/windows-team-qa-current.png)
shows manual provider and team selection plus two `Replied` statuses.

The user-facing app still does not offer an OS tool executor; any separate
experimental Rust tool library is **not** an audited production sandbox and
is not evidence of file modifications or successful coding tasks. No
macOS/Linux/Android installed GUI checks or signature/notarization checks were
performed for this updated installer. A clean-profile migration and upgrade
test remains required before distribution.

**Status: Narrow observed PASS on a Windows development build.** Not a
verified installed release or an audited agent-execution system.

## Environment and identity

- Project: `D:\coding\codex\constellation` (local uncommitted development
  tree at time of the first GUI capture; commit identifier not yet assigned).
- Windows 11, Windows WebView2/Tauri 2, local x64 Rust MSVC build.
- Rust 1.99.1, Node 24.17.0, pnpm 12.15.0.
- Launched with `pnpm tauri dev`; observed `constellation-desktop.exe` with
  window title `Constellation` and a nonzero main window handle.
- Windows UI Automation was used to discover actual controls, invoke buttons,
  and type through the real WebView2 user interface. The GUI automation
  process accessed the real desktop; this was not a synthetic browser DOM mock.
- No remote AI provider, live commercial API, paid model, or installed APK.

## Actual GUI operations

| Step | Observed result | Evidence / qualification |
| --- | --- | --- |
| Launch native app | PASS | Nonzero native HWND and visible branded UI; actual screenshot captured |
| First-run with no provider | PASS (smoke) | Calm empty room screen, no fabricated AI response or requirement to sign in |
| Create room | PASS | UIA invoked `Create your first room`, typed room name `QA Room Oct 30`, invoked `Create room`, then found `# QA Room Oct 30` in accessibility tree |
| Add role-based agent | PASS | UIA invoked `Add a teammate` and `Add teammate`; `Researcher` appeared in the sidebar with status message |
| Save user message | PASS | UIA typed `LOCAL GUI SMOKE MESSAGE`, enabled and invoked `Send message`, then found its text in the conversation |
| Restart and retain data | PASS (narrow) | `tauri.conf.json` change restarted the native development process; the same room, Researcher and saved message remained visible in UIA |
| Configure localhost provider | PASS (fixture only) | Native Settings modal accepted `http://127.0.0.1:18767/v1` and `fixture-model`; provider displayed in settings |
| Native HTTP request + response | PASS (contract fixture only) | Node loopback fixture at `tests/fixtures/mock-openai.mjs` returned `[FIXTURE ONLY] The local test server received your request.`; the native app displayed this attributed to Researcher after a real room message |
| Accessibility exposure | PARTIAL | UIA read actual navigation, modal names, edit controls, buttons and messages. Full keyboard/screen-reader/TalkBack/200% zoom audit **not done** |
| Preview and app icon | PASS (visual smoke) | Original Constellation icon in the title bar, desktop layout visually inspected |
| NSIS Windows installer generated | PASS (build) | `Constellation_0.1.0_x64-setup.exe`, 2,913,610 bytes, SHA-256 `9e73088101ee48370f0f6c78f9bc35730cc5726b612cb40a6f46792fd3639ebd` |
| Silent NSIS installation | PASS (Windows-only smoke) | Installer invoked with `/S /D=D:\\coding\\codex\\constellation\\qa-installed`; exit code 0; installed `constellation-desktop.exe` and `uninstall.exe` verified. Isolated QA destination was absent before install. |
| Installed binary starts | PASS (Windows-only smoke) | Launched `qa-installed\\constellation-desktop.exe`, observed native HWND 4983668, `Constellation` title and 68 UIA descendants. |
| Installed binary retained SQLite content | PASS (Windows-only smoke) | Installed application could read existing room, `Researcher` agent and previously persisted `CONTRACT FIXTURE TEST` user message from app data. This is not a clean profile or installer upgrade test. |

## Captures

- [Local native room view](screenshots/windows-room-conversation.png) — the
  saved user messages in the Rooms page.
- [Native provider contract reply](screenshots/windows-fixture-reply.png) —
  screenshot shows `Researcher` replying with the prominently labelled
  fixture text. **This was not generated by an AI model.**
- [Installed native executable screenshot](screenshots/windows-installed-native.png)
  — taken after launching the binary installed by the local NSIS setup.

## Automated and compile tests (separate claims)

- `cargo test --offline --quiet` inside `crates/constellation-core`:
  **10 integration tests passed** (3 security, 7 persistence/lifecycle).
- `cargo fmt --all --check`, `cargo clippy --all-targets --offline -- -D warnings`
  for standalone core: **PASS**.
- `pnpm run check` in `apps/desktop`: **0 errors, 0 warnings**.
- `pnpm run build` in `apps/desktop`: **PASS**, static Svelte production assets.
- `cargo check --offline`, `cargo clippy --all-targets --offline -- -D warnings`,
  `cargo test --offline --lib` in desktop Tauri crate: **PASS**, 3 native tests.

## Scope still unverified / unavailable

No authenticated **live** model provider test; no robust model capability
negotiation, streaming tests, or external egress/privacy pen test. The
permission gate is tested as a library, but a sandboxed agent executor with
real shell/Git/browser/desktop tools does not exist. This smoke is not a
substitute for the 200 roles, the 200 vetted development skills/MCP servers,
Android, macOS/Linux, E2EE sync, user-owned remote runner, signed installers,
or publication. The 188-row acceptance ledger remains open because its broad
end-to-end gate descriptions demand stronger, release-specific evidence.
