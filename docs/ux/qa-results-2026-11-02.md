# Constellation UX: native verification and release-readiness report

**Date:** 2026-11-02

**Tested UX source:** `9e6b5fc60048f5e1bbc4c5560e92c08cee301620` on `ux/competitor-rebuild-20261101`, following first pushed UX commit `6d9143ceebf71c595bda41e330506e398e6aacbb`.

**Scope:** Locally tested Windows x64 NSIS package and Linux amd64 `.deb`/`.AppImage` built from the current UX implementation. This is **not** proof of a complete four-platform release: Android, macOS, GitHub Actions, and additional native UX/security gates have explicit blockers.

## Final provider-directory UX implementation candidate

The new source-pinned UX follow-up is commit **`5f4aee67dee55fd63a34b4005066a533b1b4a447`**, which adds local provider-directory search and ephemeral test-result filters (no automatic provider/network request). This section supersedes the earlier binary hashes below **only for the newly implemented provider-directory UX**. Earlier results remain valid for the prior immutable candidate.

### New current-source native Windows artifacts

| Artifact | SHA-256 | Bytes | Signed |
| --- | --- | ---: | --- |
| `apps/desktop/src-tauri/target/release/constellation-desktop.exe` | `15B03E9CBC4FBF2F77E85C9F0F8440C63FEB9868D83A852BCAE7C560D33059C8` | 10,316,288 | NotSigned |
| `apps/desktop/src-tauri/target/release/bundle/nsis/Constellation_0.1.2_x64-setup.exe` | `C33BB3491CB9230993C19BFFE0B761E4226F4867A765AEE9836763BC3C95E42C` | 3,809,482 | NotSigned |
| Installed private QA EXE in `%TEMP%/ConstellationUXRelease20261102-v5/` | `8C6FC7685F80D5251B36A7CE7B0CDEC5D7F96052865FFF89C5E30AC659252FA7` | 10,316,288 | NotSigned |

The new NSIS was executed on a hidden desktop and installed in the disposable QA location. `tests/nsis-payload-compare.py` **PASS**: exactly three Tauri `UNK`→`NSS` marker bytes differ, at offset 9,384,607; all remaining bytes are identical. Native installed UIA run `0ddad6459f994e999b19b445192ec9d0`: **PASS**, real palette, room/agent/task creation, SQLite persistence, safely disabled unconfigured task Run, creation of a dummy loopback-only provider without network calls, local provider search matching/no-match states, native screenshot and clean process close, `errors=[]`. Native standalone provider-specific run `bb8d1dd89b0846139c2f7b83a485debe` also **PASS**; its real Settings screenshot was visually inspected, showing the filtered local model and status. The native installed historical SQLite migration/reopen **PASS**, schema version 1, integrity `ok`. Exact new standalone EXE also passed encrypted backup, restore and workflow preview/save/cancel native E2E with blockers `[]`, evidence under ignored `apps/desktop/src-tauri/target/ux-evidence-provider-final/`.

### New current-source native Linux artifacts

| Artifact | SHA-256 | Bytes | Result |
| --- | --- | ---: | --- |
| `target-linux/release/bundle/deb/Constellation_0.1.2_amd64.deb` | `6C0758AA1156E8C729712765BD2206BA1E949E18B93650FC4402100622C20AE7` | 5,152,688 | PASS |
| `target-linux/release/bundle/appimage/Constellation_0.1.2_amd64.AppImage` | `729DF1B2FBF944DE3FF1BD3EA2609AB9707DAF438EE3BD81484ABB2A1181FAF2` | 88,496,632 | PASS |

Both packages freshly rebuilt from the same provider-directory UX implementation and **passed real isolated Linux Xvfb GUI smoke** (`GUI_WINDOW_VISIBLE=YES`, `GUI_WORKSPACE_RENDERED=YES`, viewport entropy `0.28195`). This includes the `.deb` private extraction and AppImage extraction-mode GUI, **rootless isolated `dpkg --root` unpack** and exact executable byte comparison (installed ELF SHA-256 `c6cffc37121e62c72a5cc30da15755ab320ae2bf06f7056b194759433513bfe5`), and the **normal AppImage FUSE-mounted native GUI**. All four checks exited zero, with no host package-manager mutation. New session screenshot evidence under private WSL cache `run.ux-20261102074629-3398`, `run.ux-appimage-20261102074636-3707`, and `run.ux-fuse-20261102074649-4069`.

Frontend JS tests: **128/128 PASS**, Svelte `0 errors / 0 warnings`; strict source gate PASS (`required=77 tracked=441 source_files=236`). Rust library 130/130 and strict clippy passed earlier in this turn; no Rust production changes in this UX commit. All Windows and Linux binaries are **unsigned** and source-evidence does not establish macOS or Android parity. The Android debug diagnostic from an older JNI library still cannot serve as this commit's production release asset.

## Current source gates

| Check | Observation |
| --- | --- |
| `pnpm --dir apps/desktop check` | PASS, 0 Svelte errors and 0 warnings |
| `node --test --test-concurrency=1 tests/*.test.mjs` | PASS, 125/125 |
| `.github/workflows/source-gate.mjs` | PASS, `required=77 tracked=436 source_files=234` after explicit staging |
| `git diff --cached --check` | PASS for reviewed UX/QA changes |
| Rust library, clippy, formatting | Earlier PASS, 130/130 Rust library tests and strict clippy/fmt on production Rust unchanged by these UX commits; these are not a new full cross-platform Rust test run |

On 2026-11-02, `cargo test --lib --locked` was repeated in the actual Tauri crate and **130/130 passed**; `cargo clippy --all-targets --locked -- -D warnings` was repeated and **passed**. These checks did not alter production Rust sources.

## Windows native build, installation and execution

| Artifact | Exact SHA-256 | Bytes | Signature |
| --- | --- | ---: | --- |
| `apps/desktop/src-tauri/target/release/constellation-desktop.exe` | `7C80429474235DF7C20D29123EBFCE71C9C8C4019E8E6F073E2AB31A03E53757` | 10,316,288 | NotSigned |
| `apps/desktop/src-tauri/target/release/bundle/nsis/Constellation_0.1.2_x64-setup.exe` | `2150166D0613FD1E739D63D269763B865022B25EC5A14537EC020C9941EDC28B` | 3,807,235 | NotSigned |
| Real installed private QA payload in `%TEMP%/ConstellationUXRelease20261102-v4/` | `5D2762FACE765CD2D06EA900353696BC3E8BC93519A9157EF45EF9F31918BA5F` | 10,316,288 | NotSigned |

`pnpm --dir apps/desktop tauri build --bundles nsis`: **PASS**. The real NSIS installer was executed on a hidden/private Win32 desktop and installed into a disposable destination. Byte comparison with `tests/nsis-payload-compare.py`: **PASS**, exactly **3** differing bytes at offset **9,384,087**, exclusively the reviewed Tauri bundle marker (`UNK`→`NSS`); no other payload differences.

Native test evidence from `tests/windows-installed-uia-smoke.ps1` was bound to the **installed payload hash**, not a historical binary:

- Run `13e66fb2f94f4da693ea37d56d8cb76f`: **PASS**. Exact installed hash, real WebView2/UIA readiness, palette InvokePattern and ValuePattern unmatched/matching search, room and agent creation, preserved new-task draft, pending SQLite task persistence, disabled Run with no provider, status/keyword filtering, application-owned 800×560 minimum window, actual native screenshot, clean process exit and `errors=[]`.
- Run `00a52a65893e44da8e649219dd95d287`: **PASS**. Ordinary 800×560 palette screenshot (57 sampled colors) visually inspected: query, matching Evidence destination, description, and keyboard guidance all visible together without excessive spacing.
- Run `a3ad02e7b6ed44488637f6aa37ea771d`: **PASS as a diagnostic** for private `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-device-scale-factor=2`, same 800×560 application-owned window, real palette UIA and screenshot. The matching result is at least partly visible. This **does not establish** actual browser zoom, full reflow, or screen-reader accessibility.
- Installed historical SQLite schema migration and two-launch native reopen: **PASS**, `NATIVE_FIRST_LAUNCH_PASS`, `MIGRATED_USER_VERSION=1 INTEGRITY=ok`, `NATIVE_HISTORIC_REOPEN_PASS; REAL_USER_PROFILE_UNTOUCHED`.
- Standalone native `tests/windows-evidence-backup-e2e.ps1 -TestWorkflows` against the new standalone hash: **PASS**, exit 0, blockers `[]`; actual source byte-span/hash evidence, isolated encrypted backup and restoration, workflow preview/save/reload/cancel and real screenshots. Test run evidence under ignored `apps/desktop/src-tauri/target/ux-final-evidence-recovery-v4/`. No paid model-provider requests.

These tests do not establish a signed Windows binary, complete uninstall/upgrade lifecycle, all UI controls or hardware/assistive-technology accessibility.

## Linux native build and GUI rendering

| Artifact | Exact SHA-256 | Bytes | Native result |
| --- | --- | ---: | --- |
| `target-linux/release/bundle/deb/Constellation_0.1.2_amd64.deb` | `77F448565D9F15C81790AB107D0AEF9E847E7338C23B4234711D404200AD2A8E` | 5,151,796 | PASS, Ubuntu WSL2 native Xvfb |
| `target-linux/release/bundle/appimage/Constellation_0.1.2_amd64.AppImage` | `057BF842D5AA8BD1AA1A60F10C8713AB98653742E6E06AA68E68EA8630FDCFF8` | 88,496,632 | PASS, isolated extracted AppImage under Xvfb |

Fresh `scripts/qa/rebuild-local-linux-ux.sh deb,appimage`: **PASS** with the same final UX implementation. The rebuild helper excludes volatile generated frontend files to prevent copying a concurrently rewritten Vite/Svelte target. Both isolated Linux GUI scripts returned exit 0 with `GUI_WINDOW_VISIBLE=YES`, `GUI_WORKSPACE_RENDERED=YES`, and viewport entropy `0.28195`. Separate evidence screenshots:

- `.deb`: `~/.cache/constellation-linux-packages/run.ux-20261102065901-3389/isolated-gui/screenshot.png` inside the private WSL Ubuntu home
- `.AppImage`: `~/.cache/constellation-linux-packages/run.ux-appimage-20261102065916-3740/isolated-gui-appimage/screenshot.png` inside the private WSL Ubuntu home

The initial AppImage test above used `APPIMAGE_EXTRACT_AND_RUN=1`; an additional real-FUSE test subsequently passed, as documented below. Clean Debian/Ubuntu upgrade/uninstall validation, Linux accessibility/AT-SPI, and native provider/recovery parity remain unverified.

### Additional actual Linux package verification (2026-11-02)

Two new reproducible QA scripts exercise the exact final package SHA values without modifying the system package manager or visible desktop:

- `scripts/qa/run-local-linux-rootless-dpkg.sh`: **PASS**. The inspected `.deb` control archive contained no maintainer hooks, the package (`constellation` 0.1.2 amd64) was unpacked using a disposable private `dpkg --root` database and unprivileged account, and the installed executable matched an independently extracted package binary byte-for-byte. Output included `ROOTLESS_DPKG_UNPACK_PASS`, package SHA `77f448565d9f15c81790ab107d0aef9e847e7338c23b4234711d404200ad2a8e`, installed ELF SHA `bbe3bcc8611c5fde32a5334b734c4d01a73a43e1f6a731a3b3e86935824ab878`.
- `scripts/qa/run-local-linux-appimage-fuse-smoke.sh`: **PASS**. Normal kernel FUSE AppImage launch (without `APPIMAGE_EXTRACT_AND_RUN`) inside disposable Xvfb HOME, same original AppImage SHA `057bf842d5aa8bd1aa1a60f10c8713ab98653742e6e06aa68e68ea8630fdcff8`. Observed `GUI_WINDOW_VISIBLE=YES`, `GUI_WORKSPACE_RENDERED=YES` and screenshot viewport entropy `0.28195`, evidence under the private WSL Ubuntu home `~/.cache/constellation-linux-packages/run.ux-fuse-20261102072550-317/isolated-gui-appimage-fuse/`.

These additional checks close earlier **rootless dpkg unpack** and **normal FUSE AppImage runtime** gaps. They do not establish a configured install on a pristine Debian 12/Ubuntu 24.04 VM, host package upgrade/uninstall, AT-SPI, or provider/recovery feature parity.

## Android: debug diagnostic only

See [`android-local-qa-2026-11-02.md`](android-local-qa-2026-11-02.md) for the complete source/provenance and environment limitations. The production-compatible API37/AGP9.3.1 Gradle copy compiled a **debug** x86_64 APK from the existing earlier native library through an isolated HTTPS Maven mirror. Its SHA-256 is `77849DD1B137E15C76C10C0AE8CA99D36169278007D91FFA9FF2470E462EAEF0`; the app ID, 0.1.2 version, target SDK 37, x86_64 ABI and v2 debug signature were verified. An isolated emulator was created but never registered an ADB serial, even using private ADB keys/server; QA-owned QEMU processes were stopped. **No APK installation, Android app startup/restart, production signature, consent/parity or release validation passed.** Never distribute this disposable diagnostic as a release.

## macOS and GitHub CI

- No actual local macOS/Xcode machine or VM is available for Intel or Apple Silicon. Neither current-source DMG was locally built, installed or GUI-tested, and signing/notarization is unverified.
- GitHub Actions acceptance run `41328445830` and Validate run `41328445763` failed **before any code executed**. GitHub's own check-run annotation states: *The job was not started because recent account payments have failed or your spending limit needs to be increased.* Jobs had `runner_id=0`, no runner name and `steps=[]`. This is an **account billing/Actions spending-limit blocker**, not evidence of a source test failure. The GitHub account owner must resolve **Settings → Billing & plans** and rerun. Hosted CI then needs its own actual pass evidence.

## Release decision

**Do not claim or publish a complete Windows/macOS/Linux/Android release.** Windows and Linux have meaningful fresh native evidence, but Android has no tested production APK, macOS has no local native builds/testing, GitHub CI is blocked, Windows signing is absent, and several native accessibility/negative/installer matrices are unfinished. A clearly marked earlier `v0.1.2-local-desktop-preview-20261101` is not a substitute. Pushed UX source is reviewed separately in draft PR #1; the canonical worktree's unrelated changes and normal user profiles remain untouched.
