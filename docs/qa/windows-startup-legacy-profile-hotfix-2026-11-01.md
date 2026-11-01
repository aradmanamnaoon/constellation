# Constellation v0.1.1 — Windows startup crash hotfix

**Scope:** Windows 11 x64, source and private-desktop checks on 2026-11-01.
**Disposition:** Legacy startup regression fixed and exact NSIS payload tested.
Authenticode signing and real NSIS installation, upgrade and uninstall acceptance
are **NOT VERIFIED**. Do not present as a production release.

## Observed crash and established cause

The Windows Application Error log reported repeated constellation-desktop.exe
startup crashes with exception code 0xc0000409. Release panic mode was abort,
and the Tauri setup error was propagated to a fatal expect at startup.

Read-only inspection of the existing application database showed user_version
0, SQLite quick_check ok, six historic Constellation tables and one index.
The seven schema-object SQL definitions exactly matched the original shipped
source at Git revision b229c5f. The newer strict schema comparison refused
all six tables because of formatting-only indentation differences, despite
their original definitions being unchanged. No private row data was read or
modified to establish this mismatch.

## Changes

- Explicitly allowlist exact historical SQL DDL, including the four known
  combinations of optional schema extensions. Unknown columns, triggers and
  constraints remain rejected rather than automatically normalized or repaired.
- Atomically migrate the historically unversioned database to user_version 1,
  preserving the original table DDL, rows, events and provenance; repeated
  reopening of the same schema remains accepted.
- Report Tauri startup errors through an OS-native Windows error dialog and
  controlled nonzero exit rather than unhandled panic-abort.
- Bump Windows desktop application and bundle version to 0.1.1.

## Executed checks

| Check | Result |
| --- | --- |
| Original DDL versus existing unmodified user database | 7/7 exact schema objects match |
| Core Rust tests | PASS 5/5 including legacy migration, data retention, second open and malicious schema rejection |
| Desktop Rust native tests | PASS 130/130 |
| Cargo check and strict Clippy | PASS, warnings denied |
| Svelte check and frontend build | PASS, zero check errors or warnings |
| Source completeness gate | PASS required=77 tracked=410 source_files=225 |
| Native direct EXE, fresh synthetic profile on private Windows desktop | PASS |
| Native direct EXE, original unversioned synthetic DB reopen | PASS; version 0 to 1; integrity ok |
| Real NSIS-extracted EXE, synthetic legacy profile reopen | PASS; fresh launch then reopen |
| Real NSIS-extracted EXE, native UI Automation smoke | PASS, 60 descendants, eight expected controls, 25 sample colors, process closed |
| Independent offline NSIS payload comparison | PASS; only Tauri bundle marker UNK to NSS differs in 3 bytes |
| Windows Authenticode signing | NotSigned |
| Actual installer lifecycle (install/upgrade/uninstall) | NOT TESTED; existing HKCU Constellation registration preserved |

## Exact local v0.1.1 artifact identity

- Unsigned NSIS setup: 3,802,212 bytes, SHA-256
  BEC59EB5FEA2C5FA4028E6BA0804FCBD5ED31CA18F243BDAD56AB5EEA93F5A8D
- Direct release executable: SHA-256
  79998F30324B70DCEA84E19A4BF67B14129012BE183FA9045EF8CC91E0969FDB
- Actual extracted NSIS executable: SHA-256
  747A8E3C2B3DD0F997CA2A9AB22D14EBB4447ACD45A7EBE8448EB2F8D175EED5
- Only byte difference was the expected 3-byte Tauri NSIS marker at offset
  9378607, direct UNK becoming bundled NSS.

The offscreen native regression test creates its own marked temporary QA
profile, replaces only that disposable test core database with exact original
DDL, and launches the genuine packaged executable on a non-input Win32 desktop.
No real application profile was opened or reset. Installer execution and
in-place upgrade were not attempted because the registered existing installation
and user files must remain intact.

**Release limits:** unsigned Windows diagnostic hotfix only. Potential
SmartScreen warnings and installer prerequisite side effects remain. Real
installed-upgrade acceptance, clean-device signing and full Linux, macOS and
Android production security acceptance remain pending. Do not overwrite an
existing installation without a verified backup and an independently accepted
upgrade procedure.
