# Windows NSIS payload provenance. Never launches the desktop application.
# Defaults to read-only; optional isolated installation fails closed if any
# Constellation registration exists or the NSIS script may install prerequisites.
#
# powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/windows-bundle-identity.ps1 `
#   -InstallerPath <setup.exe> -DirectExePath <constellation-desktop.exe> `
#   -InstalledExePath <previously-isolated-install/constellation-desktop.exe> `
#   -NsisScriptPath <generated/installer.nsi>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$InstallerPath,
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$DirectExePath,
  [string]$InstalledExePath,
  [string]$NsisScriptPath,
  [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedInstallerSha256,
  [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedDirectSha256,
  [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedInstalledSha256,
  [switch]$AllowIsolatedSilentInstall,
  [ValidateRange(5, 120)][int]$TimeoutSeconds = 45,
  [string]$EvidencePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Windows is required.' }
if ($AllowIsolatedSilentInstall -and $InstalledExePath) {
  throw 'Specify an existing installed EXE OR an opt-in isolated installation, not both.'
}

function Get-Identity([string]$Path, [string]$Role) {
  $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
  $file = Get-Item -LiteralPath $resolved -ErrorAction Stop
  if ($file.PSIsContainer -or $file.Extension -ine '.exe') {
    throw "$Role must be an existing regular .exe file."
  }
  if (($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "$Role must not be a symlink or reparse point."
  }
  $bytes = $file.Length
  $stamp = $file.LastWriteTimeUtc.ToString('o')
  $digest = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolved).Hash.ToUpperInvariant()
  $after = Get-Item -LiteralPath $resolved -ErrorAction Stop
  if ($after.Length -ne $bytes -or $after.LastWriteTimeUtc.ToString('o') -ne $stamp) {
    throw "$Role changed during hashing; refusing mutable evidence."
  }
  return [ordered]@{
    role = $Role; path = $resolved; bytes = $bytes; modified_utc = $stamp; sha256 = $digest
  }
}

function Assert-Unchanged($Identity) {
  $now = Get-Identity $Identity.path $Identity.role
  if ($now.sha256 -ne $Identity.sha256 -or
      $now.bytes -ne $Identity.bytes -or
      $now.modified_utc -ne $Identity.modified_utc) {
    throw "$($Identity.role) changed during verification; do not attribute another payload."
  }
}

function Get-NsisEvidence([string]$Path, [string]$DirectPath) {
  $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
  if ([IO.Path]::GetExtension($resolved) -ine '.nsi') { throw 'Expected an NSIS .nsi script.' }
  $content = [IO.File]::ReadAllText($resolved)
  $source = [regex]::Match($content, '(?m)^\s*!define\s+MAINBINARYSRCPATH\s+"([^"]+)"\s*$')
  $file = [regex]::IsMatch($content, '(?m)^\s*File\s+"\$\{MAINBINARYSRCPATH\}"\s*$')
  $mode = [regex]::Match($content, '(?m)^\s*!define\s+INSTALLMODE\s+"([^"]+)"\s*$')
  $webview = [regex]::Match($content, '(?m)^\s*!define\s+INSTALLWEBVIEW2MODE\s+"([^"]*)"\s*$')
  if (-not $source.Success -or -not $file -or -not $mode.Success -or -not $webview.Success) {
    throw 'Generated NSIS script does not expose expected direct binary copy and install mode.'
  }
  # Tauri 2 emits an EMPTY INSTALLWEBVIEW2MODE for the supported `skip`
  # configuration. It emits a literal mode for every installer that can fetch
  # or launch WebView2 prerequisites. Treat empty as a safe skip ONLY if the
  # exact generated preprocessing guards and unconditional branch around the
  # three installer code paths are present. A malformed/custom NSIS script
  # still fails closed; this never changes or executes the installer itself.
  $webviewMode = $webview.Groups[1].Value
  if ($webviewMode -ceq '') {
    $guardedModes = @('downloadBootstrapper','embedBootstrapper','offlineInstaller')
    foreach ($guarded in $guardedModes) {
      $escapedGuard = [regex]::Escape($guarded)
      $pattern = '(?m)^\s*!if\s+"\$\{INSTALLWEBVIEW2MODE\}"\s+==\s+"' + $escapedGuard + '"\s*$'
      if (-not [regex]::IsMatch($content, $pattern)) {
        throw "Tauri skip mode lacks its mandatory WebView2 $guarded compile-time guard."
      }
    }
    if (-not [regex]::IsMatch($content, '(?m)^\s*!define\s+MINIMUMWEBVIEW2VERSION\s+""\s*$') -or
        -not [regex]::IsMatch($content, '(?m)^\s*Goto\s+webview2_done\s*$')) {
      throw 'Tauri skip mode unexpectedly permits an unguarded WebView2 installer path.'
    }
    $webviewMode = 'skip'
  }
  $declared = [IO.Path]::GetFullPath($source.Groups[1].Value)
  $sameSource = [string]::Equals($declared, $DirectPath, [StringComparison]::OrdinalIgnoreCase)
  $info = Get-Item -LiteralPath $resolved
  return [ordered]@{
    path = $resolved
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolved).Hash.ToUpperInvariant()
    bytes = $info.Length
    modified_utc = $info.LastWriteTimeUtc.ToString('o')
    declared_binary_source = $declared
    source_matches_direct_path = $sameSource
    install_mode = $mode.Groups[1].Value
    webview_mode = $webviewMode
    installer_writes_registry = [regex]::IsMatch($content, 'WriteRegStr\s+SHCTX\s+"\$\{UNINSTKEY\}"')
    installer_can_uninstall_previous = [regex]::IsMatch($content, 'Call\s+FindWixInstall') -and [regex]::IsMatch($content, 'ExecWait')
  }
}

function Assert-NoExistingInstallation {
  # NSIS /D= changes the destination but NOT product/registry identity.
  # A pre-existing record could trigger upgrade/uninstall of the user's app.
  $paths = @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Constellation',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Constellation',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Constellation',
    'HKCU:\Software\constellation\Constellation',
    'HKLM:\Software\constellation\Constellation'
  )
  foreach ($path in $paths) {
    if (Test-Path -LiteralPath $path) {
      throw "REFUSED isolated install: existing product registry identity at $path. Read-only verification is still safe."
    }
  }
}

function Install-Isolated($Installer, $Nsis, [int]$Deadline) {
  if ($null -eq $Nsis) { throw 'Isolated installation requires a generated NSIS script for safety review.' }
  if ($Nsis.install_mode -ne 'currentUser') { throw 'Refusing privileged/per-machine NSIS installation.' }
  Assert-NoExistingInstallation
  if ($Nsis.webview_mode -ne 'skip') {
    throw "Refusing installer prerequisite side effects: WebView2 mode '$($Nsis.webview_mode)' is not skip."
  }
  # This destination is created exclusively and cannot overlap any user path.
  $root = Join-Path ([IO.Path]::GetTempPath()) ('constellation-identity-' + [guid]::NewGuid().ToString('N'))
  if ($root -match '\s') { throw 'NSIS /D= requires an unquoted final argument; refusing whitespace in isolated path.' }
  if (Test-Path -LiteralPath $root) { throw 'Refusing an existing isolated installation directory.' }
  # IMPORTANT: /D must be the final NSIS argument, unquoted, even with /S.
  # Launch the installer only inside a NEW private non-input Win32 desktop.
  # The child has a kill-on-close Job Object and never switches desktops.
  # The bootstrap itself checks that the chosen target does not exist.
  $helper = Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1'
  $powershell = Join-Path $PSHOME 'powershell.exe'
  if (-not (Test-Path -LiteralPath $helper -PathType Leaf) -or
      -not (Test-Path -LiteralPath $powershell -PathType Leaf)) {
    throw 'Hidden-desktop installer helper is unavailable; refusing setup.'
  }
  & $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $helper -InstallNsisPath $Installer.path -InstallTarget $root -InstallTimeoutSeconds $Deadline
  if ($LASTEXITCODE -ne 0) {
    throw "Isolated hidden-desktop NSIS verification failed (exit=$LASTEXITCODE)."
  }
  $exe = Join-Path $root 'constellation-desktop.exe'
  if (-not (Test-Path -LiteralPath $exe)) {
    throw 'Isolated installer exited without creating its expected executable.'
  }
  # Deliberately do not run uninstall.exe: even an isolated NSIS uninstaller
  # may mutate global user registry records. Caller can review the QA folder.
  return $exe
}

$issues = New-Object 'System.Collections.Generic.List[string]'
$notes = New-Object 'System.Collections.Generic.List[string]'
$installer = Get-Identity $InstallerPath 'nsis_installer'
$direct = Get-Identity $DirectExePath 'direct_release_exe'
$nsis = $null
if ($NsisScriptPath) {
  $nsis = Get-NsisEvidence $NsisScriptPath $direct.path
  if (-not $nsis.source_matches_direct_path) {
    [void]$issues.Add('NSIS script MAINBINARYSRCPATH does not equal the supplied direct EXE path.')
  }
  if ([datetime]$direct.modified_utc -gt [datetime]$installer.modified_utc) {
    [void]$notes.Add('Direct release EXE has a later modification timestamp than the setup: possible post-bundle overwrite; this does not prove payload identity.')
  }
  if ([datetime]$nsis.modified_utc -gt [datetime]$installer.modified_utc) {
    [void]$issues.Add('NSIS script was modified after setup and cannot be treated as its build-time script.')
  }
}
if ($ExpectedInstallerSha256 -and $installer.sha256 -ne $ExpectedInstallerSha256.ToUpperInvariant()) {
  [void]$issues.Add('Installer does not match expected immutable SHA-256.')
}
if ($ExpectedDirectSha256 -and $direct.sha256 -ne $ExpectedDirectSha256.ToUpperInvariant()) {
  [void]$issues.Add('Direct EXE does not match expected immutable SHA-256.')
}

$installed = $null
$attribution = 'not_proven'
if ($AllowIsolatedSilentInstall) {
  if ($issues.Count -gt 0) {
    throw 'Refusing installation because input hashes or generated NSIS provenance already disagree.'
  }
  # Verify the supplied files are stable immediately before causing effects.
  Assert-Unchanged $installer
  Assert-Unchanged $direct
  $InstalledExePath = Install-Isolated $installer $nsis $TimeoutSeconds
  $attribution = 'isolated_install_of_verified_setup'
} elseif ($InstalledExePath) {
  [void]$notes.Add('The supplied installed EXE was hashed read-only; this script cannot prove which installer created it.')
}
if ($InstalledExePath) {
  $installed = Get-Identity $InstalledExePath 'installed_exe'
  if ($ExpectedInstalledSha256 -and $installed.sha256 -ne $ExpectedInstalledSha256.ToUpperInvariant()) {
    [void]$issues.Add('Installed EXE does not match expected immutable SHA-256.')
  }
  if ($installed.sha256 -ne $direct.sha256) {
    [void]$issues.Add('Installed EXE differs from direct release EXE.')
  }
} else {
  [void]$notes.Add('No installed payload was supplied or extracted: bundle-to-binary equality is UNVERIFIED.')
}

Assert-Unchanged $installer
Assert-Unchanged $direct
if ($installed) { Assert-Unchanged $installed }
if ($nsis) {
  $again = Get-NsisEvidence $nsis.path $direct.path
  if ($again.sha256 -ne $nsis.sha256 -or $again.modified_utc -ne $nsis.modified_utc) {
    throw 'Generated NSIS script changed during verification.'
  }
}

$report = [ordered]@{
  schema = 'constellation.windows_bundle_identity.v1'
  observed_utc = [datetime]::UtcNow.ToString('o')
  status = if ($issues.Count -gt 0) { 'MISMATCH_OR_STALE' } else { 'IDENTITY_NOT_PROVEN' }
  installer = $installer
  direct_exe = $direct
  generated_nsis = $nsis
  installed_exe = $installed
  installed_provenance = $attribution
  issues = @($issues)
  notes = @($notes)
}
if ($installed -and $installed.sha256 -eq $direct.sha256 -and $attribution -eq 'isolated_install_of_verified_setup' -and $issues.Count -eq 0) {
  $report.status = 'MATCH_VERIFIED'
}
$json = ConvertTo-Json -InputObject $report -Depth 8
Write-Output $json
if ($EvidencePath) {
  $path = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($EvidencePath)
  $parent = [IO.Path]::GetDirectoryName($path)
  if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
    throw 'Evidence parent must already exist; no implicit directory creation.'
  }
  # Never clobber another worker's report.
  $bytes = (New-Object System.Text.UTF8Encoding($false)).GetBytes($json + [Environment]::NewLine)
  $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
  try {
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush($true)
  } finally { $stream.Dispose() }
}
if ($report.status -ne 'MATCH_VERIFIED') { exit 2 }
