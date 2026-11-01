# Installed Windows Tauri/WebView2 smoke on a private Win32 desktop. Launches a selected EXE with an
# isolated native data directory and temporary WebView2 cache, inspects only
# its top-level window and known UIA controls, captures only that window, and
# closes only the process started by this script.
# Requires a build that supports --isolated-data-dir. No clicks/provider calls.
#
# Usage:
# powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File tests/windows-installed-uia-smoke.ps1 `
#   -ExePath qa-installed-final-c878520/constellation-desktop.exe
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$ExePath,
  [ValidateRange(5, 90)][int]$TimeoutSeconds = 35,
  [string]$OutputDirectory,
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedExeSha256,
  [string]$HiddenDesktopWorkerName
)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows is required.' }

$resolvedExe = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).ProviderPath
$exeFile = Get-Item -LiteralPath $resolvedExe
if ($exeFile.PSIsContainer -or $exeFile.Extension -ne '.exe') {
  throw 'ExePath must name an existing .exe file.'
}
if ((Get-FileHash -LiteralPath $resolvedExe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
  throw 'EXE hash differs from the prime-confirmed SHA-256; refusing launch.'
}

. (Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1')
if (-not $HiddenDesktopWorkerName) {
  $workerParams = @{
    ScriptPath = $PSCommandPath; ExePath = $resolvedExe; TimeoutSeconds = $TimeoutSeconds
    ExpectedExeSha256 = $ExpectedExeSha256
  }
  if ($OutputDirectory) {
    $workerParams.OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
  }
  Invoke-ConstellationHiddenDesktopWorker @workerParams
  return
}
Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName

$runId = [Guid]::NewGuid().ToString('N')
$scratchRoot = Join-Path ([IO.Path]::GetTempPath()) ("constellation-installed-smoke-$runId")
$artifactRoot = if ($OutputDirectory) {
  $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
} else {
  Join-Path ([IO.Path]::GetTempPath()) ("constellation-uia-evidence-$runId")
}
foreach ($folder in @(
    $scratchRoot,
    (Join-Path $scratchRoot 'Data'),
    (Join-Path $scratchRoot 'Temp'),
    (Join-Path $scratchRoot 'WebView2'),
    $artifactRoot
  )) {
  [void](New-Item -ItemType Directory -Path $folder -Force)
}

$interop = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class ConstellationInstalledWin32 {
  [StructLayout(LayoutKind.Sequential)]
  public struct Rect { public int Left, Top, Right, Bottom; }

  public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr ignored);

  [DllImport("user32.dll", SetLastError = true)]
  private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr ignored);
  [DllImport("user32.dll", SetLastError = true)]
  private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
  [DllImport("user32.dll")]
  private static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
  [DllImport("user32.dll")]
  private static extern bool IsWindowEnabled(IntPtr hwnd);
  [DllImport("user32.dll")]
  private static extern bool IsWindow(IntPtr hwnd);
  [DllImport("user32.dll")]
  private static extern int GetSystemMetrics(int index);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
  [DllImport("user32.dll")]
  private static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", SetLastError = true)]
  public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll", SetLastError = true)]
  public static extern bool PrintWindow(IntPtr hwnd, IntPtr targetDc, uint flags);

  // Helpers are deliberately restricted to windows belonging to the process
  // that THIS script launched, never all windows of the same executable name.
  public static IntPtr FindWindowForProcess(int expectedPid) {
    IntPtr match = IntPtr.Zero;
    EnumWindows((hwnd, ignored) => {
      uint pid;
      GetWindowThreadProcessId(hwnd, out pid);
      if (pid != (uint)expectedPid || !IsWindowVisible(hwnd)) return true;
      var text = new StringBuilder(512);
      GetWindowText(hwnd, text, text.Capacity);
      if (text.ToString().IndexOf("Constellation", StringComparison.OrdinalIgnoreCase) >= 0) {
        Rect bounds;
        if (GetWindowRect(hwnd, out bounds) &&
            bounds.Right - bounds.Left >= 400 && bounds.Bottom - bounds.Top >= 300) {
          match = hwnd;
          return false;
        }
      }
      return true;
    }, IntPtr.Zero);
    return match;
  }

  public static string Title(IntPtr hwnd) {
    var text = new StringBuilder(512);
    GetWindowText(hwnd, text, text.Capacity);
    return text.ToString();
  }
  public static string OwnerPredicates(IntPtr hwnd, int expectedPid) {
    uint ownerPid;
    uint tid = GetWindowThreadProcessId(hwnd, out ownerPid);
    return "is_window=" + IsWindow(hwnd)
      + "; owner_pid=" + ownerPid + "; owner_thread=" + tid
      + "; expected_pid=" + expectedPid
      + "; root_matches=" + (GetAncestor(hwnd, 2) == hwnd)
      + "; enabled=" + IsWindowEnabled(hwnd)
      + "; screen_width=" + GetSystemMetrics(0)
      + "; screen_height=" + GetSystemMetrics(1);
  }
}
'@
Add-Type -TypeDefinition $interop -ReferencedAssemblies 'System' -ErrorAction Stop
Add-Type -AssemblyName System.Drawing -ErrorAction Stop

$uiaAvailable = $true
$uiaLoadError = ''
try {
  Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes -ErrorAction Stop
} catch {
  $uiaAvailable = $false
  $uiaLoadError = 'Windows UI Automation assemblies could not be loaded.'
}

$result = [ordered]@{
  run_id = $runId
  exe_path = $resolvedExe
  exe_sha256 = (Get-FileHash -LiteralPath $resolvedExe -Algorithm SHA256).Hash.ToLowerInvariant()
  started_pid = $null
  window_handle = $null
  window_title = $null
  native_owner_predicates = $null
  window_bounds_px = $null
  webview_ready = $false
  uia_status = if ($uiaAvailable) { 'not_checked' } else { 'unavailable' }
  uia_descendants = 0
  observed_controls = @()
  screenshot_path = $null
  screenshot_method = $null
  screenshot_distinct_sample_colors = $null
  screenshot_status = 'not_captured'
  process_closed = $null
  isolation_status = 'not_checked'
  verified_window_desktop = $null
  isolation = 'native --isolated-data-dir, isolated TEMP/TMP/WebView2; APPDATA/USERPROFILE unmodified'
  isolated_profile = $scratchRoot
  isolated_data_directory = Join-Path $scratchRoot 'Data'
  errors = @()
}
if ($uiaLoadError) { $result.errors += $uiaLoadError }

$process = $null
try {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $resolvedExe
  $psi.Arguments = '--isolated-data-dir "' + (Join-Path $scratchRoot 'Data') + '"'
  $psi.WorkingDirectory = $exeFile.DirectoryName
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $false
  $psi.EnvironmentVariables['TEMP'] = Join-Path $scratchRoot 'Temp'
  $psi.EnvironmentVariables['TMP'] = Join-Path $scratchRoot 'Temp'
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $scratchRoot 'WebView2'

  # Old installed builds ignore the new CLI isolation flag. Reject those
  # builds before any startup: otherwise the GUI would open the real profile.
  # This is a conservative compiled-string guard; the post-start database
  # location check below independently verifies the flag actually took effect.
  $binaryText = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($resolvedExe))
  if (-not $binaryText.Contains('--isolated-data-dir')) {
    $result.isolation_status = 'blocked_flag_not_found'
    throw 'Isolation blocked: selected executable does not contain --isolated-data-dir. No application process was started.'
  }
  $binaryText = $null
  $result.isolation_status = 'native_flag_detected'
  if ((Get-FileHash -LiteralPath $resolvedExe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
    throw 'EXE changed since the private desktop worker launched; refusing application launch.'
  }

  $process = New-Object System.Diagnostics.Process
  $process.StartInfo = $psi
  Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
  if (-not $process.Start()) { throw 'The selected installed executable could not be started.' }
  $result.started_pid = $process.Id
  if ($uiaAvailable) { $result.uia_status = 'waiting' }

  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  $hwnd = [IntPtr]::Zero
  $expectedControls = @('Home', 'Rooms', 'Tasks', 'Evidence', 'Library', 'Settings', 'Create room', 'Create your first room')
  while ([DateTime]::UtcNow -lt $deadline) {
    $process.Refresh()
    if ($process.HasExited) { throw "The app exited before readiness (exit code $($process.ExitCode))." }
    $hwnd = [ConstellationInstalledWin32]::FindWindowForProcess($process.Id)
    if ($hwnd -ne [IntPtr]::Zero) {
      Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
      $result.verified_window_desktop = $HiddenDesktopWorkerName
      $result.window_handle = $hwnd.ToInt64()
      $result.window_title = [ConstellationInstalledWin32]::Title($hwnd)
      $result.native_owner_predicates = [ConstellationInstalledWin32]::OwnerPredicates($hwnd, $process.Id)
      if ($uiaAvailable) {
        try {
          $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
          $children = $root.FindAll(
            [System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.Condition]::TrueCondition
          )
          $result.uia_descendants = $children.Count
          $found = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
          foreach ($child in $children) {
            try {
              $controlName = $child.Current.Name
              if ($expectedControls -contains $controlName) { [void]$found.Add($controlName) }
            } catch { } # A disappearing WebView element is normal during load.
          }
          $result.observed_controls = @($found | Sort-Object)
          $result.uia_status = 'available'
          # Require WebView descendants and multiple known Svelte controls.
          if ($children.Count -ge 10 -and $found.Count -ge 3) {
            $result.webview_ready = $true
            break
          }
        } catch {
          $result.uia_status = 'query_failed'
        }
      } else {
        # A window alone is insufficient evidence of WebView2 readiness.
        # Preserve the screenshot attempt with an explicit UIA limitation.
        Start-Sleep -Milliseconds 1200
        break
      }
    }
    Start-Sleep -Milliseconds 300
  }
  if ($hwnd -eq [IntPtr]::Zero) { throw 'No visible, correctly sized Constellation HWND owned by the launched PID appeared before timeout.' }
  if (-not [IO.File]::Exists((Join-Path $scratchRoot 'Data\constellation.sqlite3'))) {
    $result.isolation_status = 'native_directory_not_used'
    throw 'The selected app did not create its SQLite database in the requested isolated directory; UIA/screenshot acceptance is blocked.'
  }
  $result.isolation_status = 'native_database_created_in_isolated_directory'
  if ($uiaAvailable -and -not $result.webview_ready) {
    $result.errors += 'WebView2 UI Automation did not expose three expected navigation controls before timeout.'
  }

  $rect = New-Object ConstellationInstalledWin32+Rect
  if (-not [ConstellationInstalledWin32]::GetWindowRect($hwnd, [ref]$rect)) {
    throw 'GetWindowRect failed for the launched application window.'
  }
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  if ($width -lt 400 -or $height -lt 300 -or $width -gt 8192 -or $height -gt 8192) {
    throw "Unexpected window bounds ($width x $height)."
  }
  $result.window_bounds_px = [ordered]@{ x = $rect.Left; y = $rect.Top; width = $width; height = $height }

  # PrintWindow paints the selected HWND into our own bitmap. Unlike
  # CopyFromScreen, it cannot capture the user's unrelated foreground windows.
  $bitmap = New-Object System.Drawing.Bitmap($width, $height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $dc = $graphics.GetHdc()
    try {
      $painted = [ConstellationInstalledWin32]::PrintWindow($hwnd, $dc, 2)
      if (-not $painted) { $painted = [ConstellationInstalledWin32]::PrintWindow($hwnd, $dc, 0) }
    } finally { $graphics.ReleaseHdc($dc) }
    if (-not $painted) { throw 'PrintWindow could not capture this WebView2 window.' }

    $sampleColors = New-Object 'System.Collections.Generic.HashSet[int]'
    foreach ($y in 0..15) {
      foreach ($x in 0..15) {
        $pixel = $bitmap.GetPixel([Math]::Min($width - 1, [int](($x + 0.5) * $width / 16)), [Math]::Min($height - 1, [int](($y + 0.5) * $height / 16)))
        [void]$sampleColors.Add($pixel.ToArgb())
      }
    }
    $result.screenshot_distinct_sample_colors = $sampleColors.Count
    $result.screenshot_status = if ($sampleColors.Count -gt 5) { 'captured_with_visual_detail' } else { 'captured_but_appears_blank' }
    $imagePath = Join-Path $artifactRoot ("installed-window-$runId.png")
    $bitmap.Save($imagePath, [System.Drawing.Imaging.ImageFormat]::Png)
    $result.screenshot_path = $imagePath
    $result.screenshot_method = 'System.Drawing.Bitmap + Win32 PrintWindow(HWND); native-window crop'
    if ($sampleColors.Count -le 5) {
      $result.errors += 'The captured window contains too little visual variation to confirm WebView2 content.'
    }
  } finally {
    $graphics.Dispose()
    $bitmap.Dispose()
  }
} catch {
  $result.errors += $_.Exception.Message
} finally {
  if ($null -ne $process) {
    try {
      $process.Refresh()
      if (-not $process.HasExited) {
        [void]$process.CloseMainWindow()
        if (-not $process.WaitForExit(4000)) {
          # Kill only the process HANDLE started by this script; never by name
          # and never kill an existing app or the broader process tree.
          $process.Kill()
          [void]$process.WaitForExit(5000)
        }
      }
      $result.process_closed = $process.HasExited
    } catch {
      $result.errors += 'Could not confirm that the spawned application process closed.'
    } finally {
      $process.Dispose()
    }
  }
}

$result | ConvertTo-Json -Depth 6
if (-not $result.webview_ready -or $result.screenshot_status -ne 'captured_with_visual_detail' -or -not $result.process_closed) {
  exit 1
}
