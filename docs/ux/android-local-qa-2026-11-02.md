# Constellation Android: isolated local build diagnostics

**Date:** 2026-11-02

**Source branch:** `ux/competitor-rebuild-20261101`

**Pushed initial UX commit:** `6d9143ceebf71c595bda41e330506e398e6aacbb`

## Scope and provenance

This is a diagnostic **debug** APK exercise, not Android release acceptance. It is built from a copied Gradle project in ignored `target-android/qa-production-mirror-20261101/project`, with an existing x86_64 Rust JNI library compiled earlier in this UX worktree. Therefore, neither the executable's exact source commit nor the latest frontend payload can be attributed solely to the pushed UX commit. A complete production build must regenerate Rust/native and frontend assets together from one pinned commit.

The original scaffold remains pinned to Android SDK compile/target 37, AGP 9.3.1, Gradle 9.6.1, application ID `app.constellation.studio`, and Kotlin Gradle plugin 2.4.21. No SDK/API downgrade was made to obtain the diagnostic.

## Dependency retrieval and reproducible diagnostic

On this Windows host, `dl.google.com` served HTTP 404 for the AGP 9.3.1 Maven POM. An HTTPS Google repository mirror at `https://maven.aliyun.com/repository/google` returned HTTP 200 with a POM identifying `com.android.tools.build:gradle:9.3.1`. The QA-only Gradle init script also uses the corresponding Maven Central mirror. This allowed the source-compatible Gradle project to configure and complete `:app:assembleX86_64Debug`, even though Google Maven access remains broken from this host.

The mirror and Gradle user home are strictly confined to the ignored QA directory. Mirroring is a **dependency supply-chain exception**; it must not silently become the default production repository. Before any real production release, acquire dependencies from trustworthy upstream endpoints or independently pin and verify the downloaded artifact hashes and provenance.

An initial Kotlin compiler daemon attempt failed (`Daemon compilation failed: null`); a subsequent invocation of Gradle with `-Pkotlin.compiler.execution.strategy=in-process` reported **BUILD SUCCESSFUL**, 62 tasks up-to-date. The diagnostic used the precompiled Rust shared object, so this success does **not** demonstrate a fresh Rust Android build from the current UI source.

## Debug APK metadata

- QA path: `target-android/qa-production-mirror-20261101/project/app/build/outputs/apk/x86_64/debug/app-x86_64-debug.apk`
- Size: **211,902,708 bytes**
- SHA-256: `77849DD1B137E15C76C10C0AE8CA99D36169278007D91FFA9FF2470E462EAEF0`
- Package: `app.constellation.studio`
- Version name/code: `0.1.2` / `1002`
- Compile/target SDK: `37` / `37`; minimum SDK: `24`
- Native ABI: `x86_64`
- Status: **development debug package; not an authenticated signed release; never upload as a production asset**

## Outstanding acceptance

Emulator boot, installation, actual UI interaction, startup/restart, native consent and key-store parity, and signed release-package verification must each be recorded separately. The earlier isolated AVD attempt failed to register `emulator-5580` in ADB despite WHPX being operational. The subsequent QA attempt used a separately generated ADB key and private ADB server port `5068`, while preserving the user's normal Android Studio AVDs and default ADB server. The private server started, the isolated AVD was created, and the diagnostic APK's **v2 debug signature verified**; nevertheless, no `emulator-5580` device registered. Its headless QEMU process consumed about 3.6 GiB RAM while the host had very low free memory, so it and the orphaned old Constellation QA QEMU process were stopped using their precisely matched, project-owned AVD command lines. The automated QA runner then exited nonzero, and **installation and launch were not tested**. Neither the older nor newer experiment modifies a user's existing `Pixel_6` AVD.

Release gating also requires fresh native assets, a verified signer, platform security/permission review, end-to-end app flows, and reproducible package/source mapping. The Windows/Linux bundles cannot be used to assert Android parity.
