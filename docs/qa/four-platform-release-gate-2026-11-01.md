# Constellation four-platform release gate — 2026-11-01

## Disposition

**No publishable, fully tested four-platform release exists at source
`c516bd2ec3dac9bb55b9d951e129d617bd1d8aeb`.** GitHub release records
were removed at the owner's explicit request: the releases API returned
`[]` after deletion. No replacement release was published. Existing Git
history and the `v0.1.2-windows-linux-maintenance-20261101` tag remain.
The deleted v0.1.2 release's metadata was archived privately at
`dist/verification-only/release-removal-20261101-4platform/`; its preexisting
locally verified Windows/Linux artifact copies and checksums are preserved.
Do not confuse a historical tag, Actions artifact or unsigned diagnostic with
a tested release.

## Platform-specific evidence

| Platform | Current observed result | Release verdict |
| --- | --- | --- |
| Windows x64 | Source `c516bd2` previously passed 130/130 native Rust desktop tests and offscreen native GUI+legacy SQLite checks on the actual NSIS-extracted and standalone EXE. Unsigned NSIS actual installation, upgrade, uninstall and clean host remain unverified. | **Not production accepted** |
| Linux x86_64 | Frozen source `c516bd2` previously produced a versioned DEB and AppImage, byte-proven ELF and rootless `dpkg` tests; three real private Xvfb GUI modes passed. Independent clean distro installations and security signoff remain unverified. | **Not production accepted** |
| macOS Intel + Apple Silicon | Older source `e3f03f9` produced both DMGs in Actions run `41281363774`; neither corresponds to current `c516bd2`, nor had authenticated Apple signing, notarization, installed-app or Gatekeeper testing. Current hosted macOS access is blocked by GitHub billing. | **No current accepted package** |
| Android ARM64 | On this Windows host, the Tauri CLI cross-compiled the native `aarch64-linux-android` Rust library under source `c516bd2`. APK packaging failed at Windows symlink privileges. A diagnostic, exact-byte copy of the compiled library into generated JNI assets allowed a manual Gradle attempt; that attempt failed resolving Google Maven `com.android.tools.build:gradle:9.3.1`. No installable APK or on-device lifecycle/security acceptance was produced. Native Android file consent and OS-backed secrets/checkpoint anchors remain intentionally unsupported. | **No package; release blocked** |

## Exact GitHub Actions failure evidence

The latest source-matched Validate run `41312968855` and Acceptance run
`41312968883` show job failures, but those statuses **do not prove source
test failures**. GitHub Checks API annotations for:

- Acceptance `source-manifest`, check `124867020439`
- Validate `Windows native compile`, check `124867020502`
- Validate `Tool library ... (windows-2025)`, check `124867020399`

all report: *The job was not started because recent account payments have
failed or your spending limit needs to be increased.* Jobs had zero steps
and no assigned runner. Therefore macOS current-source compilation/DMG
packaging **could not run at all** until GitHub billing is resolved.
Repository GitHub Actions secret-name listing returned `[]`: no configured
Apple Developer signing or Android release-signing secrets were observed.
Never fabricate a CI PASS or notarization/signer receipt.

## Android diagnostic commands and measured failures

Tools actually detected: Android Studio JDK `javac 25.0.3`, Android SDK
platform `android-37.0`, Build Tools `36.0.0`, NDK
`28.2.13676358`, installed Rust `aarch64-linux-android`, Gradle wrapper
`9.6.1`. Local emulator `Pixel_6` uses **x86_64** Android API 34, so
it cannot establish execution of the ARM64 build. The standalone
`tests/android-native-project-contract-20261101.test.mjs` and Linux version
provenance checks passed **7/7 combined**; source manifest passed:
`CI_SOURCE_GATE_PASS required=77 tracked=413 source_files=226`.
Those are static tests, **not mobile package/runtime acceptance**.

The native library compiled at
`apps/desktop/src-tauri/target/aarch64-linux-android/debug/libdesktop_lib.so`
is 203,223,024 bytes, SHA-256:
`cf825b02e46f18c5303fb4c52d02fbbd093babf6a8bb926bda7e4d8350b86978`.
The exact bytes were copied into generated
`gen/android/app/src/main/jniLibs/arm64-v8a/libdesktop_lib.so` because
Tauri failed at the symlink stage with **Creation symbolic link is not
allowed for this system**. This was diagnostic packaging, not a production
source or security-policy change.

Manual Gradle command `:app:assembleArm64Debug -x :app:rustBuildArm64Debug`
downloaded Gradle 9.6.1, but `:buildSrc:compileKotlin` failed because
`com.android.tools.build:gradle:9.3.1` could not be resolved. An independent
`curl -I` of the configured Google Maven POM URL returned HTTP 404 from
this host. Android's official AGP 9.3 release notes describe 9.3.1 as an
actual release, so do not infer invalid version from this failure alone.
Investigate actual Google Maven accessibility/mirror, required repository
configuration, and dependency verification on a controlled Android build
host. Do not bypass dependency checks or fetch arbitrary unsigned artifacts.

## Required before any new public four-platform release

1. Fix account billing/spending limits to enable actual hosted Mac Intel and
   Apple Silicon jobs. Build *both* DMGs from a single immutable final source
   SHA. Configure authenticated Apple Developer ID certificates, hardened
   runtime/notarization and verify Gatekeeper + installed-app GUI.
2. Resolve Android SDK/Gradle dependency access and symlink constraints on a
   controlled builder, without elevation as a routine workaround. Implement
   native trusted Android consent, Android Keystore-backed secrets and
   checkpoint/denial journal anchors. Build release-signed ARM64 APK/AAB,
   then device-test install, launch, permission denial, quit/restart, data
   migration, replay, storage, crash recovery, cancellation and upgrade.
3. Repeat source-pinned Windows and Linux installed-package tests from the
   same final commit; verify real installer upgrade/uninstall, clean Linux
   distro dependencies, signing and integrity.
4. Only publish release assets with accurate build SHA-256, signer identity,
   package architecture, genuine observed tests and explicit remaining gaps.
   Never label a platform tested based solely on compilation or conceal
   unverified device/installation behavior.

**Final gate: BLOCKED.** Existing releases were removed as requested,
but no new release was published because it would misrepresent missing
current-source macOS and Android testing and signing.
