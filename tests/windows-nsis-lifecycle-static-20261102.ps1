# Read-only Constellation v0.1.2 NSIS upgrade/uninstall risk and provenance gate.
# IMPORTANT: This script NEVER runs an installer, an uninstaller or the app;
# it does not read or modify any user registry hive, profile or installation.
# A non-input desktop is NOT a different Windows user/SID or registry hive.
#
# powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \
#   tests/windows-nsis-lifecycle-static-20261102.ps1 -SelfTest
[CmdletBinding()]
param(
  [string]$NsisScriptPath,
  [string]$InstallerPath,
  [string]$DirectExePath,
  [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Windows is required for this NSIS artifact review.' }

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).ProviderPath
if (-not $NsisScriptPath) {
  $NsisScriptPath = Join-Path $repoRoot 'apps/desktop/src-tauri/target/release/nsis/x64/installer.nsi'
}
if (-not $InstallerPath) {
  $InstallerPath = Join-Path $repoRoot 'apps/desktop/src-tauri/target/release/bundle/nsis/Constellation_0.1.2_x64-setup.exe'
}
if (-not $DirectExePath) {
  $DirectExePath = Join-Path $repoRoot 'apps/desktop/src-tauri/target/release/constellation-desktop.exe'
}

function Get-ReadOnlyIdentity([string]$Path, [string]$Extension) {
  $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
  $before = Get-Item -LiteralPath $resolved -ErrorAction Stop
  if ($before.PSIsContainer -or $before.Extension -ine $Extension -or
      (($before.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
    throw "Expected a regular, non-reparse $Extension input file."
  }
  $sha = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToUpperInvariant()
  $after = Get-Item -LiteralPath $resolved -ErrorAction Stop
  if ($before.Length -ne $after.Length -or $before.LastWriteTimeUtc -ne $after.LastWriteTimeUtc) {
    throw "Input changed during read-only hashing: $resolved"
  }
  return [ordered]@{
    path = $resolved
    bytes = $before.Length
    modified_utc = $before.LastWriteTimeUtc.ToString('o')
    sha256 = $sha
  }
}

function Get-NsisDefine([string]$Script, [string]$Key) {
  $pattern = '(?m)^\s*!define\s+' + [regex]::Escape($Key) + '\s+"([^"\r\n]*)"\s*$'
  $match = [regex]::Match($Script, $pattern)
  if (-not $match.Success) { throw "Missing generated NSIS define: $Key" }
  return $match.Groups[1].Value
}

function Inspect-NsisContract([string]$Script) {
  $installMode = Get-NsisDefine $Script 'INSTALLMODE'
  $product = Get-NsisDefine $Script 'PRODUCTNAME'
  $version = Get-NsisDefine $Script 'VERSION'
  $webview2 = Get-NsisDefine $Script 'INSTALLWEBVIEW2MODE'
  $uninstallKey = Get-NsisDefine $Script 'UNINSTKEY'
  $binarySource = Get-NsisDefine $Script 'MAINBINARYSRCPATH'
  $bundleId = Get-NsisDefine $Script 'BUNDLEID'

  $risk = [ordered]@{
    shared_current_user_product_identity = ($installMode -ceq 'currentUser' -and
      $product -ceq 'Constellation' -and $bundleId -ceq 'app.constellation.studio' -and
      $uninstallKey -ceq 'Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}')
    reads_existing_product_registration = (
      $Script.Contains('ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"') -and
      $Script.Contains('ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""'))
    can_execute_previous_uninstaller = (
      $Script.Contains('Function PageLeaveReinstall') -and
      $Script.Contains('Call FindWixInstall') -and
      $Script.Contains('ExecWait ''$R1'''))
    overwrites_shared_registration = (
      $Script.Contains('WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString"') -and
      $Script.Contains('WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR'))
    uninstaller_deletes_shared_registry_key = $Script.Contains('DeleteRegKey HKCU "${UNINSTKEY}"')
    uninstaller_can_remove_user_app_data = (
      $Script.Contains('RmDir /r "$APPDATA\${BUNDLEID}"') -and
      $Script.Contains('RmDir /r "$LOCALAPPDATA\${BUNDLEID}"'))
    silent_installer_creates_user_shortcuts = (
      $Script.Contains('Call CreateOrUpdateDesktopShortcut') -and
      $Script.Contains('Call CreateOrUpdateStartMenuShortcut'))
    installer_can_download_webview2 = ($webview2 -ceq 'downloadBootstrapper')
  }

  return [ordered]@{
    product = $product
    version = $version
    install_mode = $installMode
    bundle_id = $bundleId
    webview2_mode = $webview2
    uninstall_key_template = $uninstallKey
    declared_binary_source = $binarySource
    risk = $risk
  }
}

if ($SelfTest) {
  # Pure in-memory parser regression: no fixture files, registry or processes.
  $synthetic = @'
!define INSTALLMODE "currentUser"
!define PRODUCTNAME "Constellation"
!define VERSION "0.1.2"
!define BUNDLEID "app.constellation.studio"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define INSTALLWEBVIEW2MODE "downloadBootstrapper"
!define MAINBINARYSRCPATH "C:\isolated-build\constellation-desktop.exe"
ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
Function PageLeaveReinstall
Call FindWixInstall
ExecWait '$R1'
WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString"
WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR
DeleteRegKey HKCU "${UNINSTKEY}"
RmDir /r "$APPDATA\${BUNDLEID}"
RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
Call CreateOrUpdateDesktopShortcut
Call CreateOrUpdateStartMenuShortcut
'@
  $parsed = Inspect-NsisContract $synthetic
  foreach ($name in $parsed.risk.Keys) {
    if (-not $parsed.risk[$name]) { throw "Synthetic NSIS positive regression failed: $name" }
  }
  $negative = Inspect-NsisContract ($synthetic.Replace('!define INSTALLMODE "currentUser"', '!define INSTALLMODE "perMachine"'))
  if ($negative.risk.shared_current_user_product_identity) {
    throw 'Synthetic NSIS negative regression failed: wrong install mode accepted.'
  }
  $withoutPrevious = Inspect-NsisContract ($synthetic.Replace('ExecWait ''$R1''', 'NO_UNINSTALL_CALL'))
  if ($withoutPrevious.risk.can_execute_previous_uninstaller) {
    throw 'Synthetic NSIS negative regression failed: prior-uninstall call not detected as missing.'
  }
  $withoutDataRemoval = Inspect-NsisContract ($synthetic.Replace('RmDir /r "$APPDATA\${BUNDLEID}"', 'NO_APPDATA_DELETE'))
  if ($withoutDataRemoval.risk.uninstaller_can_remove_user_app_data) {
    throw 'Synthetic NSIS negative regression failed: app-data removal not detected as missing.'
  }
  Write-Output 'STATIC_SELFTEST_PASS in-memory positive/negative NSIS contract checks; no installer or user hive accessed'
}

$nsi = Get-ReadOnlyIdentity $NsisScriptPath '.nsi'
$installer = Get-ReadOnlyIdentity $InstallerPath '.exe'
$direct = Get-ReadOnlyIdentity $DirectExePath '.exe'
$content = [IO.File]::ReadAllText($nsi.path)
$contract = Inspect-NsisContract $content
$sourceMatches = [string]::Equals(
  [IO.Path]::GetFullPath($contract.declared_binary_source),
  $direct.path,
  [StringComparison]::OrdinalIgnoreCase
)
$sourcePredatesBundle = [datetime]::Parse($nsi.modified_utc) -le [datetime]::Parse($installer.modified_utc)
$allRiskPathsRecognized = $true
foreach ($name in $contract.risk.Keys) {
  if (-not $contract.risk[$name]) { $allRiskPathsRecognized = $false }
}

# A concurrent release build can rewrite these paths. Rehash after the full
# inspection rather than attributing mixed generations of installer evidence.
foreach ($original in @($nsi, $installer, $direct)) {
  $extension = [IO.Path]::GetExtension($original.path)
  $again = Get-ReadOnlyIdentity $original.path $extension
  if ($original.sha256 -cne $again.sha256 -or $original.bytes -ne $again.bytes -or
      $original.modified_utc -cne $again.modified_utc) {
    throw 'Release input changed during the read-only NSIS lifecycle inspection; retry only after build settles.'
  }
}

$report = [ordered]@{
  result = if ($sourceMatches -and $sourcePredatesBundle -and
               $contract.version -ceq '0.1.2' -and $allRiskPathsRecognized) {
    'STATIC_AUDIT_COMPLETE_LIVE_LIFECYCLE_BLOCKED'
  } else { 'STATIC_PROVENANCE_OR_CONTRACT_MISMATCH_LIVE_LIFECYCLE_BLOCKED' }
  installed_upgrade_tested = $false
  installed_uninstall_tested = $false
  current_user_registry_or_profile_accessed = $false
  installer_or_uninstaller_executed = $false
  private_desktop_is_private_user_hive = $false
  requires_separate_disposable_windows_sid_and_hive = $true
  risk_basis = 'NSIS /D= redirects only filesystem destination; HKCU product registration, previous-uninstaller actions, shortcuts and optional app-data deletion remain scoped to the process Windows SID.'
  generated_script = $nsi
  installer_exe = $installer
  direct_exe = $direct
  source_matches_direct_exe_path = $sourceMatches
  nsi_modified_no_later_than_installer = $sourcePredatesBundle
  contract = $contract
  notes = @(
    'This static contract is not cryptographic proof that this installer EXE was compiled from the inspected .nsi.',
    'Do not call tests/helper-hidden-desktop.ps1 -InstallNsisPath with the active Windows user: its private desktop does not change HKCU.',
    'No genuinely isolated Windows account/token/hive was established; neither upgrade nor uninstall was run.',
    'An authorized disposable VM or separate Windows SID with an independently loaded user hive and isolated application data is required for live NSIS lifecycle QA.'
  )
}
$report | ConvertTo-Json -Depth 12
if ($report.result -ne 'STATIC_AUDIT_COMPLETE_LIVE_LIFECYCLE_BLOCKED') { exit 2 }
