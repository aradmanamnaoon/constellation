# NSIS lifecycle acceptance, restricted to a disposable, independently installed
# Windows VM.  DEFAULT IS READ-ONLY. No installation on the host, ever.
#
# 1. powershell.exe -NoProfile -File .\tests\windows-nsis-disposable-guest-20261102.ps1 -SelfTest
# 2. Inside a clean, non-admin Windows VM account (not the Windows host):
#    powershell.exe -NoProfile -File .\windows-nsis-disposable-guest-20261102.ps1 `
#      -Mode Preflight -HostSid 'S-1-5-21-...-1001' -HostComputerName HOSTNAME
# 3. Only after PASS and snapshot, opt in separately to each phase with
#    -Execute -Mode InstallOld / Upgrade / UninstallPreserve, supplying
#    -QaRoot, both installer paths and both independently pinned SHA-256 hashes.
#
# No host profile or registry is modified by SelfTest, Inspect, or Preflight.
# The execution path requires ALL security gates again in a non-input desktop
# worker. No elevation, no automatic VM launch, no network/provider calls here.
[CmdletBinding()]
param(
  [ValidateSet('Inspect','Preflight','InstallOld','Upgrade','UninstallPreserve')]
  [string]$Mode = 'Inspect',
  [switch]$SelfTest,
  [switch]$Execute,
  [ValidatePattern('^S-1-5-21-(?:\d+-){3}\d+$')][string]$HostSid,
  [string]$HostComputerName,
  [string]$QaRoot,
  [string]$OldInstaller,
  [string]$NewInstaller,
  [string]$NewNsiScript,
  [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$OldSha256,
  [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$NewSha256,
  [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$NewNsiSha256,
  [ValidatePattern('^\d+\.\d+\.\d+$')][string]$OldVersion,
  [ValidateRange(20,180)][int]$TimeoutSeconds = 120,
  [string]$PrivateDesktopWorkerName
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Windows required.' }
$currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$currentName = [Environment]::MachineName
$thisScriptPath = $PSCommandPath
$thisScriptDirectory = $PSScriptRoot

function Get-MachineSid([string]$Sid) {
  if ($Sid -notmatch '^(S-1-5-21-\d+-\d+-\d+)-\d+$') {
    throw 'Expected a real Windows local/domain account SID, not a well-known service SID.'
  }
  return $Matches[1]
}

function Assert-GuestLocalPath([string]$Path) {
  $profile = [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\')
  $resolved = [IO.Path]::GetFullPath($Path).TrimEnd('\')
  if (-not $resolved.StartsWith($profile + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Guest QA input must reside beneath the independently verified guest user profile.'
  }
  $cursor = if (Test-Path -LiteralPath $resolved -PathType Container) { $resolved } else { Split-Path -Parent $resolved }
  while ($cursor -and $cursor.Length -ge $profile.Length) {
    $item = Get-Item -LiteralPath $cursor -ErrorAction Stop
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
      throw 'Guest QA path ancestry contains a symlink, junction or redirected folder.'
    }
    if ($cursor -ieq $profile) { break }
    $cursor = Split-Path -Parent $cursor
  }
}

# The predicate is pure and testable using intentionally synthetic identities.
# An Azure/VM-hosted GitHub runner may be detected, but still needs a known
# DIFFERENT host SID, an explicit account/profile/hive and a clean QA root.
function Test-IdentityBoundary(
  [string]$Manufacturer, [string]$Model, [string]$Sid,
  [string]$HostAccountSid, [string]$Computer, [string]$HostComputer,
  [bool]$HiveLoaded, [bool]$ProfileMatches, [bool]$NonAdmin
) {
  $errors = New-Object 'System.Collections.Generic.List[string]'
  if ([string]::IsNullOrWhiteSpace($HostAccountSid) -or
      [string]::IsNullOrWhiteSpace($HostComputer)) {
    [void]$errors.Add('Host SID and hostname must be supplied from the physical Windows host.')
  }
  $vm = ($Manufacturer -match '^(Microsoft Corporation|VMware, Inc\.|innotek GmbH|Oracle Corporation|QEMU|Red Hat)$') -and
    ($Model -match '(?i)Virtual Machine|VirtualBox|VMware Virtual Platform|Standard PC|KVM|QEMU')
  if (-not $vm) { [void]$errors.Add('The computer hardware is not a recognized disposable Windows VM guest.') }
  if ($Computer -ieq $HostComputer -or [string]::IsNullOrWhiteSpace($Computer)) {
    [void]$errors.Add('Guest computer must have an independent name, not the host name.')
  }
  if ($Sid -ceq $HostAccountSid -or [string]::IsNullOrWhiteSpace($Sid)) {
    [void]$errors.Add('Guest account SID must differ from host account SID.')
  }
  try {
    if ((Get-MachineSid $Sid) -ceq (Get-MachineSid $HostAccountSid)) {
      [void]$errors.Add('Windows machine SID prefix is identical to the host; cloned hive is unsafe.')
    }
  } catch {
    [void]$errors.Add('One or both SIDs are malformed.')
  }
  if (-not $HiveLoaded) { [void]$errors.Add('HKU guest SID hive is not loaded.') }
  if (-not $ProfileMatches) { [void]$errors.Add('Guest profile does not match independently registered SID profile.') }
  if (-not $NonAdmin) { [void]$errors.Add('Use an unprivileged disposable VM-local account (no elevated installer token).') }
  return @($errors)
}

function Get-BoundaryReport {
  $cs = Get-CimInstance Win32_ComputerSystem -ErrorAction Stop
  $localUser = Get-CimInstance Win32_UserAccount -Filter ("SID='" + $currentSid + "'") -ErrorAction Stop |
    Select-Object -First 1
  $profile = Get-CimInstance Win32_UserProfile -Filter ("SID='" + $currentSid + "'") -ErrorAction Stop |
    Select-Object -First 1
  $hive = [Microsoft.Win32.Registry]::Users.OpenSubKey($currentSid)
  $hiveLoaded = $null -ne $hive
  if ($hive) { $hive.Close() }
  $profileMatches = $false
  if ($profile -and $profile.LocalPath -and $env:USERPROFILE) {
    $profileMatches = [string]::Equals(
      [IO.Path]::GetFullPath($profile.LocalPath).TrimEnd('\'),
      [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\'),
      [StringComparison]::OrdinalIgnoreCase)
  }
  $principal = New-Object Security.Principal.WindowsPrincipal ([Security.Principal.WindowsIdentity]::GetCurrent())
  $nonAdmin = -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  $errors = @(Test-IdentityBoundary $cs.Manufacturer $cs.Model $currentSid $HostSid $currentName $HostComputerName $hiveLoaded $profileMatches $nonAdmin)
  if (-not $localUser -or -not $localUser.LocalAccount) {
    $errors += 'Lifecycle execution requires a new VM-local account, not a host/domain identity.'
  }
  if (@(Get-CimInstance Win32_LogicalDisk -ErrorAction Stop | Where-Object {$_.DriveType -eq 4}).Count -gt 0) {
    $errors += 'Mapped network drives found; do not expose host drives to disposable lifecycle VM.'
  }
  if (@(Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Root -match '^\\\\' }).Count -gt 0) {
    $errors += 'UNC filesystem drive found; remove guest host-sharing before lifecycle execution.'
  }
  # The NSIS bundle may contain the WebView2 downloadBootstrapper mode. An
  # off-network guest prevents prerequisite download or other network access.
  if (@(Get-CimInstance Win32_NetworkAdapterConfiguration -Filter 'IPEnabled=True' -ErrorAction Stop).Count -gt 0) {
    $errors += 'At least one guest network adapter is IP-enabled; disconnect ALL guest network adapters for offline NSIS acceptance.'
  }
  return [ordered]@{
    verdict = if ($errors.Count -eq 0) { 'VM_BOUNDARY_PREFLIGHT_PASS_NOT_LIFECYCLE_PASS' } else { 'BLOCKED_NO_PROVEN_DISPOSABLE_WINDOWS_VM' }
    guest_computer = $currentName
    guest_sid = $currentSid
    vm_manufacturer = $cs.Manufacturer
    vm_model = $cs.Model
    guest_hku_hive_loaded = $hiveLoaded
    independent_guest_profile = $profileMatches
    non_admin = $nonAdmin
    issues = @($errors)
    installed_or_uninstalled = $false
  }
}

function Assert-LifecycleRoot([string]$Requested) {
  if (-not $Requested -or -not [IO.Path]::IsPathRooted($Requested)) {
    throw 'A guest-owned absolute -QaRoot is mandatory.'
  }
  $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
  $root = [IO.Path]::GetFullPath($Requested).TrimEnd('\')
  if ($temp -match '\s' -or $root -match '\s') {
    throw 'NSIS /D= cannot safely use a whitespace-containing QA destination.'
  }
  if (-not $root.StartsWith($temp + '\ConstellationNSISDisposable-', [StringComparison]::OrdinalIgnoreCase) -or
      $root -notmatch '(?i)\\ConstellationNSISDisposable-[a-f0-9]{32}$') {
    throw 'QaRoot must be an exclusive GUID-named directory directly beneath guest TEMP.'
  }
  if (-not $temp.StartsWith([IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\') + '\',
      [StringComparison]::OrdinalIgnoreCase)) {
    throw 'TEMP is not beneath the independently verified guest user profile.'
  }
  $cursor = $temp
  while ($cursor -and $cursor.Length -ge 3) {
    if ((Get-Item -LiteralPath $cursor -ErrorAction Stop).Attributes -band [IO.FileAttributes]::ReparsePoint) {
      throw 'Guest TEMP ancestry includes a junction/reparse point; refusing.'
    }
    $parent = Split-Path -Parent $cursor
    if ($parent -eq $cursor) { break }
    $cursor = $parent
  }
  if (Test-Path -LiteralPath $root) {
    $item = Get-Item -LiteralPath $root
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
      throw 'QaRoot is a junction/reparse point.'
    }
  }
  return $root
}

function Assert-PinnedInstaller([string]$Path, [string]$ExpectedHash) {
  if (-not $Path -or -not $ExpectedHash) { throw 'Each installer requires a path and an independently pinned SHA-256.' }
  $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
  $file = Get-Item -LiteralPath $resolved
  if ($file.PSIsContainer -or $file.Extension -ine '.exe' -or
      ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Installer must be a regular local EXE, not a symlink/directory.'
  }
  if ($resolved.StartsWith('\\') -or $resolved -match '^[A-Za-z]:\\Users\\[^\\]+\\(OneDrive|Dropbox)\\') {
    throw 'Installer must be on the disposable VM local disk, not a shared path.'
  }
  Assert-GuestLocalPath $resolved
  $hash = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
  if ($hash -ine $ExpectedHash) { throw 'Installer SHA-256 mismatch: refusing executable.' }
  return $resolved
}

# Read-only generated NSIS contract proof. The script is NOT an embedded
# cryptographic attestation for the EXE, so both files must be independently
# pinned and packaged as immutable guest inputs by the reviewer.
function Assert-NsisContent([string]$Script) {
  $required = @(
    '!define PRODUCTNAME "Constellation"',
    '!define VERSION "0.1.2"',
    '!define INSTALLMODE "currentUser"',
    '!define BUNDLEID "app.constellation.studio"',
    '!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"',
    'WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString"',
    'DeleteRegKey HKCU "${UNINSTKEY}"',
    'ExecWait ''$R1'''
  )
  foreach ($fragment in $required) {
    if (-not $Script.Contains($fragment)) { throw 'Generated NSIS source changed; review registry or uninstall contract before guest lifecycle.' }
  }
  if (-not $Script.Contains('!define INSTALLWEBVIEW2MODE "skip"') -and
      -not $Script.Contains('!define INSTALLWEBVIEW2MODE "downloadBootstrapper"')) {
    throw 'Unsupported WebView2 prerequisite side-effect mode.'
  }
}

function Assert-NsisScript([string]$Path, [string]$ExpectedHash) {
  if (-not $Path -or -not $ExpectedHash) { throw 'Generated NSIS script and pinned SHA-256 are required.' }
  $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
  $item = Get-Item -LiteralPath $resolved
  if ($item.PSIsContainer -or $item.Extension -ine '.nsi' -or
      ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Expected a regular, pinned .nsi file, not a reparse point.'
  }
  Assert-GuestLocalPath $resolved
  if ((Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash -ine $ExpectedHash) {
    throw 'Generated NSIS script hash mismatch.'
  }
  $script = [IO.File]::ReadAllText($resolved)
  Assert-NsisContent $script
  return $resolved
}

function Read-RegistryProduct {
  $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Constellation'
  if (-not (Test-Path -LiteralPath $key)) { return $null }
  return Get-ItemProperty -LiteralPath $key
}

function Assert-CleanGuestProductRegistry {
  foreach ($key in @('HKCU:\Software\constellation\Constellation',
      'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Constellation',
      'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Constellation')) {
    if (Test-Path -LiteralPath $key) { throw 'Guest contains a preexisting Constellation product key; use a genuinely clean VM snapshot.' }
  }
  # Tauri NSIS can search HKLM for legacy WiX GUID uninstall records and
  # invoke them even when the ordinary Constellation key is absent.
  foreach ($path in @('HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
      'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*')) {
    foreach ($record in @(Get-ItemProperty -Path $path -ErrorAction SilentlyContinue)) {
      $display = $record.PSObject.Properties['DisplayName']
      $publisher = $record.PSObject.Properties['Publisher']
      if ($display -and $publisher -and $display.Value -ieq 'Constellation' -and
          $publisher.Value -ieq 'constellation') {
        throw 'Legacy MSI/WiX Constellation entry exists in guest HKLM; unsafe to invoke upgrade.'
      }
    }
  }
}

function Assert-ProductOwned([string]$Root, [string]$Version) {
  $record = Read-RegistryProduct
  if ($null -eq $record -or $record.DisplayName -cne 'Constellation' -or
      $record.DisplayVersion -cne $Version) {
    throw 'The expected guest-owned NSIS registration/version is absent.'
  }
  $expected = '"' + $Root + '"'
  if ($record.InstallLocation -ine $expected -or
      $record.UninstallString -ine ('"' + (Join-Path $Root 'uninstall.exe') + '"')) {
    throw 'NSIS registry installation location or uninstall command points outside the owned guest QA root.'
  }
}

function Invoke-BoundedProgram([string]$Exe, [string]$Arguments) {
  # Worker is already confined to the helper's non-input desktop and a
  # kill-on-close Job Object. Never fall back to the input desktop.
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName = $Exe
  $psi.Arguments = $Arguments
  $psi.WorkingDirectory = Split-Path -Parent $Exe
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
  $proc = New-Object Diagnostics.Process
  $proc.StartInfo = $psi
  try {
    if (-not $proc.Start()) { throw 'NSIS process could not start.' }
    if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) {
      $proc.Kill()
      throw 'NSIS process timed out; disposable guest state must be rolled back.'
    }
    if ($proc.ExitCode -ne 0) { throw "NSIS exited $($proc.ExitCode); discard disposable guest state." }
  } finally { $proc.Dispose() }
}

function Assert-State([string]$Root, [string]$ExpectedStage) {
  $statePath = Join-Path $Root 'constellation-nsis-disposable-state.json'
  if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) { throw 'Missing QA ownership marker.' }
  $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
  if ($state.guest_sid -cne $currentSid -or $state.guest_name -cne $currentName -or
      $state.root -ine $Root -or $state.stage -cne $ExpectedStage -or
      $state.old_sha -ine $OldSha256 -or $state.new_sha -ine $NewSha256 -or
      $state.old_version -cne $OldVersion) { throw 'QA ownership marker is incompatible with the current VM and immutable inputs.' }
  return $state
}

function Write-State([string]$Root, [string]$Stage) {
  $state = [ordered]@{
    stage = $Stage; root = $Root; guest_sid = $currentSid; guest_name = $currentName
    old_sha = $OldSha256.ToUpperInvariant(); new_sha = $NewSha256.ToUpperInvariant()
    old_version = $OldVersion
  }
  [IO.File]::WriteAllText((Join-Path $Root 'constellation-nsis-disposable-state.json'),
    ($state | ConvertTo-Json -Depth 3), [Text.Encoding]::UTF8)
}

if ($SelfTest) {
  $valid = @(Test-IdentityBoundary 'Microsoft Corporation' 'Virtual Machine' 'S-1-5-21-999-888-777-1001' 'S-1-5-21-11-22-33-1001' 'CQA-GUEST' 'HOST' $true $true $true)
  if ($valid.Count -ne 0) { throw 'Synthetic disposable guest unexpectedly rejected.' }
  $cases = @(
    @('ASUSTeK COMPUTER INC.','ASUS TUF Gaming F15','S-1-5-21-999-888-777-1001','S-1-5-21-11-22-33-1001','CQA-GUEST','HOST',$true,$true,$true),
    @('Microsoft Corporation','Virtual Machine','S-1-5-21-11-22-33-1001','S-1-5-21-11-22-33-1001','CQA-GUEST','HOST',$true,$true,$true),
    @('Microsoft Corporation','Virtual Machine','S-1-5-21-11-22-33-1002','S-1-5-21-11-22-33-1001','CQA-GUEST','HOST',$true,$true,$true),
    @('Microsoft Corporation','Virtual Machine','S-1-5-21-999-888-777-1001','S-1-5-21-11-22-33-1001','HOST','HOST',$true,$true,$true),
    @('Microsoft Corporation','Virtual Machine','S-1-5-21-999-888-777-1001','S-1-5-21-11-22-33-1001','CQA-GUEST','HOST',$false,$true,$true),
    @('Microsoft Corporation','Virtual Machine','S-1-5-21-999-888-777-1001','S-1-5-21-11-22-33-1001','CQA-GUEST','HOST',$true,$false,$true),
    @('Microsoft Corporation','Virtual Machine','S-1-5-21-999-888-777-1001','S-1-5-21-11-22-33-1001','CQA-GUEST','HOST',$true,$true,$false)
  )
  foreach ($case in $cases) {
    if (@(Test-IdentityBoundary @case).Count -eq 0) { throw 'Synthetic unsafe identity unexpectedly passed.' }
  }
  $synthetic = @'
!define PRODUCTNAME "Constellation"
!define VERSION "0.1.2"
!define INSTALLMODE "currentUser"
!define BUNDLEID "app.constellation.studio"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define INSTALLWEBVIEW2MODE "downloadBootstrapper"
WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString"
DeleteRegKey HKCU "${UNINSTKEY}"
ExecWait '$R1'
'@
  Assert-NsisContent $synthetic
  foreach ($invalid in @(
      $synthetic.Replace('!define INSTALLMODE "currentUser"', '!define INSTALLMODE "perMachine"'),
      $synthetic.Replace('DeleteRegKey HKCU "${UNINSTKEY}"', 'NO_DELETION'),
      $synthetic.Replace('ExecWait ''$R1''', 'NO_PRIOR_UNINSTALL'),
      $synthetic.Replace('!define INSTALLWEBVIEW2MODE "downloadBootstrapper"', '!define INSTALLWEBVIEW2MODE "arbitrary"')
    )) {
    $rejected = $false
    try { Assert-NsisContent $invalid } catch { $rejected = $true }
    if (-not $rejected) { throw 'Synthetic NSIS source mismatch unexpectedly passed.' }
  }
  Write-Output 'PASS disposable NSIS predicates: 1 positive/7 negative VM account cases and 1 positive/4 negative NSIS source contracts. No installer invoked.'
  return
}

$report = Get-BoundaryReport
if ($Mode -eq 'Inspect' -or $Mode -eq 'Preflight') {
  $report | ConvertTo-Json -Depth 6
  # Inspect is observational; Preflight must fail closed.
  if ($Mode -eq 'Preflight' -and $report.issues.Count -gt 0) { exit 3 }
  return
}

# All execution below is gated. The host fails before any filesystem writes.
if (-not $Execute) { throw 'NSIS execution disabled by default; -Execute is required inside a disposable VM.' }
if ($report.issues.Count -gt 0) { throw 'Refusing NSIS lifecycle outside a verified independent disposable Windows guest: ' + ($report.issues -join '; ') }
if (-not $OldVersion -or [version]$OldVersion -ge [version]'0.1.2' -or
    -not $OldInstaller -or -not $NewInstaller) {
  throw 'A real, DIFFERENT older version and both versioned installers are mandatory.'
}
$root = Assert-LifecycleRoot $QaRoot
$old = Assert-PinnedInstaller $OldInstaller $OldSha256
$new = Assert-PinnedInstaller $NewInstaller $NewSha256
$reviewedNsi = Assert-NsisScript $NewNsiScript $NewNsiSha256
if ($old -ieq $new -or $OldSha256 -ieq $NewSha256) { throw 'Distinct installer paths and immutable hashes required for upgrade.' }

Assert-GuestLocalPath $thisScriptDirectory
if ($thisScriptDirectory -match '(?i)\\(OneDrive|Dropbox)\\' -or
    ((Get-Item -LiteralPath $thisScriptDirectory).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
  throw 'QA harness is located in a synchronized or redirected guest directory.'
}

# The private desktop helper must be copied into the DISPOSABLE guest with
# this file. Its worker verifies its own non-input desktop identity.
$helper = Join-Path $thisScriptDirectory 'helper-hidden-desktop.ps1'
if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) {
  throw 'Missing private-desktop helper INSIDE guest. Never use a host shared-folder helper.'
}
. $helper
if (-not $PrivateDesktopWorkerName) {
  $desktopName = 'ConstellationQA_' + [Guid]::NewGuid().ToString('N')
  $args = @(
    '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
    ('"' + $thisScriptPath + '"'), '-Mode', $Mode, '-Execute',
    '-HostSid', ('"' + $HostSid + '"'),
    '-HostComputerName', ('"' + $HostComputerName + '"'),
    '-QaRoot', ('"' + $root + '"'),
    '-OldInstaller', ('"' + $old + '"'), '-NewInstaller', ('"' + $new + '"'),
    '-NewNsiScript', ('"' + $reviewedNsi + '"'), '-NewNsiSha256', $NewNsiSha256,
    '-OldSha256', $OldSha256, '-NewSha256', $NewSha256,
    '-OldVersion', $OldVersion, '-TimeoutSeconds', $TimeoutSeconds,
    '-PrivateDesktopWorkerName', $desktopName
  ) -join ' '
  $child = '& "' + (Join-Path $PSHOME 'powershell.exe') + '" ' + $args + '; exit $LASTEXITCODE'
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($child))
  $exit = [ConstellationHiddenDesktopGuard]::Run(
    (Join-Path $PSHOME 'powershell.exe'), $encoded, $thisScriptDirectory,
    $desktopName, [Math]::Min(600000, 60000 + 1000 * $TimeoutSeconds))
  if ($exit -ne 0) { throw "Private desktop lifecycle phase failed with exit=$exit; revert disposable VM snapshot." }
  # Recheck the child stage from its owner-bound manifest rather than
  # presenting a child exit code as proof of an actual lifecycle transition.
  $stage = switch ($Mode) {
    'InstallOld' { 'old_installed' }
    'Upgrade' { 'upgraded' }
    'UninstallPreserve' { 'uninstalled_preserved' }
  }
  [void](Assert-State $root $stage)
  if ($Mode -eq 'InstallOld') { Assert-ProductOwned $root $OldVersion }
  if ($Mode -eq 'Upgrade') { Assert-ProductOwned $root '0.1.2' }
  if ($Mode -eq 'UninstallPreserve' -and $null -ne (Read-RegistryProduct)) {
    throw 'Uninstall claimed to succeed but guest registry still has Constellation.'
  }
  [void](Assert-PinnedInstaller $old $OldSha256)
  [void](Assert-PinnedInstaller $new $NewSha256)
  [void](Assert-NsisScript $reviewedNsi $NewNsiSha256)
  Write-Output "DISPOSABLE_VM_PHASE_FINISHED=$Mode; this is not a full multi-phase acceptance PASS."
  return
}
Assert-ConstellationHiddenDesktop $PrivateDesktopWorkerName
$again = Get-BoundaryReport
if ($again.issues.Count -gt 0) { throw 'Guest isolation changed before NSIS execution.' }

switch ($Mode) {
  'InstallOld' {
    if (Test-Path -LiteralPath $root) { throw 'InstallOld root already exists; refusing overwrite.' }
    Assert-CleanGuestProductRegistry
    if (Test-Path -LiteralPath (Join-Path $env:APPDATA 'app.constellation.studio')) {
      throw 'Guest app-data path already exists; refuse ownership of prior data.'
    }
    [void](New-Item -Path $root -ItemType Directory -ErrorAction Stop)
    Invoke-BoundedProgram $old ('/S /D=' + $root)
    Assert-ProductOwned $root $OldVersion
    [void](New-Item -Path (Join-Path $env:APPDATA 'app.constellation.studio') -ItemType Directory)
    [IO.File]::WriteAllText((Join-Path $env:APPDATA 'app.constellation.studio\constellation-nsis-qa-marker.txt'),
      'synthetic VM-only preservation marker', [Text.Encoding]::UTF8)
    Write-State $root 'old_installed'
  }
  'Upgrade' {
    [void](Assert-State $root 'old_installed')
    Assert-ProductOwned $root $OldVersion
    Invoke-BoundedProgram $new ('/S /D=' + $root)
    Assert-ProductOwned $root '0.1.2'
    if (-not (Test-Path -LiteralPath (Join-Path $root 'constellation-desktop.exe'))) {
      throw 'Upgrade reported success but installed EXE is absent.'
    }
    $marker = Join-Path $env:APPDATA 'app.constellation.studio\constellation-nsis-qa-marker.txt'
    if (-not (Test-Path -LiteralPath $marker) -or
        (Get-Content -LiteralPath $marker -Raw) -cne 'synthetic VM-only preservation marker') {
      throw 'Upgrade failed to preserve synthetic guest data.'
    }
    Write-State $root 'upgraded'
  }
  'UninstallPreserve' {
    [void](Assert-State $root 'upgraded')
    Assert-ProductOwned $root '0.1.2'
    $marker = Join-Path $env:APPDATA 'app.constellation.studio\constellation-nsis-qa-marker.txt'
    if (-not (Test-Path -LiteralPath $marker) -or
        (Get-Content -LiteralPath $marker -Raw) -cne 'synthetic VM-only preservation marker') {
      throw 'Synthetic marker missing before uninstall.'
    }
    Invoke-BoundedProgram (Join-Path $root 'uninstall.exe') '/S'
    if ($null -ne (Read-RegistryProduct) -or
        (Test-Path -LiteralPath (Join-Path $root 'constellation-desktop.exe')) -or
        -not (Test-Path -LiteralPath $marker)) {
      throw 'Uninstall preservation requirements not met; revert VM snapshot.'
    }
    Write-State $root 'uninstalled_preserved'
  }
}
Write-Output "DISPOSABLE_VM_PHASE_PASS=$Mode; only the chosen phase was observed. Do not claim upgrade/uninstall acceptance without reviewing all phase records and guest VM evidence."
