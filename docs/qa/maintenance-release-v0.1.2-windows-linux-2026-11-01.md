# Constellation v0.1.2 — Windows and Linux maintenance prerelease

**Date:** 2026-11-01. **Status:** source and experimental package checks
passed within the scopes below. **Not production-ready.** This report does
not certify installed Windows upgrade/uninstall, cross-platform consent, code
signing, notarization or complete functional requirements.

## Bugs addressed

1. **Linux package version mismatch:** The offline Debian fallback, AppImage
   builder, rootless dpkg verifier and Xvfb smoke scripts previously hardcoded
   the 0.1.0 package filenames and DEB control version. Packages built against
   newer source would be mislabeled 0.1.0. All four scripts now derive the
   semver from the **frozen Tauri source snapshot**, reject an invalid version,
   and use that exact version in control metadata and filenames. A new Node
   regression test checks the four script contracts and the desktop version
   agreement. The actual 0.1.2 DEB control and AppImage filenames were verified.
2. **Windows HTTP fixture race:** In a previous hosted Windows validation, the
   negative streamed Content-Type test failed intermittently. It could race
   the client immediately rejecting the HTTP headers against the fixture
   writing the deliberately invalid stream body in 17-byte fragments. The
   fixture now sends the header and complete body together for **only invalid
   MIME** tests; the accepted streaming protocols still test fragmented
   network writes. No model transport behavior or fail-closed Content-Type
   validation was changed, and the test still asserts that no untrusted text
   reaches provisional UI. A prior hosted failure's exact cause was not
   independently reproduced (30 pre-change local passes); this is a narrow
   race hardening, not a claim that all possible Windows CI flakiness is fixed.
3. **Installation-independent Windows distribution:** Added a direct portable
   Windows executable to the release staging alongside the unsigned NSIS
   installer. Both were tested as actual Windows processes without modifying
   the user's registered installation. The portable executable still depends
   on the host WebView2 runtime and opens the normal Constellation profile
   unless run with the approved isolated-profile test flag; it is not a
   sandbox or a substitute for a data backup.

The prior **v0.1.1** exact legacy SQLite startup migration fix remains in
the 0.1.2 sources, and its native first-launch/reopen tests were repeated.

## Automated test gates

- Windows desktop Rust tests: **130/130 PASS**.
- Existing JavaScript contract tests: **106/106 PASS**; new version-provenance
  contract: **1/1 PASS**.
- Strict Rust Clippy, rustfmt, desktop Cargo check: **PASS**.
- Svelte typechecking: **0 errors, 0 warnings**; production frontend build
  succeeded as part of Windows NSIS build.
- Invalid native HTTP streaming MIME negative test: **24 additional repeated
  invocations passed** after the fixture change.
- Windows and Ubuntu WSL source-completeness gates passed. WSL crate-wide
  tests, 200 skill integrations and all multi-platform acceptance gates are
  **not claimed** by this report.

## Windows x64 release package identity

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| v0.1.2 unsigned NSIS setup | 3,802,391 | 5ea5a2defeeeeca09608fbc844a213439c03db625ca0d4a433f0102cb5a416e3 |
| v0.1.2 unsigned portable direct executable | 10,312,192 | e932eb3fe5bd6d13da420f205cba784846368819a37af415aff3beedda454d81 |
| executable extracted from NSIS bundle | 10,312,192 | 5f0c6d5e0f1a96670e107d9c3bf78b60c1d43b052407b4800e15f7a86fb1b328 |

Independent 7-Zip extraction established that the NSIS executable and the
direct executable differ at **only the expected 3-byte Tauri bundle marker**,
offset 9378615, `UNK` becoming `NSS`.

Both the bundled and portable executables passed private, non-input Win32
desktop testing: real first launch, a second launch with the exact historic
unversioned SQLite profile (schema version **0 to 1**, integrity **ok**),
and native UI Automation (60 descendants, 8 expected controls, nonblank
window screenshot with 25 sampled colors). Real user app data, existing
installer registration, and visible desktop were untouched.

**Windows Authenticode: NotSigned.** Real NSIS installer installation,
installed-upgrade, uninstall, Windows SmartScreen and WebView2 bootstrapper
behavior were not validated. Do not overwrite an important existing
installation without a verified backup.

## Linux x86_64 release package identity

Frozen-source build: Ubuntu WSL local run
`/home/saba/.cache/constellation-linux-packages/run.4POocHSE`.
Both before-build and after-package strict source hash checks reported
`SOURCE_SHA_BOUND_PASS`.

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `constellation-studio_0.1.2_amd64.deb` | 4,256,386 | b8adfe94f9bf12c35f4b431352f2c96b0c846a4b6f97e52d5501fe44b939fdc4 |
| `Constellation_0.1.2_x86_64.AppImage` | 5,839,352 | 2d9a88646f5f815afec43a60cbe8d8982805b5e59f029135b23b53118ca57384 |
| native Linux ELF embedded in packages | 12,582,152 | db393fc61385446b43f5d4e57dbe0f2c338476366a4da86619265266f968d30e |

DEB control reports version **0.1.2**, architecture `amd64`; extracted
package ELF was byte-identical to the compiled binary. AppImage embedded
ELF, icon and AppRun matched the staged source. Rootless `dpkg --root`
unpacked without modifying host package managers.

Three real private Xvfb sessions succeeded: extracted DEB, AppImage rootless
extract-and-run and native FUSE AppImage. All showed the named app window
and rendered content with measured viewport entropy **0.28195**, above the
0.05 guard. This is **not** clean-machine installation acceptance, broad
distro portability, full accessibility or production security certification.
AppImage still uses system GTK/WebKit2GTK dependencies.

## Open release gates

- Windows production signing, current-source installer upgrade/uninstall on a
  clean isolated Windows machine, installed file-consent and cancellation QA.
- Signed/notarized macOS Intel and Apple Silicon packages and real installed GUI.
- Android production signing, native consent/key vault, on-device lifecycle
  tests. Earlier v0.1.0 Android APK remains DEBUG/UNVERIFIED only.
- Full app master-spec acceptance, complete platform security/skills/source
  ledger, real supported-distro installations and independent release gate.

This release must remain **published as experimental prerelease**, not stable
or production-certified.
