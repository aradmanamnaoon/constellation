# Constellation Ubuntu WSL local validation — 2026-11-01

## Source and environment

- Candidate: Git `be3d19ac80a45bc3376aaae5b09c2b34e1adc439` plus the small local Linux compilation fixes listed below. All commands ran against the same checkout; final commit SHA must be recorded after publication.
- Ubuntu WSL2 x86_64, Linux kernel `6.18.33.2-microsoft-standard-WSL2`, Rust `1.99.1`, Node `22.22.1`; frontend validation used Windows PNPM `12.15.0` with the committed frozen lockfile.
- The WSL ext4 VHDX resides on Windows C:. The `880 GB free` reported inside ext4 was **not** real host capacity: C: initially had under 1 GB free; D: had under 0.4 GB.
- Reviewed `%LOCALAPPDATA%\Temp\constellation-cargo-target\debug` and confirmed only regenerable Cargo debug output, no `.git` directory, and no running Windows Rust/compiler processes; removed that exact directory only, recovering roughly 10 GB on C:. Preserved release output, source, Git history, backups, and private QA.
- One missing Linux package, `libxdo-dev`, was installed from the Ubuntu package repository (64.9 KB archive, 688 KB of installed files). Existing GTK 3, WebKit2GTK 4.1, and Ayatana AppIndicator libraries were present.

## Observed gates

| Gate | Observed result | Scope |
| --- | --- | --- |
| Committed source manifest | `CI_SOURCE_GATE_PASS required=77 tracked=406 source_files=224` | Same checkout, WSL |
| Node contract suite | **106 passed, 0 failed** | WSL Node 22 |
| All 14 standalone Rust crates | Every crate's `cargo test --all-targets --locked` passed, including the project-archive integration suite | WSL Rust 1.99.1 |
| Unix process groups | 5 passed, 3 intentionally ignored helper fixtures | WSL, including unrelated-control protection |
| All 14 Rust crate Clippy suites | `--all-targets --locked -- -D warnings` passed | WSL Rust 1.99.1 |
| Linux desktop Rust | `cargo check`, strict `cargo clippy`, `cargo test --lib` **124 passed**, development `cargo build` passed | WSL Linux, NOT release installer |
| Native desktop startup smoke | Private `xvfb-run` kept the development process alive through its expected 8-second timeout (124); only Xvfb `libEGL/DRI3` software graphics warnings observed | No real GUI interaction/installed package verified |
| Frontend | Svelte check **0 errors, 0 warnings**, Vite build succeeded | Windows local PNPM 12.15.0, not an Ubuntu frontend install |

## Fixes made after WSL found real failures

1. `crates/constellation-runner-service/tests/inbox.rs`: added missing `std::fs` import inside the Unix-specific test block; the runner service's real protocol, cancellation and permission tests then passed.
2. `apps/desktop/src-tauri/src/lib.rs`: scoped dead-code lint exclusions to Linux/unsupported-OS implementations of **Windows native consent** and **OS-vault-backed checkpoint** modules, retaining strict Windows/macOS lint checks and the fail-closed unsupported-platform behavior.
3. `apps/desktop/src-tauri/src/native_multi_step_checkpoint.rs`, `turn_checkpoint.rs`, and `tool_host.rs`: gated OS-only imports/constant to their supported platforms; native Windows/macOS security code and Linux denial behavior were not altered.

## Repeating locally without GitHub Actions

Run from Ubuntu WSL, with the repository mounted at `/mnt/d/coding/codex/constellation`:

```bash
cd /mnt/d/coding/codex/constellation
bash scripts/qa/run-ubuntu-wsl-ci.sh preflight
bash scripts/qa/run-ubuntu-wsl-ci.sh crates
CONSTELLATION_WSL_DESKTOP_TARGET_DIR="$HOME/.cache/constellation-linux-desktop-final-20261031-worker1" bash scripts/qa/run-ubuntu-wsl-ci.sh desktop
```

The script refuses compilation when Windows host free space is critically low, uses existing Ubuntu Cargo caches with `--locked --offline`, never removes files, and does not push or create a release. A newly missing dependency must be deliberately fetched using the exact committed lockfile before rerunning.

## Release gates still open

- Linux production `.deb`/AppImage **not built or installed from this source**. Native installed GUI acceptance and provenance remain pending.
- macOS ARM/Intel signed/notarized installed binaries and Android production signing, on-device consent and ADB acceptance have **not** been proven by WSL.
- Windows code-signing and isolated installed NSIS lifecycle acceptance remain pending.
- GitHub hosted Actions were still blocked by GitHub account billing/budget settings when checked; WSL green does **not** certify an Actions run or public release. Do not publish experimental/older artifacts as final.
