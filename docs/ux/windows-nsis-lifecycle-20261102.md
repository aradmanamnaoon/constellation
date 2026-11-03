# Constellation Windows NSIS upgrade/uninstall — isolation gate

**2026-11-02 · Independent scoped QA · Outcome: LIVE LIFECYCLE BLOCKED, static audit PASSED**

This is an honest acceptance blocker, **not** a completed upgrade/uninstall test.
The repository's current generated NSIS installer is designed for the same
Windows-user product identity as an existing Constellation installation on this
machine. Installing into a different temporary directory **does not isolate
HKCU**, Windows shortcuts or the previous-uninstaller command.

## Observed build and source evidence

Inspected without launching any executable:

| Evidence | SHA-256 | Bytes |
| --- | --- | ---: |
| `apps/desktop/src-tauri/target/release/nsis/x64/installer.nsi` | `966B454F8BBD29D21E7F59A656358637EFB271C7ED26B6638853B05866FDD268` | 32,871 |
| `apps/desktop/src-tauri/target/release/bundle/nsis/Constellation_0.1.2_x64-setup.exe` | `EC29919EF2B66F8F4FF8D3EDE46002844994A31DC8CF5FDC23AA9CEFC24541B2` | 3,809,926 |
| `apps/desktop/src-tauri/target/release/constellation-desktop.exe` | `7477A4543054D005B7BF4A7D1173FBA906EBA8BB577D124163CFBA74287580CA` | 10,316,288 |

The table represents the final source-locked UX revision rebuild at **04:39 UTC**,
independently hashed and accepted by a fresh read-only static audit.
Another worker was rebuilding Windows artifacts during the earlier audit:
the earlier setup SHA-256 was `C33BB3491CB9230993C19BFFE0B761E4226F4867A765AEE9836763BC3C95E42C`
and EXE SHA-256 was `15B03E9CBC4FBF2F77E85C9F0F8440C63FEB9868D83A852BCAE7C560D33059C8`.
One intermediate static invocation appropriately returned a contract/provenance
failure while artifacts changed. After rebuild completion, the hardened static
rehash audit returned exit `0`. These hashes pin a **local candidate**, not a
public release or proof of a tested upgrade.

The generated script defines version `0.1.2`, product `Constellation`,
`INSTALLMODE "currentUser"`, and `BUNDLEID "app.constellation.studio"`
(lines 35–63). Its declared `MAINBINARYSRCPATH` equals the inspected
release executable's path; the NSI timestamp precedes the setup timestamp.
These are **provenance indicators, not cryptographic proof** that the setup EXE
was actually compiled from these particular NSI bytes.

### Exact unsafe shared-state actions in the generated script

| Source lines | Installer/uninstaller action | Why a temporary install directory is insufficient |
| --- | --- | --- |
| 61–63, 99–106 | Fixed per-user Constellation product/uninstall identity | A second Constellation instance under the same SID is not an isolated product |
| 195–225, 321–404 | Finds a previous WiX/NSIS install; can `ExecWait` its uninstaller | Upgrade/reinstall may execute the **existing** uninstall command |
| 536–540, 892–896 | Reads the previous user's install location | Explicit `/D=` changes only the destination; other shared state remains |
| 693–728 | Writes `SHCTX` product registry and `UninstallString` | The existing HKCU uninstall record could be overwritten |
| 741–745 | Creates shortcuts even for silent installations | The active user's Desktop/Start Menu can be mutated |
| 846–860 | Deletes shared uninstall key and HKCU Run entry | Uninstall can affect the existing account's Constellation registration |
| 863–879 | Optional recursive removal of `$APPDATA\app.constellation.studio` and `$LOCALAPPDATA\app.constellation.studio` | Personal application data could be deleted |
| 56–60, 580–686 | `INSTALLWEBVIEW2MODE "downloadBootstrapper"` | Installer may download or install an external WebView2 prerequisite |

Read-only HKCU registry **metadata inspection** confirmed an existing
`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Constellation`
registration for version `0.1.2`, pointing to an earlier QA installation
under a temporary directory (`ConstellationUXRelease20261102-v5`). We did not
modify that key, uninstall entry, referenced installation or any user data.
The presence of a prior QA installation does **not** grant ownership to
overwrite or delete it.

### Why the hidden-desktop helper is not sufficient

`tests/helper-hidden-desktop.ps1:109–178` validates a unique non-input
desktop and contains subprocesses with a kill-on-close Job Object. This is
valuable for avoiding visible windows and orphaned test processes.
However, `helper-hidden-desktop.ps1:279–334` launches the same-user NSIS
installer with `/S /D=<temporary path>` and does **not** create a separate
Windows logon SID, independently loaded `HKU` hive or isolated Windows user
profile. Consequently it cannot safely establish real upgrade/uninstall
isolation on the present host.

Existing `tests/windows-bundle-identity.ps1:110–158` also recognizes this
hazard and refuses an opt-in installation if Constellation product registry
keys already exist or WebView2 mode is not `skip`. Its isolated install
verification intentionally does **not** run `uninstall.exe`.

## Safe QA implementation and result

`tests/windows-nsis-lifecycle-static-20261102.ps1` is an independent
read-only contract/provenance gate. It:

- Checks three regular files (NSI, setup, EXE) by SHA-256 and stable metadata,
  then rehashes every input to detect concurrent build replacement;
  parses the generated `!define` values and exact dangerous operations.
- Runs optional in-memory positive/negative synthetic parser tests.
- Emits a structured JSON disposition with
  `installed_upgrade_tested=false` and `installed_uninstall_tested=false`.
- **Never starts** `installer.exe`, `uninstall.exe`, the application, a
  hidden desktop or a VM. It never opens or changes a registry hive or
  personal app-data directory. No credential, network, paid-model or
  provider operation is performed.
- Fails on changed/unrecognized build provenance or unexpected script
  contracts. A successful static audit deliberately retains the
  `LIVE_LIFECYCLE_BLOCKED` status.

Run from the isolated UX checkout:

```powershell
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File tests/windows-nsis-lifecycle-static-20261102.ps1 -SelfTest
```

**Observed:** exit `0`, `STATIC_SELFTEST_PASS` and JSON
`result="STATIC_AUDIT_COMPLETE_LIVE_LIFECYCLE_BLOCKED"`. Eight dangerous
shared-state contract checks were found. This does not claim that the
installer is safe to execute in the current Windows account.

## Actual upgrade/uninstall acceptance: safe prerequisites

An owner-authorized **disposable Windows VM** (preferred) or demonstrably
distinct disposable Windows local account with independently loaded user
hive is required before executing the installer. Do not use a fake
`USERPROFILE`, `APPDATA`, `LOCALAPPDATA` override or a non-input desktop as
a substitute for Windows SID/hive isolation.

When one is provided:

1. Snapshot the disposable guest; prove its Windows SID, profile,
   `HKCU\...\Uninstall\Constellation` and `HKLM` product registration are
   separate from, and cannot affect, the real user. No real profile mount,
   host registry redirection or profile sharing.
2. Pin full SHA-256 hashes of **two real versioned installers** to establish an
   actual upgrade (for example an authenticated older `0.1.1` installer and
   the final `0.1.2` installer). A second install of `0.1.2` is a
   **reinstall**, not evidence of an upgrade.
3. Install the older build only in the disposable guest; create synthetic
   application state and record its exact files/registry. Avoid remote
   providers; preinstall approved WebView2 runtime or explicitly authorize
   and isolate prerequisite networking.
4. Upgrade to `0.1.2` and assert installed binary identity, correct
   version/registration and persistence of the synthetic guest data after
   restart. Confirm no unrelated guest paths or registry keys changed.
5. From a fresh guest snapshot, test uninstall **without** app-data removal;
   assert program files, shortcuts and product registration are removed while
   intended app data persists. In a separate disposable snapshot, explicitly
   test uninstall **with** app-data removal and verify only guest data
   disappears. Test reinstall and failure/rollback paths independently.
6. Capture exact tested SHA, silent/interactive mode, process exit codes,
   timestamps, snapshots, registry diffs, package/file hashes and native
   window/GUI results. Run no install/uninstall against the current real
   Windows SID, even if the install folder is inside `%TEMP%`.

**Current verdict:** This host has no proven separate disposable Windows
user/hive. The independent test provides non-destructive source-level
validation, but full native NSIS upgrade/uninstall acceptance remains
`NOT RUN — UNSAFE WITHOUT SEPARATE WINDOWS ACCOUNT/VM`.
