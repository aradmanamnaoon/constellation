# Constellation — local CI substitution and native acceptance evidence (2026-11-02)

**Source revision under test:** `c7f7cfa5575f11c8a9fe79c67b5daac55a239360` on `ux/competitor-rebuild-20261101`, checked out cleanly on Windows (`target/local-ci-clean-20261102`) and Ubuntu WSL2 (`/home/saba/constellation-local-ci-20261102`). The source SHA was independently checked on both. No GitHub runner executed these checks. The owner requested using their local machine instead of GitHub Actions; this document records observed checks, not invented green CI job results or a complete four-platform release.

## Replaced CI jobs: actual local results

| Hosted CI family / equivalent | Host and observed result | Limit of claim |
| --- | --- | --- |
| Source inventory, immutable action policy | Windows and clean Ubuntu `node .github/workflows/source-gate.mjs`: **PASS**, `required=77`, `tracked=457`, `source_files=244`; JS source-policy regressions also included in 141/141 | Not a signature, release, or external source review |
| Standalone Rust crates: format, test and strict Clippy | **Windows PASS**: 14 crates × 3 gates, **42/42 records exit 0** (ignored cache logs: `target/local-ci-windows-20261102/crate-results.ndjson`). **Ubuntu PASS**: `scripts/qa/run-ubuntu-wsl-ci.sh crates`, marker `WSL_CRATE_GATES_PASS`, all 14 manifests independently formatted/tested/linted with pinned `rustc 1.99.1` | Linux/Windows platforms, not Mac/Android Rust targets |
| JS tests and frontend semantic/accessibility regressions | **Windows 141/141 PASS**; **Ubuntu 141/141 PASS** (with TypeScript Node 22 compatibility loader, from same committed sources) | DOM contract tests are not screen reader or physical keyboard acceptance |
| Frontend package installation, Svelte diagnostics, production assets | Clean Windows checkout `pnpm install --offline --frozen-lockfile`, `pnpm check`: **0 errors/0 warnings**, `pnpm build`: **PASS**. Ubuntu source-pinned desktop package builds previously passed under earlier production-equivalent commit `c76498c` (next commit only changes local test scripts) | Not native installed application acceptance on Mac/Android |
| Desktop Rust format, check, lib tests and strict Clippy | Clean Windows checkout: `cargo fmt --all --check`, `cargo check --locked`, `cargo test --lib --locked` **130/130 PASS**, `cargo clippy --all-targets --locked -- -D warnings`: **PASS** | Linux desktop source compile `cargo clippy` **PASS**, but further Linux desktop tests/build were **safely aborted by low C: free space** (script refused when below 3 GiB); not equivalent to full Linux Tauri matrix |
| Native Windows GUI, provider and task user journeys | Source-equivalent native WebView2 EXE SHA-256 `9EDC5FA5A619D47E796BD67793BC5111D1FD53D0A67B3E359FEA92480E4BF057`; previously observed hidden private desktop native UIA run `ae361bda0f244298bcbc5fada8ca0c14` **PASS**, no provider network calls; task/room-team native run from earlier same production source separately recorded in `qa-results-2026-11-02.md` | A source EXE test does not certify the current-user NSIS installer lifecycle or physical keyboard |
| Linux native packages | Source-equivalent production binaries (loader-only changes since build): `.deb` SHA-256 `30525E8FF501B5F9D13E270CABB506557AA0DA77D22B55B33F1EE73673DFD400`; `.AppImage` SHA-256 `4364962C8A99A2DEB79B3BC9829E64C005DB341AA2F978B71FD8398B96D0C993`. Earlier **4/4 actual Ubuntu QA checks PASS**, including Xvfb GUI, rootless dpkg and real FUSE AppImage; see `qa-results-2026-11-02.md` | Not multi-distro physical install or AT-SPI screen reader QA |
| Docs, roles, complete plan and basic skill schema | Clean Windows checkout: role generator `--check` **PASS**, docs `--require-tracked` **PASS**, generated plan `--check` **PASS**, skills portable schema `validate.mjs` **PASS**; docs/plan/contrast/role tests **15/15 PASS** | Strict **installed/vetted/exercised 200 skills** gate **FAIL**: missing source/license/evaluation receipts for many entries. Structural manifest validation is not real skill installation/exercise |
| NSIS source/installer static inspection and disposable guest guard | Local Windows source-matched NSIS SHA-256 `F8BDF9DF1184DC68B4DD67DA994A92586C6B35CA656AFD81A0DD42070C3F4BA1`. `tests/windows-nsis-lifecycle-static-20261102.ps1` static audit and self-test **PASS**; `tests/windows-nsis-disposable-guest-20261102.ps1 -SelfTest` **PASS**, guards reject the physical host | Real upgrade/uninstall **NOT RUN**; fixed current-user HKCU identity can collide with previous Constellation install. Need a separate disposable Windows VM/user SID/hive |
| Android native source and SDK | Android SDK, NDK 28.2.13676358, `aarch64-linux-android` Rust target and Android Studio JDK 25 **detected**. Android static manifest/build-policy tests included in 141/141. The SDK directory is `platforms/android-37.0` (not `android-37`); no physical ADB device connected | **No fresh Android APK build or installed-device QA**: machine lacked the RAM headroom required by bounded Gradle/AVD checks; native Android consent, secure vault and authenticated checkpoint still unsupported. Old debug APK is nonrelease |
| macOS arm64 and x64 | VirtualBox VM inventory showed only `kali-linux-2026.2-virtualbox-amd64`. Windows/Ubuntu x64 cannot execute native macOS/Xcode/Cocoa/Keychain tests | **NOT RUN**, no eligible macOS machine/VM/Xcode/signing/notarization. Cross-compiling or guessing would not qualify |

## Observed failures and safety interventions

1. **Node 22 Ubuntu TS loader:** six test files initially failed because Ubuntu's system Node v22.22.1 advertises `--experimental-strip-types` yet is compiled without a TypeScript parser. Installed the exact lockfile-matched frontend dependencies **offline**, added `scripts/qa/local-node-ts-loader.mjs` and its registration module to transpile locally owned `.ts` test imports via the pinned TypeScript package, and updated `run-ubuntu-wsl-ci.sh` to use this route on Node below 24. The six affected files then **27/27 PASS**, full Ubuntu JS suite **141/141 PASS**. Node 24 on Windows remains native; no application production code affected.
2. **Ubuntu Git pointer:** the Windows worktree's `.git` file refers to Windows `D:` metadata and cannot be used directly by the Linux Git CLI. A separate **clean** Ubuntu worktree at the exact pinned commit was created without replacing or editing the Windows source. Source gate **PASS** there.
3. **Disk guard:** initial Ubuntu crate matrix refused a Rust build with less than 3 GiB on Windows C:. Only an identified, inactive legacy Cargo cache (`%TEMP%/constellation-check-target`) was **moved, not deleted** into the ignored project `target/archived-windows-cargo-cache-20261102` on D:, after checking no symlinks/running Rust jobs. C: increased from 2.83 GiB to 4.87 GiB free, allowing **all 14 Ubuntu crate gates to pass**. A second identified inactive build cache was similarly archived to `target/archived-windows-cargo-cache-worker2-20261102` when C: again approached the guard. Preserved user-owned archives, other caches, desktops, registry, signing credentials and all unrelated dirty files.
4. **Linux Tauri desktop:** source SHA verified and `cargo clippy` native Linux compile **PASS**; the runner then refused further tests because the WSL VHD filled host C: below the required 3 GiB. Do not mislabel Linux desktop test/build stage as passed in this clean checkout. Earlier source-equivalent Linux production packages/GUI acceptance remain separately valid.
5. **Toolchain:** Windows `rustup` lists a `1.99.1` alias whose direct selection lacks a manifest, while the functioning default Rust `rustc --version` is exactly 1.99.1. The local Windows crate matrix used the functioning default compiler without changing rustup installations; Ubuntu also uses 1.99.1.
6. **Full release preflight:** `tests/release-readiness.ps1` on a clean pinned checkout returned **exit 1**. Seven gates passed (clean commit, full source, remote, 15 lockfiles, source inventory, Git blob size, 200 role definitions). Four gates remain **FAIL**: actual 200 installed/vetted/exercised integrations, **188 pending/non-PASS native acceptance rows**, **1240 unverified execution tasks/23 source gaps**, and missing hosted/platform signature/installed release receipts. No release was published; these blockers are not solved by substituting a local CLI run for native cross-platform evidence.

## Reproduction, without changing the original worktree

All commands below refer to the clean, pinned checkout; preserve its exact source revision when comparing outcomes. Native test scripts use private synthetic data. They do not authorize external provider calls or real user-profile installation.

```powershell
# Windows clean checkout, with Cargo target cache reused from the original tree:
cd D:\coding\codex\constellation-ux-rebuild\target\local-ci-clean-20261102
node .github/workflows/source-gate.mjs
node --test tests/*.test.mjs
pnpm --dir apps/desktop check
pnpm --dir apps/desktop build
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --all --check
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib --locked
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
node catalog/build-roles.mjs --check
node docs/mission/generate-complete-execution-plan.mjs --check
node tests/docs-links.mjs --require-tracked
powershell -NoProfile -File tests/release-readiness.ps1

# Ubuntu WSL clean checkout; do not bypass the physical Windows disk guard:
wsl -d Ubuntu -- bash /home/saba/constellation-local-ci-20261102/scripts/qa/run-ubuntu-wsl-ci.sh preflight
wsl -d Ubuntu -- bash /home/saba/constellation-local-ci-20261102/scripts/qa/run-ubuntu-wsl-ci.sh crates
```

**Important:** local reruns can replace most unit, static, lint, build and synthetic native UI jobs, but cannot truthfully produce Mac test results on a Windows host, erase missing Android security implementation, bypass protected installer SID checks, create 200 missing integration receipts, or certify a public release before its evidence exists. The local-first approach is adopted; the acceptance gates remain fail closed.
