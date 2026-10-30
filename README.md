# ✳ Constellation — Your AI team. Your machine. Your rules.

**A local-first workspace for collaborating with multiple AI agents.**

![Real native Windows Constellation group room with two explicitly selected AI-agent roles and per-agent reply statuses](docs/qa/screenshots/windows-team-qa-current.png)

The screenshot is from a real Windows Tauri/WebView2 development build. The
two-agent response test used a deterministic localhost fixture, **not an AI
model**, and is documented with [actual Windows GUI evidence](docs/qa/windows-native-smoke.md).

> **Development status: functional prototype, not a production release.** The
> requested 200 role definitions are present, while the 200 vetted development integrations,
> secured coding execution, Android APK, remote runner, encrypted sync,
> cross-platform installers, full platform testing, and public releases have
> **not** been delivered. An experimental local Windows NSIS installer was
> built and tested separately; no production installer has been published. Do not
> expose private files, credentials, or high-risk workflows to this prototype.

## What is implemented

- A custom Tauri 2/Svelte 5 desktop UI featuring Home, Rooms, Tasks, a
  **200-template prompt-only Role Library** and Settings. Chat and role creation are
  directly backed by the Rust engine.
- **Explicit, attributed team chat:** save a message locally first, then choose
  a model provider and ask one or up to eight selected agents. Per-agent
  success/partial-failure is visible; retry skips already-saved responses.
- **Explicit text-only task runner:** record a task, choose a model, and run
  it on demand. The model's result is stored in the room with an atomic
  succeeded task state. Cancellation interrupts an in-flight response, and
  restarts mark unverified requests failed. OS tools and automatic coding
  execution remain unavailable in the desktop app.
- Copy a single room's attributed transcript as Markdown without sending it to
  a model or writing a file silently.
- A fully Rust-owned persistence crate (`crates/constellation-core`) with
  SQLite WAL, event-atomic changes, typed IDs, rooms, agents, messages, task
  state transitions, and idempotent operation IDs.
- A default-deny scope-aware permission policy **as a core library only**.
  A separate **unexposed experimental** tool library contains read, guarded
  write, process and Git operation prototypes with exact-scope permissions,
  one-use approvals, digest binding and receipts. These are tested building
  blocks, **not** a verified OS sandbox or an autonomous task runtime. No
  shell/desktop/browser execution is exposed to agents in the desktop app.
- OpenAI-compatible provider profile validation: remote HTTPS or explicit
  loopback HTTP; no API keys are persisted. The desktop sends room context to
  the explicitly selected provider **only after the user clicks Ask**.
  Provider/tool capability negotiation and live-provider certification remain open.
- An offline system-font, compact and responsive interface with keyboard
  navigation, paginated chat history and a disabled browser-only preview.

## Install and explore

**Windows x64:** A locally built, **unsigned experimental** NSIS installer is
generated at
`apps/desktop/src-tauri/target/release/bundle/nsis/Constellation_0.1.0_x64-setup.exe`
when `pnpm tauri build --bundles nsis` completes. That local file is not a
verified public download. Cross-platform installation packages and release
links do **not** exist yet. The latest validation/installer identity is recorded
in [native QA](docs/qa/windows-native-smoke.md).

Begin with the illustrated-style [getting-started guide](docs/getting-started.md)
and [provider/privacy notes](docs/providers.md). You can create a room and save
messages **without a model account**; remote inference may cost money and sends
the chosen room content to the remote provider.

To run a task, visit **Tasks**, create a plan assigned to an agent, select the
configured model in **Model for task execution**, then press **Run text task**.
The selected model receives the task instructions only. It cannot operate on
your files or run processes through this feature. A real installed Windows
fixture exercise is recorded in [native QA](docs/qa/windows-native-smoke.md).

## Develop

Prerequisites: Rust stable (MSVC on Windows), Node.js, Windows C++ Build Tools
and WebView2 on Windows. Other platforms require their documented Tauri system
dependencies. **Building the UI does not require a model account.**

```sh
cd crates/constellation-core
cargo test

cd ../constellation-tools
cargo test

cd ../../apps/desktop
pnpm install --frozen-lockfile
pnpm run check
pnpm tauri dev
```

If the D: volume lacks space for Rust debug binaries, relocate **only this
build's output** on Windows PowerShell before launching from `apps/desktop`:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp\constellation-cargo-target'
$env:CARGO_INCREMENTAL = '0'
pnpm tauri dev
```

The current project has not undergone a production security audit.

## Architecture

```text
apps/desktop/src       Svelte / view only, no direct shell permissions
apps/desktop/src-tauri Native command boundary and network request dispatcher
crates/constellation-core  Rust persisted state, provider request preparation,
                           task state machine, policy evaluation
crates/constellation-tools Experimental standalone local read/write/process/Git
                            approval library; NOT connected to native app,
                            NOT sandboxed or crash-audited
catalog/                  200 distinct prompt-only role instructions and tests
skills-manifest/          Truthful, validated inventory; 0/200 fully exercised
docs/                   Provisional architecture comparison, security and QA
assets/                 Original source vector emblem
```

Documentation:

- [Architecture decision](docs/architecture-decision.md)
- [Acceptance ledger](docs/quality/acceptance-ledger.md)
- [Initial threat model](docs/security/initial-threat-model.md)
- [Native Windows GUI smoke evidence](docs/qa/windows-native-smoke.md)
- [Getting started](docs/getting-started.md)
- [Provider and privacy boundaries](docs/providers.md)
- [Implementation roadmap](docs/roadmap.md)
- [Development tool evidence inventory](skills-manifest/README.md)
- [Mission checkpoint](docs/mission/status.md)

## Safety and limits

No arbitrary shell, filesystem modification, browser control, MCP tools,
auto-delegation, sync or remote runner is enabled. The desktop app can open
without a provider. Provider errors do not falsely mark tasks complete.
Running as the local user is **not** itself a security sandbox; a tested OS
containment layer must precede actual high-privilege coding tools.

**Licensing:** Repository license selection and dependency redistribution
review are pending. Do not publish an installer or copy the branding without
the final license review.
