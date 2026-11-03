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
  [switch]$TestCommandPalette,
  [switch]$TestTaskJourney,
  [switch]$TestRoomTeam,
  [switch]$TestProviderDirectory,
  [ValidateRange(800, 4096)][int]$ViewportWidth = 0,
  [ValidateRange(560, 2160)][int]$ViewportHeight = 0,
  [ValidateSet(1, 2)][int]$WebViewDeviceScaleFactor = 1,
  [Parameter(Mandatory = $true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedExeSha256,
  [string]$HiddenDesktopWorkerName
)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows is required.' }
if (($ViewportWidth -gt 0) -ne ($ViewportHeight -gt 0)) { throw 'ViewportWidth and ViewportHeight must be provided together.' }
if ($TestRoomTeam -and -not $TestTaskJourney) { throw 'Room team native QA requires the isolated Task journey to create a room and agent first.' }

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
    ExpectedExeSha256 = $ExpectedExeSha256; TestCommandPalette = [bool]$TestCommandPalette
    TestTaskJourney = [bool]$TestTaskJourney; TestProviderDirectory = [bool]$TestProviderDirectory
    TestRoomTeam = [bool]$TestRoomTeam
    ViewportWidth = $ViewportWidth; ViewportHeight = $ViewportHeight
    WebViewDeviceScaleFactor = $WebViewDeviceScaleFactor
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
  public static extern bool MoveWindow(IntPtr hwnd, int x, int y, int width, int height, bool repaint);
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
  viewport_resize_status = if ($ViewportWidth -gt 0) { 'pending' } else { 'not_requested' }
  webview_scale_request = if ($WebViewDeviceScaleFactor -eq 2) { 'forced WebView2 device-scale-factor 2; effect requires screenshot inspection, not actual browser zoom' } else { 'default' }
  webview_ready = $false
  uia_status = if ($uiaAvailable) { 'not_checked' } else { 'unavailable' }
  uia_descendants = 0
  observed_controls = @()
  palette_result = if ($TestCommandPalette) { 'pending' } else { 'not_requested' }
  task_journey_result = if ($TestTaskJourney) { 'pending' } else { 'not_requested' }
  room_team_result = if ($TestRoomTeam) { 'pending' } else { 'not_requested' }
  provider_directory_result = if ($TestProviderDirectory) { 'pending' } else { 'not_requested' }
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
  if ($WebViewDeviceScaleFactor -eq 2) {
    # This is a PRIVATE child-process WebView2 command line for a high-DPI
    # layout probe. It never changes the user's OS scaling or browser profile.
    $psi.EnvironmentVariables['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--force-device-scale-factor=2'
  }

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
  if ($ViewportWidth -gt 0) {
    Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
    $before = New-Object ConstellationInstalledWin32+Rect
    if (-not [ConstellationInstalledWin32]::GetWindowRect($hwnd, [ref]$before)) { throw 'Cannot read owned HWND bounds before resize.' }
    if (-not [ConstellationInstalledWin32]::MoveWindow($hwnd, $before.Left, $before.Top, $ViewportWidth, $ViewportHeight, $true)) {
      throw 'Cannot resize the private test HWND.'
    }
    Start-Sleep -Milliseconds 450
    $after = New-Object ConstellationInstalledWin32+Rect
    if (-not [ConstellationInstalledWin32]::GetWindowRect($hwnd, [ref]$after)) { throw 'Cannot read owned HWND bounds after resize.' }
    $actualWidth = $after.Right - $after.Left
    $actualHeight = $after.Bottom - $after.Top
    $result.viewport_requested_px = @{ width = $ViewportWidth; height = $ViewportHeight }
    $result.viewport_actual_px = @{ width = $actualWidth; height = $actualHeight }
    if ([Math]::Abs($actualWidth - $ViewportWidth) -gt 8 -or [Math]::Abs($actualHeight - $ViewportHeight) -gt 8) {
      throw "Private HWND refused requested resize: requested $ViewportWidth x $ViewportHeight, observed $actualWidth x $actualHeight."
    }
    $result.viewport_resize_status = 'passed: owned application HWND resized, not a 200% browser zoom proof'
  }
  if ($TestCommandPalette) {
    if (-not $uiaAvailable -or -not $result.webview_ready) {
      throw 'Cannot test palette without a ready native WebView2 accessibility tree.'
    }
    Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
    Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $nameProperty = [System.Windows.Automation.AutomationElement]::NameProperty
    $scope = [System.Windows.Automation.TreeScope]::Descendants
    $commandButton = $root.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition($nameProperty, 'Search workspace (Ctrl+K)')))
    if ($null -eq $commandButton) { throw 'The real UIA tree does not expose the workspace search button.' }
    $invoke = $commandButton.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    if ($null -eq $invoke) { throw 'The workspace search button lacks a real invoke pattern.' }
    $invoke.Invoke()
    $foundSearch = $false
    $foundEvidence = $false
    $deadline = [DateTime]::UtcNow.AddSeconds(6)
    do {
      $controls = $root.FindAll($scope, [System.Windows.Automation.Condition]::TrueCondition)
      foreach ($control in $controls) {
        try {
          if ($control.Current.Name -eq 'Search workspace commands and rooms') { $foundSearch = $true }
          if ($control.Current.Name -eq 'Evidence') { $foundEvidence = $true }
        } catch {}
      }
      if ($foundSearch -and $foundEvidence) { break }
      Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $foundSearch -or -not $foundEvidence) {
      throw "The invoked native palette is incomplete: search=$foundSearch; evidence=$foundEvidence."
    }
    $modal = $root.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition($nameProperty, 'Search workspace')))
    if ($null -eq $modal) { throw 'Palette search modal is missing from the current UIA tree.' }
    $searchControl = $modal.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition($nameProperty, 'Search workspace commands and rooms')))
    if ($null -eq $searchControl) { throw 'Palette input is missing inside the search dialog.' }
    $result.palette_input_type = $searchControl.Current.ControlType.ProgrammaticName
    $supportedValue = $null
    if (-not $searchControl.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$supportedValue)) {
      throw 'Palette input has no writable UI Automation ValuePattern.'
    }
    $value = [System.Windows.Automation.ValuePattern]$supportedValue
    if ($value.Current.IsReadOnly) { throw 'Palette search input is read-only.' }
    $value.SetValue('zz-a-purposefully-unmatched-destination')
    $noMatches = $false
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
      $message = $modal.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition($nameProperty, 'No matching destinations. Try the room name or a section such as Evidence.')))
      if ($null -ne $message) { $noMatches = $true; break }
      Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $noMatches) { throw 'Palette did not show its real no-results message after UIA typed an unmatched search.' }
    $value.SetValue('Evidence')
    $foundOption = $false
    $result.palette_sample_results = @()
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
      $listbox = $modal.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition($nameProperty, 'Workspace destinations')))
      $resultItem = $null
      if ($null -ne $listbox) {
        $resultControls = $listbox.FindAll($scope, [System.Windows.Automation.Condition]::TrueCondition)
        $labels = @()
        foreach ($child in $resultControls) {
          try {
            $name = [string]$child.Current.Name
            if ($name -and $labels.Count -lt 14) { $labels += ($child.Current.ControlType.ProgrammaticName + ': ' + $name) }
            if ($name -match '^Evidence(?:\s|$)' -and $name -match 'Saved documents') { $resultItem = $child }
          } catch {}
        }
        $result.palette_sample_results = $labels
      }
      if ($null -ne $resultItem) { $foundOption = $true; break }
      Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $foundOption) { throw 'Palette search did not expose the Evidence result in its scoped listbox.' }
    $result.palette_result = 'passed: native UIA invoke + ValuePattern unmatched/positive search and real listbox result'
  }
  if ($TestTaskJourney) {
    if (-not $uiaAvailable -or -not $result.webview_ready) {
      throw 'Cannot exercise task creation without a ready native WebView2 accessibility tree.'
    }
    Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
    Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $scope = [System.Windows.Automation.TreeScope]::Descendants
    function Find-ExactUxControl([string]$name, [string]$type = '', [int]$seconds = 7) {
      $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
      do {
        $matches = @()
        foreach ($child in $root.FindAll($scope, [System.Windows.Automation.Condition]::TrueCondition)) {
          try {
            if ($child.Current.Name -ceq $name -and (!$type -or $child.Current.ControlType.ProgrammaticName -ceq $type)) {
              $matches += $child
            }
          } catch {}
        }
        if ($matches.Count -eq 1) { return $matches[0] }
        if ($matches.Count -gt 1) { throw "Ambiguous native UIA control '$name' ($($matches.Count))." }
        Start-Sleep -Milliseconds 160
      } while ([DateTime]::UtcNow -lt $deadline)
      throw "Expected native UIA control was not exposed: '$name' [$type]."
    }
    function Invoke-UxControl([System.Windows.Automation.AutomationElement]$element) {
      $patternObject = $null
      if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$patternObject)) {
        ([System.Windows.Automation.InvokePattern]$patternObject).Invoke()
        return
      }
      # Chromium exposes aria-pressed filter buttons through UIA TogglePattern.
      if ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$patternObject)) {
        ([System.Windows.Automation.TogglePattern]$patternObject).Toggle()
        return
      }
      # WebView2 may expose aria-expanded buttons through ExpandCollapsePattern
      # rather than InvokePattern. This is an accessibility action on the
      # private-desktop element, not synthetic input to the physical keyboard.
      if ($element.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$patternObject)) {
        $expandable = [System.Windows.Automation.ExpandCollapsePattern]$patternObject
        if ($expandable.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed) {
          $expandable.Expand()
        } elseif ($expandable.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded) {
          $expandable.Collapse()
        } else {
          throw "Native expandable button '$($element.Current.Name)' has no actionable expanded/collapsed state."
        }
        return
      }
      # LegacyIAccessible is Chromium's fallback for some otherwise-semantic
      # HTML buttons. A follow-up DB assertion must prove the requested action.
      if ($element.TryGetCurrentPattern([System.Windows.Automation.LegacyIAccessiblePattern]::Pattern, [ref]$patternObject)) {
        ([System.Windows.Automation.LegacyIAccessiblePattern]$patternObject).DoDefaultAction()
        return
      }
      $available = @($element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName }) -join ', '
      throw "Native button '$($element.Current.Name)' has no actionable UIA pattern; available: $available."
    }
    function Set-UxInput([System.Windows.Automation.AutomationElement]$element, [string]$text) {
      $patternObject = $null
      if (-not $element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$patternObject)) {
        throw "Missing ValuePattern for native input '$($element.Current.Name)'."
      }
      $valuePattern = [System.Windows.Automation.ValuePattern]$patternObject
      if ($valuePattern.Current.IsReadOnly) { throw "Native input '$($element.Current.Name)' is read-only." }
      $valuePattern.SetValue($text)
    }
    if ($TestCommandPalette) {
      Invoke-UxControl (Find-ExactUxControl 'Close dialog' 'ControlType.Button')
    }
    Invoke-UxControl (Find-ExactUxControl 'Tasks' 'ControlType.Button')
    Invoke-UxControl (Find-ExactUxControl 'Plan your first task' 'ControlType.Button')
    [void](Find-ExactUxControl 'Complete your setup to save this plan')
    [void](Find-ExactUxControl 'Create a room first' 'ControlType.Button')
    [void](Find-ExactUxControl 'Add a teammate' 'ControlType.Button')
    $taskText = 'Private UX task journey, no model or file action'
    Set-UxInput (Find-ExactUxControl 'INSTRUCTIONS' 'ControlType.Edit') $taskText
    Invoke-UxControl (Find-ExactUxControl 'Create a room first' 'ControlType.Button')
    Set-UxInput (Find-ExactUxControl 'ROOM NAME' 'ControlType.Edit') 'Private UX room'
    Invoke-UxControl (Find-ExactUxControl 'Create room' 'ControlType.Button')
    Invoke-UxControl (Find-ExactUxControl 'Add a teammate' 'ControlType.Button')
    Set-UxInput (Find-ExactUxControl 'DISPLAY NAME' 'ControlType.Edit') 'Private UX analyst'
    Invoke-UxControl (Find-ExactUxControl 'Add teammate' 'ControlType.Button')
    $draft = Find-ExactUxControl 'INSTRUCTIONS' 'ControlType.Edit'
    $valuePattern = $draft.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    if ([string]$valuePattern.Current.Value -cne $taskText) {
      throw 'Task draft was lost while completing room and agent setup.'
    }
    Invoke-UxControl (Find-ExactUxControl 'Save pending task' 'ControlType.Button')
    [void](Find-ExactUxControl $taskText)
    [void](Find-ExactUxControl ('Model for task execution: ' + $taskText) 'ControlType.ComboBox')
    $run = Find-ExactUxControl 'Run text task' 'ControlType.Button'
    if ($run.Current.IsEnabled) { throw 'Task Run is enabled without any configured provider.' }
    Invoke-UxControl (Find-ExactUxControl 'Completed' 'ControlType.Button')
    [void](Find-ExactUxControl 'No matching tasks')
    Invoke-UxControl (Find-ExactUxControl 'All' 'ControlType.Button')
    Set-UxInput (Find-ExactUxControl 'Find a task' 'ControlType.Edit') 'no-matching-synthetic-task'
    [void](Find-ExactUxControl 'No matching tasks')
    Invoke-UxControl (Find-ExactUxControl 'Clear task filters' 'ControlType.Button')
    [void](Find-ExactUxControl $taskText)
    $sqlitePath = Join-Path (Join-Path $scratchRoot 'Data') 'constellation.sqlite3'
    $readTask = @'
const { DatabaseSync } = require("node:sqlite");
const db = new DatabaseSync(process.argv[2], { readOnly: true });
const count = db.prepare("SELECT COUNT(*) AS n FROM tasks").get().n;
console.log(count);
db.close();
'@
    $count = $readTask | node - $sqlitePath
    if ($LASTEXITCODE -ne 0 -or [string]$count -cne '1') {
      throw 'Expected exactly one native persisted task in the disposable SQLite file.'
    }
    $result.task_journey_result = 'passed: native prerequisites, draft return, SQLite task, safe disabled Run, status and keyword filtering'
    if ($TestRoomTeam) {
      Invoke-UxControl (Find-ExactUxControl 'Rooms' 'ControlType.Button')
      $found = $null
      $deadline = [DateTime]::UtcNow.AddSeconds(8)
      do {
        foreach ($child in $root.FindAll($scope, [System.Windows.Automation.Condition]::TrueCondition)) {
          try {
            if ($child.Current.Name -match '^Agents.*1 selected.*Change room team') { $found = $child; break }
          } catch {}
        }
        if ($null -ne $found) { break }
        Start-Sleep -Milliseconds 150
      } while ([DateTime]::UtcNow -lt $deadline)
      if ($null -eq $found) { throw 'Room team expandable control was not discoverable in actual WebView2 UIA.' }
      $expansion = $null
      if (-not $found.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$expansion)) {
        throw 'Room team control does not expose keyboard-accessible ExpandCollapsePattern.'
      }
      ([System.Windows.Automation.ExpandCollapsePattern]$expansion).Expand()
      $member = Find-ExactUxControl 'Deselect worker Private UX analyst for this room' 'ControlType.Button'
      $toggle = $null
      if (-not $member.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) {
        throw 'Worker selection did not expose a proper aria-pressed toggle button.'
      }
      ([System.Windows.Automation.TogglePattern]$toggle).Toggle()
      Invoke-UxControl (Find-ExactUxControl 'Save room team' 'ControlType.Button')
      [void](Find-ExactUxControl 'This room team is saved on this device.')
      $readRoom = @'
const { DatabaseSync } = require("node:sqlite");
const db = new DatabaseSync(process.argv[2], { readOnly: true });
const rooms = db.prepare('SELECT data FROM rooms').all();
if (rooms.length !== 1) process.exit(3);
const room = JSON.parse(rooms[0].data);
if (!Array.isArray(room.member_ids) || room.member_ids.length !== 0) process.exit(4);
const recorded = db.prepare('SELECT kind FROM events').all()
  .some(({kind}) => JSON.parse(kind).type === 'room_members_changed');
db.close();
if (!recorded) process.exit(5);
console.log('PERSISTED_EMPTY_TEAM_AND_AUDIT_PASS');
'@
      $verification = $readRoom | node - $sqlitePath
      if ($LASTEXITCODE -ne 0 -or [string]$verification -cne 'PERSISTED_EMPTY_TEAM_AND_AUDIT_PASS') {
        throw 'Persisted room membership/audit did not match the user-initiated UI selection.'
      }
      # Test the other removal affordance separately. The referenced worker
      # owns the saved task above; the native action must reject deletion and
      # retain its identity. There is no model/provider request.
      Invoke-UxControl (Find-ExactUxControl 'Remove unused worker Private UX analyst from workspace' 'ControlType.Button')
      [void](Find-ExactUxControl 'Confirm removing Private UX analyst from the workspace')
      Invoke-UxControl (Find-ExactUxControl 'Confirm remove unused' 'ControlType.Button')
      [void](Find-ExactUxControl 'Worker removal denied. Remove the worker from saved rooms first; workers with messages, tasks or workflow history must be retained. A changed worker cannot be deleted from a stale window.')
      # Now create and remove a genuinely unused worker through real buttons.
      Invoke-UxControl (Find-ExactUxControl 'Add agent' 'ControlType.Button')
      Set-UxInput (Find-ExactUxControl 'DISPLAY NAME' 'ControlType.Edit') 'Temporary unused worker'
      Invoke-UxControl (Find-ExactUxControl 'Add teammate' 'ControlType.Button')
      # The newly persisted worker's text is nested inside an aria-labelled
      # roster button; Chromium does not necessarily expose a separate UIA
      # text node matching the bare display name. The actionable control is
      # the stronger assertion that the new worker is visible in the roster.
      $unusedWorkerRemove = Find-ExactUxControl 'Remove unused worker Temporary unused worker from workspace' 'ControlType.Button'
      Invoke-UxControl $unusedWorkerRemove
      [void](Find-ExactUxControl 'Confirm removing Temporary unused worker from the workspace')
      Invoke-UxControl (Find-ExactUxControl 'Confirm remove unused' 'ControlType.Button')
      $readWorkers = @'
const { DatabaseSync } = require('node:sqlite');
const db = new DatabaseSync(process.argv[2], { readOnly: true });
const agents = db.prepare('SELECT data FROM agents').all().map(({data}) => JSON.parse(data));
const removedEvents = db.prepare('SELECT kind FROM events').all().filter(({kind}) => JSON.parse(kind).type === 'agent_removed');
const tasks = db.prepare('SELECT data FROM tasks').all().map(({data}) => JSON.parse(data));
db.close();
if (agents.length !== 1 || agents[0].name !== 'Private UX analyst') process.exit(4);
if (removedEvents.length !== 1 || tasks.length !== 1 || tasks[0].agent_id !== agents[0].id) process.exit(5);
console.log('NATIVE_WORKER_REMOVAL_REFERENCES_PROTECTED_AND_UNUSED_REMOVED_PASS');
'@
      $verified = $readWorkers | node - $sqlitePath
      if ($LASTEXITCODE -ne 0 -or [string]$verified -cne 'NATIVE_WORKER_REMOVAL_REFERENCES_PROTECTED_AND_UNUSED_REMOVED_PASS') {
        throw 'Native worker removal did not preserve the referenced worker and remove only unused worker with audited event.'
      }
      $result.room_team_result = 'passed: native toggle buttons, explicit saved empty team, referenced worker removal denied, unused worker removed with SQLite audit'
    }
  }
  if ($TestProviderDirectory) {
    if (-not $uiaAvailable -or -not $result.webview_ready) {
      throw 'Provider directory requires an actual WebView2/UIA tree.'
    }
    Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
    Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $scope = [System.Windows.Automation.TreeScope]::Descendants
    $nameProperty = [System.Windows.Automation.AutomationElement]::NameProperty
    function Find-ProviderControl([string]$name, [string]$type = '', [int]$timeout = 8) {
      $deadline = [DateTime]::UtcNow.AddSeconds($timeout)
      do {
        $matches = @()
        foreach ($child in $root.FindAll($scope, [System.Windows.Automation.Condition]::TrueCondition)) {
          try {
            if ($child.Current.Name -ceq $name -and (!$type -or $child.Current.ControlType.ProgrammaticName -ceq $type)) {
              $matches += $child
            }
          } catch { }
        }
        if ($matches.Count -eq 1) { return $matches[0] }
        if ($matches.Count -gt 1) { throw "Ambiguous provider UIA element '$name' ($($matches.Count))." }
        Start-Sleep -Milliseconds 170
      } while ([DateTime]::UtcNow -lt $deadline)
      throw "Provider UIA element missing: '$name'."
    }
    function Invoke-ProviderControl([string]$name) {
      $button = Find-ProviderControl $name
      $value = $null
      if (-not $button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$value)) {
        throw "Provider control cannot be invoked: '$name'."
      }
      ([System.Windows.Automation.InvokePattern]$value).Invoke()
    }
    function Set-ProviderValue([string]$name, [string]$value) {
      $input = Find-ProviderControl $name 'ControlType.Edit'
      $pattern = $null
      if (-not $input.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        throw "Provider input does not expose ValuePattern: '$name'."
      }
      $vp = [System.Windows.Automation.ValuePattern]$pattern
      if ($vp.Current.IsReadOnly) { throw "Provider input is read-only: '$name'." }
      $vp.SetValue($value)
    }
    function Assert-ProviderValue([string]$name, [string]$expected) {
      $input = Find-ProviderControl $name 'ControlType.Edit'
      $pattern = $null
      if (-not $input.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        throw "Provider input does not expose readable ValuePattern: '$name'."
      }
      $actual = ([System.Windows.Automation.ValuePattern]$pattern).Current.Value
      if ($actual -cne $expected) { throw "Provider revision did not prefill '$name' correctly." }
    }
    # Save only a dummy loopback profile in this private database. No key,
    # probe, model request, external connection or credentials are used.
    Invoke-ProviderControl 'Settings'
    Invoke-ProviderControl 'Add provider'
    [void](Find-ProviderControl 'Bring your own model.')
    Set-ProviderValue 'DISPLAY NAME' 'QA isolated local model'
    Set-ProviderValue 'MODEL IDENTIFIER' 'qa-disabled-model'
    Invoke-ProviderControl 'Save connection locally'
    [void](Find-ProviderControl 'QA isolated local model')
    [void](Find-ProviderControl 'Not tested in this session')
    Set-ProviderValue 'Find a provider' 'definitely-not-a-provider'
    [void](Find-ProviderControl 'No providers match these local filters. Clear the search or choose All.')
    Set-ProviderValue 'Find a provider' 'qa-disabled-model'
    [void](Find-ProviderControl 'QA isolated local model')
    # Revise opens the existing public configuration only. Do not submit,
    # probe or retrieve API keys from the OS credential manager.
    Invoke-ProviderControl 'Revise configuration for QA isolated local model'
    [void](Find-ProviderControl 'Revise a connection.')
    [void](Find-ProviderControl 'Save revised configuration')
    Assert-ProviderValue 'DISPLAY NAME' 'QA isolated local model'
    Assert-ProviderValue 'MODEL IDENTIFIER' 'qa-disabled-model'
    Assert-ProviderValue 'API BASE URL' 'http://127.0.0.1:11434/v1'
    # A changed public identity creates a second local profile. Never
    # overwrite or erase the original, copy a secret or make a model call.
    Set-ProviderValue 'DISPLAY NAME' 'QA revised isolated model'
    Invoke-ProviderControl 'Save revised configuration'
    [void](Find-ProviderControl 'QA isolated local model')
    [void](Find-ProviderControl 'QA revised isolated model')
    # Removal is a separately confirmed, fail-closed action. This disposable
    # test profile has no persisted credential, message, task or workflow.
    Invoke-ProviderControl 'Remove unused connection QA revised isolated model'
    [void](Find-ProviderControl 'Confirm removal of QA revised isolated model')
    Invoke-ProviderControl 'Confirm remove unused'
    [void](Find-ProviderControl 'QA isolated local model')
    $sqlitePath = Join-Path (Join-Path $scratchRoot 'Data') 'constellation.sqlite3'
    $readProviders = @'
const { DatabaseSync } = require('node:sqlite');
const db = new DatabaseSync(process.argv[2], { readOnly: true });
const rows = db.prepare('SELECT data FROM providers').all();
if (rows.length !== 1 || JSON.parse(rows[0].data).display_name !== 'QA isolated local model') process.exit(4);
const removalEvents = db.prepare('SELECT kind FROM events').all()
  .filter(({kind}) => JSON.parse(kind).type === 'provider_removed');
db.close();
if (removalEvents.length !== 1) process.exit(5);
console.log('PROVIDER_REMOVAL_LOCAL_AUDIT_PASS');
'@
    $verified = $readProviders | node - $sqlitePath
    if ($LASTEXITCODE -ne 0 -or [string]$verified -cne 'PROVIDER_REMOVAL_LOCAL_AUDIT_PASS') {
      throw 'UIA removal failed to persist exactly one original provider and one audited removal.'
    }
    $result.provider_directory_result = 'passed: dummy provider persisted; local search positive/negative; safe revision created separate profile; explicit confirmed removal left original and one audited event without probes or key access'
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
if (-not $result.webview_ready -or $result.screenshot_status -ne 'captured_with_visual_detail' -or -not $result.process_closed -or ($ViewportWidth -gt 0 -and -not $result.viewport_resize_status.StartsWith('passed:')) -or ($TestCommandPalette -and -not $result.palette_result.StartsWith('passed:')) -or ($TestTaskJourney -and -not $result.task_journey_result.StartsWith('passed:')) -or ($TestRoomTeam -and -not $result.room_team_result.StartsWith('passed:')) -or ($TestProviderDirectory -and -not $result.provider_directory_result.StartsWith('passed:'))) {
  exit 1
}
