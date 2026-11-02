# Isolated Windows desktop bootstrap for Constellation UIA tests. The parent
# never launches an application itself. A child PowerShell interpreter runs on
# a newly created, non-input WinSta0 desktop, and verifies that identity before
# it is permitted to launch the tested app. NEVER call SwitchDesktop.
#
# Read-only launch probe (no app):
# powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/helper-hidden-desktop.ps1 -ProbeOnly
[CmdletBinding()]
param(
  [switch]$ProbeOnly,
  [string]$VerifyWorkerDesktopName,
  [string]$ProbeOutputPath,
  [string]$InstallNsisPath,
  [string]$InstallTarget,
  [string]$HiddenInstallerDesktopName,
  [switch]$NativeEventProbeSelfTest,
  [ValidateRange(5,120)][int]$InstallTimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Hidden desktop bootstrap requires Windows.' }

if (-not ('ConstellationHiddenDesktopGuard' -as [type])) {
  $desktopInterop = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class ConstellationHiddenDesktopGuard {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  private struct STARTUPINFO {
    public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX; public int dwY; public int dwXSize; public int dwYSize;
    public int dwXCountChars; public int dwYCountChars; public int dwFillAttribute;
    public int dwFlags; public short wShowWindow; public short cbReserved2;
    public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct PROCESS_INFORMATION {
    public IntPtr hProcess, hThread; public uint dwProcessId, dwThreadId;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
    public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
    public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
    public uint ActiveProcessLimit;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct IO_COUNTERS {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
    public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
    public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
    public IO_COUNTERS IoInfo;
    public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
  }
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern IntPtr CreateDesktop(string name, string device, IntPtr devMode, uint flags, uint desiredAccess, IntPtr securityAttributes);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint desiredAccess);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern IntPtr GetThreadDesktop(uint threadId);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool GetUserObjectInformation(IntPtr obj, int index, StringBuilder data, int length, out int needed);
  [DllImport("kernel32.dll")]
  private static extern uint GetCurrentThreadId();
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool CreateProcess(string applicationName, StringBuilder commandLine, IntPtr processAttributes,
    IntPtr threadAttributes, bool inheritHandles, uint creationFlags, IntPtr environment,
    string currentDirectory, ref STARTUPINFO startup, out PROCESS_INFORMATION process);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern IntPtr CreateJobObject(IntPtr securityAttributes, string name);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool SetInformationJobObject(IntPtr job, int informationClass, ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION info, uint length);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern uint ResumeThread(IntPtr thread);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool CloseHandle(IntPtr handle);

  private static string Name(IntPtr desktop) {
    if (desktop == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "Desktop handle unavailable");
    var buffer = new StringBuilder(256); int required;
    if (!GetUserObjectInformation(desktop, 2, buffer, buffer.Capacity * 2, out required))
      throw new Win32Exception(Marshal.GetLastWin32Error(), "Desktop name query failed");
    return buffer.ToString();
  }
  private static string InputName() {
    // The input desktop is whatever the user can currently see. Never switch.
    IntPtr desktop = OpenInputDesktop(0, false, 0x00020000);
    if (desktop == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot identify input desktop: refusing launch");
    try { return Name(desktop); } finally { CloseDesktop(desktop); }
  }
  public static string VerifyWorker(string expectedName) {
    if (String.IsNullOrEmpty(expectedName) || !expectedName.StartsWith("ConstellationQA_", StringComparison.Ordinal)
        || expectedName.Length != 48) throw new InvalidOperationException("Malformed isolated desktop identity");
    string attached = Name(GetThreadDesktop(GetCurrentThreadId()));
    string input = InputName();
    if (!String.Equals(attached, expectedName, StringComparison.Ordinal)
        || String.Equals(attached, input, StringComparison.Ordinal))
      throw new InvalidOperationException("Refusing launch: PowerShell is not attached to its unique, non-input desktop");
    return attached;
  }
  // Window ownership is checked independently of the worker. If a future
  // process-launch implementation moves the app onto the input desktop,
  // refuse every UIA interaction and screenshot rather than inspecting it.
  public static string VerifyWindow(IntPtr window, string expectedName) {
    if (window == IntPtr.Zero) throw new ArgumentException("Missing window handle");
    uint pid;
    uint threadId = GetWindowThreadProcessId(window, out pid);
    if (threadId == 0 || pid == 0)
      throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot identify application window thread");
    string actual = Name(GetThreadDesktop(threadId));
    string input = InputName();
    if (!String.Equals(actual, expectedName, StringComparison.Ordinal)
        || String.Equals(actual, input, StringComparison.Ordinal))
      throw new InvalidOperationException("Application window does not belong to the private non-input desktop");
    return actual;
  }
  public static int Run(string powershellExe, string encodedCommand, string workingDirectory, string desktopName, int timeoutMs) {
    if (timeoutMs < 1000 || timeoutMs > 600000) throw new ArgumentOutOfRangeException("timeoutMs");
    if (String.Equals(Name(GetThreadDesktop(GetCurrentThreadId())), desktopName, StringComparison.Ordinal)
        || String.Equals(InputName(), desktopName, StringComparison.Ordinal))
      throw new InvalidOperationException("Cannot use caller's active desktop");
    const uint desktopRights = 0x000F00FF; // excludes DESKTOP_SWITCHDESKTOP
    IntPtr desktop = CreateDesktop(desktopName, null, IntPtr.Zero, 0, desktopRights, IntPtr.Zero);
    if (desktop == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateDesktop failed");
    IntPtr job = IntPtr.Zero;
    PROCESS_INFORMATION process = new PROCESS_INFORMATION();
    try {
      if (Name(desktop) != desktopName || InputName() == desktopName)
        throw new InvalidOperationException("New desktop is input desktop");
      job = CreateJobObject(IntPtr.Zero, null);
      if (job == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateJobObject failed");
      var limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
      limits.BasicLimitInformation.LimitFlags = 0x00002000; // KILL_ON_JOB_CLOSE (including descendant app)
      if (!SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))))
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Job kill-on-close setup failed");
      var startup = new STARTUPINFO { cb = Marshal.SizeOf(typeof(STARTUPINFO)), lpDesktop = "WinSta0\\" + desktopName,
        dwFlags = 1, wShowWindow = 0 };
      var cmd = new StringBuilder("\"" + powershellExe + "\" -NoProfile -NonInteractive -STA -ExecutionPolicy Bypass -EncodedCommand " + encodedCommand);
      // CREATE_SUSPENDED prevents the worker from launching before assignment
      // to the kill-on-close job. CREATE_NO_WINDOW suppresses its console.
      if (!CreateProcess(powershellExe, cmd, IntPtr.Zero, IntPtr.Zero, false, 0x08000004, IntPtr.Zero,
          workingDirectory, ref startup, out process))
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Isolated CreateProcess failed");
      if (!AssignProcessToJobObject(job, process.hProcess))
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot contain worker process in cleanup job");
      if (ResumeThread(process.hThread) == 0xFFFFFFFF)
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot resume isolated worker");
      uint wait = WaitForSingleObject(process.hProcess, (uint)timeoutMs);
      if (wait == 0x102) throw new TimeoutException("Isolated desktop worker timed out; job cleanup will terminate it and descendants");
      if (wait != 0) throw new Win32Exception(Marshal.GetLastWin32Error(), "Waiting for isolated worker failed");
      uint exit;
      if (!GetExitCodeProcess(process.hProcess, out exit))
        throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot read isolated worker exit status");
      return unchecked((int)exit);
    } finally {
      if (job != IntPtr.Zero) CloseHandle(job); // application and WebView2 descendants cannot survive this job
      if (process.hThread != IntPtr.Zero) CloseHandle(process.hThread);
      if (process.hProcess != IntPtr.Zero) CloseHandle(process.hProcess);
      CloseDesktop(desktop);
    }
  }
}
'@
  Add-Type -TypeDefinition $desktopInterop -ReferencedAssemblies 'System' -ErrorAction Stop
}

function Assert-ConstellationHiddenDesktop([string]$DesktopName) {
  [void][ConstellationHiddenDesktopGuard]::VerifyWorker($DesktopName)
}

function Assert-ConstellationHiddenWindow([IntPtr]$Window, [string]$DesktopName) {
  [void][ConstellationHiddenDesktopGuard]::VerifyWindow($Window, $DesktopName)
}

function Quote-HiddenWorkerLiteral([string]$Value) {
  return "'" + $Value.Replace("'", "''") + "'"
}

function Invoke-ConstellationHiddenDesktopWorker {
  param(
    [Parameter(Mandatory=$true)][string]$ScriptPath,
    [Parameter(Mandatory=$true)][string]$ExePath,
    [Parameter(Mandatory=$true)][int]$TimeoutSeconds,
    [string]$OutputDirectory,
    [switch]$SkipBackup,
    [switch]$TestProjectArchive,
    [switch]$TestWorkflows,
    [switch]$TestWorkflowStep,
    [switch]$TestWorkflowReport,
    [switch]$TestFileTools,
    [switch]$TestNativeConsent,
    [switch]$TestCommandPalette,
    [switch]$TestTaskJourney,
    [switch]$TestRoomTeam,
    [switch]$TestProviderDirectory,
    [int]$ViewportWidth,
    [int]$ViewportHeight,
    [int]$WebViewDeviceScaleFactor = 1,
    [string]$ExpectedExeSha256,
    [string]$ExpectedNativeSourceSha256,
    [string]$ExpectedSourceIdentityPath
  )
  $desktopName = 'ConstellationQA_' + [Guid]::NewGuid().ToString('N')
  $outputFile = Join-Path ([IO.Path]::GetTempPath()) ("constellation-hidden-worker-" + [Guid]::NewGuid().ToString('N') + '.txt')
  $quotedScript = Quote-HiddenWorkerLiteral $ScriptPath
  $scriptLine = "& $quotedScript -ExePath $(Quote-HiddenWorkerLiteral $ExePath) -TimeoutSeconds $TimeoutSeconds -HiddenDesktopWorkerName $(Quote-HiddenWorkerLiteral $desktopName)"
  if ($OutputDirectory) { $scriptLine += " -OutputDirectory $(Quote-HiddenWorkerLiteral $OutputDirectory)" }
  if ($SkipBackup) { $scriptLine += ' -SkipBackup' }
  if ($TestProjectArchive) { $scriptLine += ' -TestProjectArchive' }
  if ($TestWorkflows) { $scriptLine += ' -TestWorkflows' }
  if ($TestWorkflowStep) { $scriptLine += ' -TestWorkflowStep' }
  if ($TestWorkflowReport) { $scriptLine += ' -TestWorkflowReport' }
  if ($TestFileTools) { $scriptLine += ' -TestFileTools' }
  if ($TestNativeConsent) { $scriptLine += ' -TestNativeConsent' }
  if ($TestCommandPalette) { $scriptLine += ' -TestCommandPalette' }
  if ($TestTaskJourney) { $scriptLine += ' -TestTaskJourney' }
  if ($TestRoomTeam) { $scriptLine += ' -TestRoomTeam' }
  if ($TestProviderDirectory) { $scriptLine += ' -TestProviderDirectory' }
  if ($ViewportWidth -gt 0 -and $ViewportHeight -gt 0) { $scriptLine += " -ViewportWidth $ViewportWidth -ViewportHeight $ViewportHeight" }
  if ($WebViewDeviceScaleFactor -eq 2) { $scriptLine += ' -WebViewDeviceScaleFactor 2' }
  if ($ExpectedExeSha256) { $scriptLine += " -ExpectedExeSha256 $(Quote-HiddenWorkerLiteral $ExpectedExeSha256)" }
  if ($ExpectedNativeSourceSha256) { $scriptLine += " -ExpectedNativeSourceSha256 $(Quote-HiddenWorkerLiteral $ExpectedNativeSourceSha256)" }
  if ($ExpectedSourceIdentityPath) { $scriptLine += " -ExpectedSourceIdentityPath $(Quote-HiddenWorkerLiteral $ExpectedSourceIdentityPath)" }
  $workerCode = "`$ErrorActionPreference='Stop'; $scriptLine *> $(Quote-HiddenWorkerLiteral $outputFile)"
  $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($workerCode))
  $powershellPath = Join-Path $PSHOME 'powershell.exe'
  if (-not (Test-Path -LiteralPath $powershellPath -PathType Leaf)) { throw 'Windows PowerShell executable missing.' }
  try {
    $exitCode = [ConstellationHiddenDesktopGuard]::Run(
      $powershellPath, $encoded, (Split-Path -Parent $ScriptPath), $desktopName,
      [Math]::Min(600000, 60000 + 3000 * $TimeoutSeconds)
    )
    if (Test-Path -LiteralPath $outputFile) {
      Get-Content -LiteralPath $outputFile | Write-Output
    }
    if ($exitCode -ne 0) { throw "Hidden desktop acceptance exited $exitCode; no active-desktop fallback was attempted." }
  } finally {
    if (Test-Path -LiteralPath $outputFile) { Remove-Item -LiteralPath $outputFile -Force }
  }
}

if ($NativeEventProbeSelfTest) {
  if (-not $VerifyWorkerDesktopName) {
    $desktopName = 'ConstellationQA_' + [Guid]::NewGuid().ToString('N')
    $outputFile = Join-Path ([IO.Path]::GetTempPath()) ('constellation-native-event-selftest-' + [Guid]::NewGuid().ToString('N') + '.txt')
    try {
      $line = "& $(Quote-HiddenWorkerLiteral $PSCommandPath) -VerifyWorkerDesktopName $(Quote-HiddenWorkerLiteral $desktopName) -NativeEventProbeSelfTest *> $(Quote-HiddenWorkerLiteral $outputFile)"
      $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($line))
      $code = [ConstellationHiddenDesktopGuard]::Run((Join-Path $PSHOME 'powershell.exe'), $encoded, $PSScriptRoot, $desktopName, 15000)
      if (Test-Path -LiteralPath $outputFile) { Get-Content -LiteralPath $outputFile | Write-Output }
      if ($code -ne 0) { throw "Native event observer self-test failed on private desktop (exit=$code)." }
    } finally {
      if (Test-Path -LiteralPath $outputFile) { Remove-Item -LiteralPath $outputFile -Force }
    }
    return
  }
  Assert-ConstellationHiddenDesktop $VerifyWorkerDesktopName
  . (Join-Path $PSScriptRoot 'helper-native-consent.ps1')
  Write-Output ('PASS private WinEvent observer: ' + [ConstellationPrivateConsentDialog]::SelfTestNativeEventHook($PID))
  return
}

if ($InstallNsisPath) {
  if (-not $InstallTarget) { throw 'InstallTarget is required for a hidden NSIS installation.' }
  $installer = (Resolve-Path -LiteralPath $InstallNsisPath -ErrorAction Stop).ProviderPath
  if ([IO.Path]::GetExtension($installer) -ine '.exe') { throw 'NSIS installer must be an existing EXE.' }
  if (-not [IO.Path]::IsPathRooted($InstallTarget)) { throw 'InstallTarget must be absolute.' }
  $target = [IO.Path]::GetFullPath($InstallTarget)
  $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
  if (-not $target.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Refusing installation outside the temporary QA directory.'
  }
  if (Test-Path -LiteralPath $target) { throw 'Refusing to overwrite an existing QA installation.' }
  if (-not $HiddenInstallerDesktopName) {
    $desktopName = 'ConstellationQA_' + [Guid]::NewGuid().ToString('N')
    $outputFile = Join-Path ([IO.Path]::GetTempPath()) ('constellation-hidden-nsis-' + [Guid]::NewGuid().ToString('N') + '.txt')
    $line = "& $(Quote-HiddenWorkerLiteral $PSCommandPath) -InstallNsisPath $(Quote-HiddenWorkerLiteral $installer) -InstallTarget $(Quote-HiddenWorkerLiteral $target) -HiddenInstallerDesktopName $(Quote-HiddenWorkerLiteral $desktopName) -InstallTimeoutSeconds $InstallTimeoutSeconds"
    $worker = "`$ErrorActionPreference='Stop'; $line *> $(Quote-HiddenWorkerLiteral $outputFile)"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($worker))
    try {
      $exitCode = [ConstellationHiddenDesktopGuard]::Run(
        (Join-Path $PSHOME 'powershell.exe'), $encoded, $PSScriptRoot, $desktopName,
        ([Math]::Min(180000, 20000 + 1000 * $InstallTimeoutSeconds))
      )
      if (Test-Path -LiteralPath $outputFile) { Get-Content -LiteralPath $outputFile | Write-Output }
      if ($exitCode -ne 0) { throw "Hidden-desktop silent NSIS installation exited $exitCode; no active desktop install attempted." }
    } finally {
      if (Test-Path -LiteralPath $outputFile) { Remove-Item -LiteralPath $outputFile -Force }
    }
    return
  }
  Assert-ConstellationHiddenDesktop $HiddenInstallerDesktopName
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName = $installer
  # NSIS requires /D=<path> as its final, unquoted argument.
  $psi.Arguments = '/S /D=' + $target
  $psi.WorkingDirectory = Split-Path -Parent $installer
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
  $installProcess = New-Object Diagnostics.Process
  $installProcess.StartInfo = $psi
  try {
    Assert-ConstellationHiddenDesktop $HiddenInstallerDesktopName
    if (-not $installProcess.Start()) { throw 'Silent NSIS installer could not start on private desktop.' }
    if (-not $installProcess.WaitForExit($InstallTimeoutSeconds * 1000)) {
      $installProcess.Kill()
      throw 'Silent NSIS installation timed out in its private desktop.'
    }
    if ($installProcess.ExitCode -ne 0) { throw "Silent NSIS installation exited $($installProcess.ExitCode)." }
    $installedExe = Join-Path $target 'constellation-desktop.exe'
    if (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
      throw 'NSIS reported success but the isolated installed executable is absent.'
    }
    Assert-ConstellationHiddenDesktop $HiddenInstallerDesktopName
    Write-Output "PASS: private-desktop silent NSIS install; EXE=$installedExe SHA256=$((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash)"
  } finally {
    $installProcess.Dispose()
  }
  return
}

if ($VerifyWorkerDesktopName) {
  Assert-ConstellationHiddenDesktop $VerifyWorkerDesktopName
  if ($ProbeOutputPath) {
    [IO.File]::WriteAllText($ProbeOutputPath, 'isolated_desktop_verified', [Text.Encoding]::UTF8)
  }
} elseif ($ProbeOnly) {
  $probeOutput = Join-Path ([IO.Path]::GetTempPath()) ('constellation-hidden-probe-' + [Guid]::NewGuid().ToString('N') + '.txt')
  $desktopName = 'ConstellationQA_' + [Guid]::NewGuid().ToString('N')
  try {
    $line = "& $(Quote-HiddenWorkerLiteral $PSCommandPath) -VerifyWorkerDesktopName $(Quote-HiddenWorkerLiteral $desktopName) -ProbeOutputPath $(Quote-HiddenWorkerLiteral $probeOutput)"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($line))
    $code = [ConstellationHiddenDesktopGuard]::Run((Join-Path $PSHOME 'powershell.exe'), $encoded, (Split-Path -Parent $PSCommandPath), $desktopName, 15000)
    if ($code -ne 0 -or -not (Test-Path -LiteralPath $probeOutput) -or (Get-Content -LiteralPath $probeOutput -Raw) -ne 'isolated_desktop_verified') {
      throw "Isolated desktop worker verification failed (exit=$code). No app launched."
    }
    Write-Output 'PASS: Unique hidden desktop identity verified in child PowerShell. No app launched.'
  } finally {
    if (Test-Path -LiteralPath $probeOutput) { Remove-Item -LiteralPath $probeOutput -Force }
  }
}
