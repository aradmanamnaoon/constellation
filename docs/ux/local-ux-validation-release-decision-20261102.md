# Constellation UX — local validation and release decision

**Date:** 2026-11-02

**Code revision verified:** `8c9478a55cb1a2634f03382637ecc46b65350b6f`

**Development branch:** `ux/competitor-rebuild-20261101`
**GitHub PR:** <https://github.com/aradmanamnaoon/constellation/pull/1> (draft, unmerged)

## Source and local test evidence

| Target | Observed check | Result / bounded claim |
| --- | --- | --- |
| Windows x64 | Full Node regression, native Rust `cargo test --lib --locked` and strict Clippy, Svelte check | 156/156 Node, 8/8 core and 132/132 desktop Rust tests pass; Clippy passes; Svelte 0 errors/0 warnings |
| Windows x64 | `pnpm --dir apps/desktop tauri build --bundles nsis` | PASS; release-mode executable SHA-256 `C67E5C688D257DE4F63E7CA1EAEB574948BA7A2CBF775F1444039DEC76831E55`; unsigned NSIS installer 3,833,583 bytes, SHA-256 `9C18BA238ED5D56E34BA2B3DB2D8460BB3B90A2AD31EEFD7F23F42B316E7BE4A`, built but **not installed** |
| Windows x64 | Private WebView2/UIA provider Settings journey | PASS, disposable run `baed69b19f73483da5b8af20b00676d4`: local provider filtering, safe revision, explicit credential-free removal, persisted SQLite history/event check, zero network/provider requests |
| Windows x64 | Separate private WebView2/UIA palette/task/room-team journey | PASS, disposable run `853ebeb2e4014a0681022cfafd50e51c`: command-palette search, room/team setup, task planning and filtering, audited team persistence; no personal desktop input |
| Windows x64 | Provider Settings remembered-key deletion | **Not yet native vault tested**; source checks and Core CAS unit tests pass, but no disposable independent OS-vault user/profile exists. No real keyring deletion or remote provider call performed |
| Windows x64 | NSIS static audit and disposable guest harness self-tests | PASS for source/contracts/negative guards. **Real install, upgrade and uninstall NOT RUN**: current-user HKCU installation identity is shared with a pre-existing app. Do not run on physical host |
| Ubuntu WSL2 x86_64 | Fresh detached Linux checkout reconstructed from Windows-verified Git bundle, exact HEAD `8c9478a` | Source gate PASS; offline dependencies; **156/156 Node tests** and all **14 standalone Rust crate format/test/strict Clippy gates PASS** (`WSL_CRATE_GATES_PASS`). Linux Rust/native packaging is a separate gate |
| Ubuntu WSL2 x86_64 | Fresh `.deb` / AppImage from `8c9478a` | **NOT RUN**: C: free space dropped to approximately 3.1 GiB and host available RAM to approximately 1.9 GiB after the source matrix. The project script refuses further compilation under 3 GiB; avoid triggering WSL VHD growth or risking host stability. Older-source native package results must not be labeled current-source acceptance |
| Android ARM64 | Signed production APK and installed device native security | **NOT RUN / NOT IMPLEMENTED**: secure native consent, OS Keystore, authenticated checkpoint parity and signed ARM64 installed QA are outstanding. Previous x86_64 debug artifact was not built from this exact source |
| macOS Intel / ARM64 | Xcode DMG native build, install, UI, signature/notarization | **NOT RUN**: no accessible genuine macOS/Xcode test environment. Windows/WSL cannot certify Cocoa, Keychain or Gatekeeper |

## Native-test failure that is not an application regression

A combined QA fixture created task history before expecting **Remove unused** to delete a provider. The native Core correctly blocked removal, preserving provider provenance. A read-only SQLite examination confirmed two retained profiles and no `provider_removed` event. Both provider-only and task/team-only journeys were rerun independently and passed. Never loosen the history/permission fence to satisfy an invalid combined test expectation.

## Release readiness — authoritative fail-closed result

Executed `tests/release-readiness.ps1` against a **new, clean, detached Windows worktree** at `8c9478a` (not the dirty development checkout). The preflight returned exit code **1**, with four independent failed gates:

1. `required_installed_exercised_integrations`: 200 vetted/exercised skill integrations not proven.
2. `master_acceptance_ledger`: 188 rows, all 188 remain non-PASS.
3. `complete_execution_inventory`: 1,240 tasks are not independently VERIFIED DONE and 23 source gaps remain.
4. `hosted_ci_signatures_and_public_release`: no accepted hosted/build signature chains, actual platform install receipts or final publication verification.

The source/lockfile/remote/role/git-history gates passed. Passing individual tests does **not** bypass the four failing gates. The existing public `v0.1.2-local-desktop-preview-20261101` remains accurately labeled. **No new release or tag was created; the UX PR remains draft/unmerged.**

## Unblocking requirements

Provide a disposable Windows VM with separate SID/HKCU hive and an independently verified old installer for full NSIS lifecycle; a production Android ARM64 device/emulator plus implemented native consent/Keystore/checkpoint and safe signing; genuine macOS Intel/ARM64 native test environments with Xcode; enough host disk and RAM headroom for a fresh Linux native packaging and installed GUI run. Restore GitHub Actions execution by resolving the account-level billing restriction before claiming hosted workflow evidence. Additionally finish full keyboard, real 200% page zoom, IME, screen-reader, large history stress, live-provider revocation in a disposable OS account, and the remaining 188-row/1240-task UX/security/feature acceptance work.

**Decision:** UX remains **in progress**; publish **nothing new** until the required acceptance checks are actually observed. Continue development only on the isolated feature branch; preserve canonical `main`, unrelated dirty files, existing releases and user profiles.
