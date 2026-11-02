# UI Automation focus evidence only: no hardware keyboard emulation, SendInput,
# foreground manipulation, provider requests, or WebView browser zoom claims.
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$ExePath,
  [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$ExpectedExeSha256,
  [ValidateRange(10,50)][int]$TimeoutSeconds = 30,
  [string]$OutputDirectory,
  [string]$HiddenDesktopWorkerName
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Requires Windows' }
$exe = (Resolve-Path -LiteralPath $ExePath).ProviderPath
if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
  throw 'Executable hash mismatch; refusing launch'
}
. (Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1')
if (-not $HiddenDesktopWorkerName) {
  $dir = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else {
    [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\target\ux-keyboard-focus-20261102'))
  }
  [void](New-Item -ItemType Directory -Force -Path $dir)
  $worker = @{
    ScriptPath = $PSCommandPath
    ExePath = $exe
    ExpectedExeSha256 = $ExpectedExeSha256
    TimeoutSeconds = $TimeoutSeconds
    OutputDirectory = $dir
  }
  Invoke-ConstellationHiddenDesktopWorker @worker
  return
}
Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
if (-not $OutputDirectory) { throw 'Private desktop worker needs isolated output path' }
$artifact = [IO.Path]::GetFullPath($OutputDirectory)
$allowed = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\target')).TrimEnd('\') + '\'
if (-not $artifact.StartsWith($allowed, [StringComparison]::OrdinalIgnoreCase)) {
  throw 'QA evidence must remain under ignored application target directory'
}
[void](New-Item -ItemType Directory -Force -Path $artifact)
$id = [Guid]::NewGuid().ToString('N')
$scratch = Join-Path $artifact ('isolated-app-' + $id)
foreach ($name in @('Data','Temp','WebView2')) {
  [void](New-Item -ItemType Directory -Force -Path (Join-Path $scratch $name))
}
$result = [ordered]@{
  run_id = $id
  exe_sha256 = $ExpectedExeSha256.ToLowerInvariant()
  private_desktop_proven = $false
  webview_uia_ready = $false
  dialog_open_by_uia = $false
  combobox_uia_accessible = $false
  matching_listbox_result_uia = $false
  combobox_has_keyboard_focus = $false
  previous_focus_restored_after_uia_close = $false
  previous_focus_before_open = $false
  app_data_isolated = $false
  process_stopped = $false
  physical_keyboard_result = 'NOT_TESTED: hidden non-input desktop cannot safely receive physical SendInput'
  browser_zoom_200_percent_result = 'NOT_TESTED: forced device scale and HWND resizing are not browser zoom'
  errors = @()
}
$interop = @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class UxKeyboardOwnWindow {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
  public delegate bool Enumerator(IntPtr hwnd,IntPtr context);
  [DllImport("user32.dll")] private static extern bool EnumWindows(Enumerator callback,IntPtr context);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
  [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr hwnd,out RECT rect);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int capacity);
  public static IntPtr Find(int expected) {
    IntPtr match=IntPtr.Zero;
    EnumWindows((w,context) => {
      uint pid;GetWindowThreadProcessId(w,out pid);
      if(pid!=(uint)expected || !IsWindowVisible(w)) return true;
      var title=new StringBuilder(300);GetWindowText(w,title,title.Capacity);
      RECT rect;
      if(title.ToString().Contains("Constellation") && GetWindowRect(w,out rect)
        && rect.Right-rect.Left>=400 && rect.Bottom-rect.Top>=300) {
        match=w;return false;
      }
      return true;
    },IntPtr.Zero);
    return match;
  }
}
'@
Add-Type -TypeDefinition $interop -ReferencedAssemblies 'System' -ErrorAction Stop
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes -ErrorAction Stop
$process = $null
try {
  if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe)).Contains('--isolated-data-dir')) {
    throw 'Selected binary has no native isolated-data-dir support'
  }
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $exe
  $psi.Arguments = '--isolated-data-dir "' + (Join-Path $scratch 'Data') + '"'
  $psi.WorkingDirectory = Split-Path -Parent $exe
  $psi.UseShellExecute = $false
  $psi.EnvironmentVariables['TEMP'] = (Join-Path $scratch 'Temp')
  $psi.EnvironmentVariables['TMP'] = (Join-Path $scratch 'Temp')
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'] = (Join-Path $scratch 'WebView2')
  Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
  $process = New-Object Diagnostics.Process
  $process.StartInfo = $psi
  if (-not $process.Start()) { throw 'Owned app launch failed' }
  $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  $hwnd = [IntPtr]::Zero
  $root = $null
  $button = $null
  $scope = [System.Windows.Automation.TreeScope]::Descendants
  $name = [System.Windows.Automation.AutomationElement]::NameProperty
  while ([DateTime]::UtcNow -lt $deadline) {
    $process.Refresh()
    if ($process.HasExited) { throw 'App exited before native WebView readiness' }
    $hwnd = [UxKeyboardOwnWindow]::Find($process.Id)
    if ($hwnd -ne [IntPtr]::Zero) {
      Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
      $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
      $button = $root.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition($name, 'Search workspace (Ctrl+K)')))
      if ($null -ne $button) { $result.private_desktop_proven=$true; $result.webview_uia_ready=$true; break }
    }
    Start-Sleep -Milliseconds 300
  }
  if (-not $result.webview_uia_ready) { throw 'No private WebView2 UIA command button appeared' }
  if (-not (Test-Path -LiteralPath (Join-Path $scratch 'Data\constellation.sqlite3'))) {
    throw 'App did not create SQLite under its isolated QA data path'
  }
  $result.app_data_isolated = $true
  Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
  $button.SetFocus()
  Start-Sleep -Milliseconds 150
  $result.previous_focus_before_open = [bool]$button.Current.HasKeyboardFocus
  ([System.Windows.Automation.InvokePattern]$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke()
  $deadline = [DateTime]::UtcNow.AddSeconds(8)
  $dialog = $null; $combo = $null
  while ([DateTime]::UtcNow -lt $deadline) {
    $dialog = $root.FindFirst($scope,(New-Object System.Windows.Automation.PropertyCondition($name,'Search workspace')))
    $combo = $root.FindFirst($scope,(New-Object System.Windows.Automation.PropertyCondition($name,'Search workspace commands and rooms')))
    if ($null -ne $dialog -and $null -ne $combo) { break }
    Start-Sleep -Milliseconds 120
  }
  if ($null -eq $dialog -or $null -eq $combo) { throw 'Private UIA Invoke did not open real command palette dialog' }
  $result.dialog_open_by_uia = $true
  $result.combobox_uia_accessible = ($combo.Current.ControlType.ProgrammaticName -eq 'ControlType.ComboBox')
  $result.combobox_has_keyboard_focus = [bool]$combo.Current.HasKeyboardFocus
  ([System.Windows.Automation.ValuePattern]$combo.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)).SetValue('Evidence')
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ([DateTime]::UtcNow -lt $deadline) {
    $list = $dialog.FindFirst($scope,(New-Object System.Windows.Automation.PropertyCondition($name,'Workspace destinations')))
    if ($null -ne $list) {
      foreach ($option in $list.FindAll($scope,[System.Windows.Automation.Condition]::TrueCondition)) {
        if ($option.Current.Name -match '^Evidence.*Saved documents') {
          $result.matching_listbox_result_uia = $true; break
        }
      }
    }
    if ($result.matching_listbox_result_uia) { break }
    Start-Sleep -Milliseconds 100
  }
  $close = $dialog.FindFirst($scope,(New-Object System.Windows.Automation.PropertyCondition($name,'Close dialog')))
  if ($null -eq $close) { throw 'Native UIA dialog close button absent' }
  Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
  ([System.Windows.Automation.InvokePattern]$close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)).Invoke()
  $deadline = [DateTime]::UtcNow.AddSeconds(5)
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($null -eq $root.FindFirst($scope,(New-Object System.Windows.Automation.PropertyCondition($name,'Search workspace')))) {
      $result.previous_focus_restored_after_uia_close = [bool]$button.Current.HasKeyboardFocus
      if ($result.previous_focus_restored_after_uia_close) { break }
    }
    Start-Sleep -Milliseconds 120
  }
} catch {
  $result.errors += [string]$_.Exception.Message
} finally {
  if ($process) {
    try {
      if (-not $process.HasExited) { $process.Kill(); [void]$process.WaitForExit(5000) }
      $result.process_stopped = [bool]$process.HasExited
    } catch { $result.errors += 'Unable to prove owned process stopped' }
    $process.Dispose()
  }
  $output = Join-Path $artifact ('ux-keyboard-focus-' + $id + '.json')
  $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $output -Encoding UTF8
  Get-Content -LiteralPath $output -Raw | Write-Output
}
if (-not $result.private_desktop_proven -or -not $result.dialog_open_by_uia -or
    -not $result.app_data_isolated -or -not $result.process_stopped -or $result.errors.Count) {
  throw 'Native private keyboard QA incomplete: inspect JSON, no forced pass'
}
