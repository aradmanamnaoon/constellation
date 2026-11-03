# Constellation release: owner actions and hard external blockers

**Updated:** 2026-11-02. **Project:** `aradmanamnaoon/constellation`. This document distinguishes actions the application maintainer can perform on the connected Windows host from account/hardware/security prerequisites the maintainer cannot invent.

## P0 — GitHub Actions blocked before execution

Observed on UX SHA `3429f88753673ffd00d50408417340f5fb35e4c1`: Validate `41390556316` and Acceptance `41390556318` were both marked failed with `runner_id=0` and `steps=[]`. GitHub's exact job annotation: **"The job was not started because recent account payments have failed or your spending limit needs to be increased."** This is not a code-test failure and cannot be fixed by retrying builds locally.

**Owner action:** Visit <https://github.com/settings/billing>; resolve any overdue charge, payment method, and GitHub Actions budget/spending limit in **Settings → Billing & licensing / Billing & plans**. Check the owning billing account (personal account or organization if repo ownership changes). Then open <https://github.com/aradmanamnaoon/constellation/actions> and rerun Validate and Acceptance on the exact source commit. Inspect actual runner ID, executed steps and job exit status; a green badge without a runner is not acceptance. If restrictions persist after billing is current, contact GitHub Support with run IDs `41390556316`, `41390556318` and the quoted annotation. **Do not provide payment credentials or personal keys to the coding agent.**

## P0 — native macOS ARM64/Intel test environment

Available connected host is Windows x64; no proven running Mac VM or Xcode. A Linux VM/WSL cannot build signed native macOS DMGs or exercise Cocoa, Gatekeeper and Keychain. The existing workflow provides `macos-15` (ARM64) and `macos-15-intel` runners, but they are currently blocked by the above account issue.

**Owner action:** Fix Actions first, then run the existing explicit macOS acceptance workflow and inspect both DMGs, architecture and exact source SHA. For the requested **local/native GUI acceptance**, provide an authorized Mac with Xcode and both architectures (or separate real Intel/ARM64 machines or hosted native test sessions) and a disposable test user, never a production profile. For distribution with trusted signing/notarization, provision Apple Developer ID and notarization credentials in protected GitHub secrets or the Mac keychain; do not send them in chat or logs. Unsigned DMGs may be clearly labeled as such, but never described as notarized/installed/tested without evidence.

## P0 — Android native security parity and signed, installed production APK

The Android x86_64 diagnostic APK is **debug-signed and uses an older Rust JNI library**. Its provenance is not the current UX branch. Android native two-stage file approval is currently unsupported; the credential vault and authenticated checkpoint/rollback anchor also fail closed on Android. These require actual Android Activity/Keystore implementations and model/file denial/restart tests; switching an `UnsupportedPlatform` boolean to success would be unsafe.

**Owner action:** Make a disposable Android 64-bit device or emulator available for test, with sufficient free host memory (the existing safety gate requires at least 4 GiB free before launching an AVD). Provide a production signing keystore through an authorized local environment or protected CI secrets, not messages. Native Android consent, hardware-backed-or-device-protected key storage, deterministic replay/denial/crash tests, fresh source-pinned ARM64 APK/AAB, install/restart and permission-negative QA must be implemented and observed before Android release publication. Signing credentials/device access are owner prerequisites; **writing the missing Android security code remains developer work**.

## P1 — safe Windows install, upgrade, uninstall

NSIS `INSTALLMODE=currentUser` uses a fixed HKCU Constellation product identity. The current host's HKCU already contains an earlier QA Constellation registration. Even `/D=<new temporary folder>` does **not** isolate Windows SID, uninstall key, shortcuts or user app data. The NSIS upgrade path can execute the previous uninstaller; app-data removal may be recursive on explicit user choice. The tested hidden desktop isolates GUI input, not the user's registry hive.

**Owner action:** Supply a disposable, separately identified Windows VM (preferred) with an isolated user SID/HKCU hive, no host profile/registry mounts, and an authenticated older-version installer for a *real upgrade* test. Then test initial install, version upgrade, retained synthetic data, uninstall without data deletion, uninstall with explicit deletion (on a fresh snapshot), reboot/restart and shortcuts, collecting native UI and SHA evidence. Do not run the shared-user uninstall or installer in the normal account. The new static/disposable lifecycle scripts are guards, not a substitute for this test. A trusted Authenticode signing certificate is additionally needed if the release must be Windows signed; no code-signing EKU certificate was found in inspected stores.

## Release acceptance and ownership

The UX implementation branch and draft PR remain the appropriate destination until mandatory acceptance is real. Do not merge or announce a four-platform release solely because local Windows/Linux tests pass. A complete release requires current-source Windows x64, Linux amd64, macOS arm64 + x64, and Android arm64 installers, SHA-256 and immutable commit provenance, correct version/metadata, native install/launch/restart tests, security-negative results, actual successful hosted jobs, and accurately stated signature/notarization status. Keep every owner action and blocker visible in the 12-step plan and the draft PR.
