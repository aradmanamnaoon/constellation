# Isolated hidden-desktop Windows UI Automation acceptance for Constellation.
# Operates on one uniquely isolated Tauri --isolated-data-dir and WebView2
# profile, invokes only a bounded localhost fixture when -TestWorkflowStep is
# explicitly selected, and closes only the application process
# started here. UI interaction is permitted solely through discovered UIA
# controls; a missing pattern is a recorded blocker, never a simulated PASS.
#
# powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File .\tests\windows-evidence-backup-e2e.ps1 `
#   -ExePath "C:\path\to\constellation-desktop.exe"
# -TestNativeConsent requires a prime-pinned custom-protocol EXE, native
# consent source SHA-256, and exact eleven-source identity manifest.
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$ExePath,
  [ValidateRange(10, 90)][int]$TimeoutSeconds = 40,
  [string]$OutputDirectory,
  [switch]$SkipBackup,
  [switch]$TestProjectArchive,
  [switch]$TestWorkflows,
  [switch]$TestDrafts,
  [switch]$TestWorkflowStep,
  [switch]$TestWorkflowReport,
  [switch]$TestFileTools,
  [switch]$TestNativeConsent,
  [string]$ExpectedExeSha256,
  [string]$ExpectedNativeSourceSha256,
  [string]$ExpectedSourceIdentityPath,
  [string]$HiddenDesktopWorkerName
)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows is required.' }
if ($TestWorkflowStep) { $TestWorkflows = $true }
if ($TestWorkflowReport) { $TestWorkflows = $true }
if ($TestNativeConsent) { $TestFileTools = $true }
if ($TestFileTools -and -not $TestNativeConsent) {
  throw 'The current native V2 file-approval flow requires -TestNativeConsent; refusing an unhandled modal dialog before launch.'
}
if ($TestNativeConsent -and -not $ExpectedExeSha256) {
  throw 'Native V2 review acceptance requires the new prime-confirmed EXE SHA-256; refusing launch.'
}
if ($TestNativeConsent -and $ExpectedNativeSourceSha256 -notmatch '^[a-fA-F0-9]{64}$') {
  throw 'Native V2 acceptance requires the prime-confirmed native_consent.rs source SHA-256; refusing launch.'
}
if ($TestNativeConsent -and -not $ExpectedSourceIdentityPath) {
  throw 'Native SHA-bound read acceptance requires the complete eleven-source fingerprint file; refusing launch.'
}
if ($TestNativeConsent -and $ExpectedExeSha256 -ieq 'AA93166E3D2C6C5E50FB1D0C072BAC52E9E06C062E01C72406750A6E2686E218') {
  throw 'Old AA93166E release predates native V2 consent integration; refusing stale binary.'
}
$exe = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).ProviderPath
$exeInfo = Get-Item -LiteralPath $exe
if ($exeInfo.PSIsContainer -or $exeInfo.Extension -ine '.exe') {
  throw '-ExePath must refer to an existing executable.'
}
if ($ExpectedExeSha256) {
  if ($ExpectedExeSha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'ExpectedExeSha256 must be exactly 64 hexadecimal characters.' }
  $actual = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
  if ($actual -ine $ExpectedExeSha256) { throw 'Executable hash differs from prime-confirmed SHA-256; refusing launch.' }
}
$nativeSource = Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\src\native_consent.rs'
function Assert-PinnedConstellationSources {
  param([string]$ManifestPath, [string]$NativeHash)
  $expectedNames = @(
    'apps/desktop/src-tauri/src/lib.rs',
    'apps/desktop/src-tauri/src/native_consent.rs',
    'apps/desktop/src-tauri/src/tool_host.rs',
    'apps/desktop/src/routes/+page.svelte',
    'apps/desktop/src/routes/studio.css',
    'apps/desktop/src-tauri/Cargo.toml',
    'apps/desktop/src-tauri/Cargo.lock',
    'crates/constellation-tools/src/execution.rs',
    'crates/constellation-tools/src/lib.rs',
    'crates/constellation-tools/Cargo.toml',
    'crates/constellation-tools/Cargo.lock'
  )
  $manifestFile = (Resolve-Path -LiteralPath $ManifestPath -ErrorAction Stop).ProviderPath
  $raw = [IO.File]::ReadAllText($manifestFile)
  # PowerShell 5.1 pipes a top-level JSON array as one object[] value;
  # ConvertFrom-Json -InputObject preserves the actual entry count.
  $entries = ConvertFrom-Json -InputObject $raw
  if ($entries.Count -ne $expectedNames.Count) { throw 'SHA-bound read build requires exactly eleven pinned source entries.' }
  $rootDir = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
  foreach ($name in $expectedNames) {
    # $Matches is a PowerShell automatic regex variable. Do not use it as a
    # local name: -notmatch would silently replace the entry array.
    $foundEntries = @($entries | Where-Object { $_.Path -ceq $name })
    if ($foundEntries.Count -ne 1 -or [string]$foundEntries[0].Hash -notmatch '^[a-fA-F0-9]{64}$') {
      throw "Pinned source manifest is missing or duplicates expected file $name."
    }
    $entry = $foundEntries[0]
    $file = Join-Path $rootDir ($name.Replace('/', '\'))
    $info = Get-Item -LiteralPath $file -ErrorAction Stop
    $observed = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
    if ($info.PSIsContainer -or $info.Length -ne [long]$entry.Size -or
        $observed -ine [string]$entry.Hash) {
      throw "Pinned source file changed or differs from manifest: $name."
    }
    if ($name -ceq 'apps/desktop/src-tauri/src/native_consent.rs' -and
        $observed -ine $NativeHash) {
      throw 'Pinned native consent source does not match the separately confirmed SHA-256.'
    }
  }
  return (Get-FileHash -LiteralPath $manifestFile -Algorithm SHA256).Hash.ToLowerInvariant()
}
$expectedManifestHash = $null
if ($TestNativeConsent) {
  $expectedManifestHash = Assert-PinnedConstellationSources $ExpectedSourceIdentityPath $ExpectedNativeSourceSha256
}
if ($TestNativeConsent -and
    (Get-FileHash -LiteralPath $nativeSource -Algorithm SHA256).Hash -ine $ExpectedNativeSourceSha256) {
  throw 'Native V2 source changed after prime confirmation; refusing launch.'
}

. (Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1')
if (-not $HiddenDesktopWorkerName) {
  # No native process may start in this caller's interactive desktop.
  $workerParams = @{
    ScriptPath = $PSCommandPath; ExePath = $exe; TimeoutSeconds = $TimeoutSeconds
    SkipBackup = $SkipBackup; TestProjectArchive = $TestProjectArchive; TestWorkflows = $TestWorkflows; TestWorkflowStep = $TestWorkflowStep
    TestDrafts = $TestDrafts
    TestWorkflowReport = $TestWorkflowReport; TestFileTools = $TestFileTools
    TestNativeConsent = $TestNativeConsent; ExpectedExeSha256 = $ExpectedExeSha256
    ExpectedNativeSourceSha256 = $ExpectedNativeSourceSha256
    ExpectedSourceIdentityPath = $ExpectedSourceIdentityPath
  }
  if ($OutputDirectory) {
    $workerParams.OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
  }
  Invoke-ConstellationHiddenDesktopWorker @workerParams
  return
}
Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName

$runId = [Guid]::NewGuid().ToString('N')
# Deliberate two-launch QA uses the native-only, exact app-owned resume
# identity. Other acceptance sessions keep their old one-launch sandbox.
$sandboxRoot = if ($TestDrafts) { Join-Path $env:LOCALAPPDATA 'Temp' } else { [IO.Path]::GetTempPath() }
$sandboxName = if ($TestDrafts) { "constellation-model-qa-$runId" } else { "constellation-ui-e2e-$runId" }
$sandbox = Join-Path $sandboxRoot $sandboxName
$isolatedData = Join-Path $sandbox 'Data'
$artifacts = if ($OutputDirectory) {
  $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
} else {
  Join-Path ([IO.Path]::GetTempPath()) "constellation-ui-e2e-captures-$runId"
}
foreach ($path in @($sandbox, $isolatedData, (Join-Path $sandbox 'Temp'), (Join-Path $sandbox 'WebView2'), $artifacts)) {
  [void](New-Item -ItemType Directory -Force -Path $path)
}
if (@(Get-ChildItem -LiteralPath $isolatedData -Force).Count -ne 0) {
  throw 'Isolation refusal: the selected native data directory is not empty.'
}

$win32 = @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class ConstellationEvidenceWindow {
  [StructLayout(LayoutKind.Sequential)]
  public struct Rect { public int Left, Top, Right, Bottom; }
  public delegate bool EnumWindowsCallback(IntPtr hwnd, IntPtr ignored);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr ignored);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  private static extern int GetWindowText(IntPtr hwnd, StringBuilder buffer, int size);
  [DllImport("user32.dll")]
  private static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);

  public static IntPtr FindOwnMainWindow(int pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((hwnd, unused) => {
      uint owner;
      GetWindowThreadProcessId(hwnd, out owner);
      if (owner != (uint)pid || !IsWindowVisible(hwnd)) return true;
      var title = new StringBuilder(512);
      GetWindowText(hwnd, title, title.Capacity);
      Rect rect;
      if (title.ToString().IndexOf("Constellation", StringComparison.OrdinalIgnoreCase) >= 0
          && GetWindowRect(hwnd, out rect) && rect.Right - rect.Left >= 400
          && rect.Bottom - rect.Top >= 300) {
        found = hwnd;
        return false;
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
'@
Add-Type -TypeDefinition $win32 -ReferencedAssemblies 'System' -ErrorAction Stop
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes -ErrorAction Stop
Add-Type -AssemblyName System.Drawing -ErrorAction Stop

function Ui-Records($root) {
  $items = $root.FindAll(
    [Windows.Automation.TreeScope]::Descendants,
    [Windows.Automation.Condition]::TrueCondition
  )
  $records = New-Object 'System.Collections.Generic.List[object]'
  for ($i = 0; $i -lt $items.Count; $i++) {
    try {
      $element = $items.Item($i)
      [void]$records.Add([pscustomobject]@{
        Element = $element
        Name = [string]$element.Current.Name
        Id = [string]$element.Current.AutomationId
        Type = [string]$element.Current.ControlType.ProgrammaticName
      })
    } catch { } # A transient WebView subtree may be replaced during hydration.
  }
  return @($records.ToArray())
}

function Ui-Find($root, [string]$name, [string]$automationId) {
  $records = @(Ui-Records $root)
  if ($automationId) {
    $match = @($records | Where-Object { $_.Id -ceq $automationId })
    if ($match.Count -eq 1) { return $match[0] }
  }
  if ($name) {
    $match = @($records | Where-Object { $_.Name -ceq $name })
    if ($match.Count -eq 1) { return $match[0] }
  }
  return $null
}

function Ui-Require($root, [string]$name, [string]$automationId) {
  $record = Ui-Find $root $name $automationId
  if ($null -eq $record) {
    throw "UIA control not uniquely discoverable: name='$name' id='$automationId'."
  }
  return $record
}

function Ui-RequireUniqueEdit($root, [string]$name) {
  # Svelte labels are often exposed by Chromium as a Text element and a
  # separately editable input with the same accessible name. Select the
  # actual Edit control and refuse zero or ambiguous matches.
  $matches = @((Ui-Records $root) | Where-Object {
    $_.Name -ceq $name -and $_.Type -ceq 'ControlType.Edit'
  })
  if ($matches.Count -ne 1) {
    throw "Expected one editable UIA control named '$name'; observed $($matches.Count)."
  }
  return $matches[0]
}

function Ui-InvokeModalCreateRoom($root) {
  # Both the sidebar shortcut and the dialog submit are called 'Create room'.
  # The dialog is later in the UIA document order and has the editable input.
  $until = [DateTime]::UtcNow.AddSeconds(6)
  do {
    $controls = @((Ui-Records $root) | Where-Object {
      $_.Name -ceq 'Create room' -and $_.Type -ceq 'ControlType.Button'
    })
    if ($controls.Count -eq 2 -and $controls[1].Element.Current.IsEnabled) {
      Ui-Invoke $controls[1]
      return
    }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $until)
  throw "Expected enabled dialog Create room button after sidebar shortcut; discovered $($controls.Count)."
}

function Ui-NativeConsentPreview($root, [string]$fileName, [string]$expectedPrivateRoot, [string]$expectedSha256, [long]$expectedByteCount = -1) {
  $items = @(Ui-Records $root)
  $taskIds = @($items | Where-Object {
    $_.Name -match '(?i)\btask\s+([0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12})'
  } | ForEach-Object {
    [regex]::Match($_.Name, '(?i)\btask\s+([0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12})').Groups[1].Value
  } | Select-Object -Unique)
  if ($taskIds.Count -ne 1) { throw 'Cannot uniquely extract task ID from the immutable file-action preview.' }
  $taskId = $taskIds[0]
  $paths = @($items | Where-Object {
    $_.Name -and $_.Name.EndsWith(('\' + $fileName), [StringComparison]::OrdinalIgnoreCase) -and
    [IO.Path]::IsPathRooted($_.Name)
  } | Select-Object -ExpandProperty Name -Unique)
  if ($paths.Count -ne 1) { throw 'Cannot uniquely read the full canonical path from the native preview.' }
  $canonical = [string]$paths[0]
  $normalize = { param([string]$s) $s.Replace('\\?\','').TrimEnd('\') }
  $rootPath = & $normalize $expectedPrivateRoot
  $previewPath = & $normalize $canonical
  if (-not $previewPath.StartsWith(($rootPath + '\'), [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Native preview path is outside this private file fixture; refusing OS approval.'
  }
  $visibleReadSha = @($items | Where-Object {
    $_.Name -match ('(?i)\bSHA-256\s+' + [regex]::Escape($expectedSha256) + '\b')
  })
  if ($expectedByteCount -ge 0) {
    $expectedCountAndSha = ('{0} bytes · SHA-256 {1}' -f $expectedByteCount, $expectedSha256)
    if ($visibleReadSha.Count -ne 0 -or
        @($items | Where-Object { $_.Name -ceq $expectedCountAndSha }).Count -ne 0) {
      throw 'Read file SHA-256 or byte count leaked into the renderer before native consent.'
    }
    if (-not @($items | Where-Object {
      $_.Name -match 'The file size and SHA-256 are private until the independent native OS consent dialog'
    }).Count) {
      throw 'Renderer did not warn that read metadata is available only in the native OS consent dialog.'
    }
  } elseif ($expectedSha256 -and $visibleReadSha.Count -eq 0) {
    throw 'Create-new immutable preview SHA-256 differs from expected fixture bytes.'
  }
  return [pscustomobject]@{ task_id=$taskId; canonical_path=$canonical }
}

function Ui-Invoke($record) {
  $patternObject = $null
  if ($record.Element.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$patternObject)) {
    try { ([Windows.Automation.InvokePattern]$patternObject).Invoke() }
    catch { throw "InvokePattern call failed on name='$($record.Name)', id='$($record.Id)': $($_.Exception.GetBaseException().Message)" }
    return
  }
  if ($record.Element.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$patternObject)) {
    try { ([Windows.Automation.ExpandCollapsePattern]$patternObject).Expand() }
    catch { throw "ExpandCollapsePattern call failed on name='$($record.Name)', id='$($record.Id)': $($_.Exception.GetBaseException().Message)" }
    return
  }
  if ($record.Element.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$patternObject)) {
    try { ([Windows.Automation.TogglePattern]$patternObject).Toggle() }
    catch { throw "TogglePattern call failed on name='$($record.Name)', id='$($record.Id)': $($_.Exception.GetBaseException().Message)" }
    return
  }
  if ($record.Element.TryGetCurrentPattern([Windows.Automation.LegacyIAccessiblePattern]::Pattern, [ref]$patternObject)) {
    try { ([Windows.Automation.LegacyIAccessiblePattern]$patternObject).DoDefaultAction() }
    catch { throw "LegacyIAccessiblePattern call failed on name='$($record.Name)', id='$($record.Id)': $($_.Exception.GetBaseException().Message)" }
    return
  }
  $patterns = @($record.Element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
  throw "UIA control cannot be activated: name='$($record.Name)', id='$($record.Id)', type='$($record.Type)', supported=$($patterns -join ',')."
}

function Ui-SetText($root, $record, [string]$value) {
  $patternObject = $null
  if (-not $record.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$patternObject)) {
    throw "UIA ValuePattern unavailable: name='$($record.Name)', id='$($record.Id)', type='$($record.Type)'."
  }
  $pattern = [Windows.Automation.ValuePattern]$patternObject
  if ($pattern.Current.IsReadOnly) { throw "UIA input is readonly: '$($record.Name)'." }
  $pattern.SetValue($value)
  if (-not $record.Element.Current.IsPassword) {
    # Svelte may re-render the HTML input after its input event. The old
    # AutomationElement and Current.Value can be stale immediately after
    # SetValue. Re-query the current UIA tree before checking its value.
    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    $observedLength = -1
    do {
      $fresh = Ui-Find $root $record.Name $record.Id
      if ($null -eq $fresh -and -not $record.Id -and $record.Type -ceq 'ControlType.Edit') {
        $edits = @((Ui-Records $root) | Where-Object {
          $_.Name -ceq $record.Name -and $_.Type -ceq 'ControlType.Edit'
        })
        if ($edits.Count -eq 1) { $fresh = $edits[0] }
      }
      if ($null -ne $fresh) {
        $readPatternObject = $null
        if ($fresh.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$readPatternObject)) {
          $read = [string]([Windows.Automation.ValuePattern]$readPatternObject).Current.Value
          $observedLength = $read.Length
          if ($read -ceq $value) { return }
        }
      }
      Start-Sleep -Milliseconds 120
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "UIA input did not preserve expected text: name='$($record.Name)', id='$($record.Id)', requested_chars=$($value.Length), observed_chars=$observedLength."
  }
  # Password values are deliberately inaccessible through UIA readback.
  Start-Sleep -Milliseconds 220
}

function Ui-Wait($root, [string]$name, [string]$id, [int]$seconds = 8) {
  $until = [DateTime]::UtcNow.AddSeconds($seconds)
  do {
    $record = Ui-Find $root $name $id
    if ($null -ne $record) { return $record }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $until)
  return $null
}

function Ui-WaitEnabled($root, [string]$name, [int]$seconds = 5) {
  $until = [DateTime]::UtcNow.AddSeconds($seconds)
  $record = $null
  do {
    $record = Ui-Find $root $name ''
    if ($null -ne $record -and $record.Element.Current.IsEnabled) { return $record }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $until)
  if ($null -eq $record) { throw "UIA submit control not discoverable: '$name'." }
  throw "UIA submit control remained disabled after text input: '$name'."
}

function Ui-SelectFirstOption($root, [string]$controlId, [string]$excludedName, [string]$fallbackChoice) {
  $combo = Ui-Require $root '' $controlId
  if ($combo.Type -ne 'ControlType.ComboBox') {
    throw "Expected UIA ComboBox for '$controlId', found '$($combo.Type)'."
  }
  # Set UIA focus only on an element whose owning native HWND was already
  # verified on the private desktop. SetFocus does not send global keyboard
  # input or switch the Windows input desktop.
  $focusState = 'not_requested'
  try {
    Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
    $combo.Element.SetFocus()
    Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
    $focusState = 'private_desktop_uia_focus_requested'
  } catch {
    $focusState = 'private_desktop_uia_focus_unavailable'
  }
  $expand = $null
  if ($combo.Element.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$expand)) {
    ([Windows.Automation.ExpandCollapsePattern]$expand).Expand()
  }
  $deadline = [DateTime]::UtcNow.AddSeconds(3)
  do {
    $records = @(Ui-Records $root)
    $options = @($records | Where-Object {
      $_.Type -match 'ListItem|MenuItem' -and $_.Name -and
      $_.Name -cne $excludedName -and $_.Name -notmatch '^Choose '
    })
    if ($options.Count) { break }
    Start-Sleep -Milliseconds 180
  } while ([DateTime]::UtcNow -lt $deadline)
  if (-not $options.Count) {
    # Native WebView2 may expose HTML select items only as children of its
    # control rather than descendants of the top-level Document.
    $children = $combo.Element.FindAll(
      [Windows.Automation.TreeScope]::Descendants,
      [Windows.Automation.Condition]::TrueCondition
    )
    foreach ($child in $children) {
      try {
        $name = [string]$child.Current.Name
        if ($name -and $name -cne $excludedName -and $name -notmatch '^Choose ' -and
            $child.Current.ControlType.ProgrammaticName -match 'ListItem|MenuItem') {
          $options += [pscustomobject]@{ Element = $child; Name = $name; Type = $child.Current.ControlType.ProgrammaticName; Id = [string]$child.Current.AutomationId }
        }
      } catch { }
    }
  }
  if (-not $options.Count) {
    # Chromium sometimes exposes a select's ValuePattern but does not expose
    # its ListItems on an unselected desktop. Try only a precisely known,
    # locally created fixture option, and require UIA to read back its value.
    # Never use SendInput: it can target the user's active desktop.
    $valueObject = $null
    $valueState = 'missing'
    if ($fallbackChoice -and $combo.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$valueObject)) {
      $valuePattern = [Windows.Automation.ValuePattern]$valueObject
      $valueState = if ($valuePattern.Current.IsReadOnly) { 'readonly' } else { 'writable' }
      if (-not $valuePattern.Current.IsReadOnly) {
        try {
          $valuePattern.SetValue($fallbackChoice)
          Start-Sleep -Milliseconds 200
          $fresh = Ui-Require $root '' $controlId
          $freshValue = $null
          if ($fresh.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$freshValue) -and
              ([string]([Windows.Automation.ValuePattern]$freshValue).Current.Value) -ceq $fallbackChoice) {
            return $fallbackChoice
          }
          $valueState = 'write_not_reflected'
        } catch {
          $valueState = 'write_rejected: ' + $_.Exception.GetBaseException().Message
        }
      }
    }
    $names = @($records | Where-Object { $_.Type -match 'ListItem|ComboBox|MenuItem' } | Select-Object -First 20 | ForEach-Object { "$($_.Type):$($_.Name)" })
    $patterns = @($combo.Element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
    throw "UIA select '$controlId' has no discoverable selectable option after three seconds; focus=$focusState; value_pattern=$valueState; patterns=$($patterns -join ','); observed $($names -join ' | ')."
  }
  $selected = $options[0]
  $itemPattern = $null
  if (-not $selected.Element.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$itemPattern)) {
    throw "UIA option '$($selected.Name)' in '$controlId' lacks SelectionItemPattern."
  }
  ([Windows.Automation.SelectionItemPattern]$itemPattern).Select()
  Start-Sleep -Milliseconds 150
  $current = Ui-Require $root '' $controlId
  if (-not $current.Element.Current.IsEnabled) { throw "Selected combo '$controlId' became disabled." }
  return $selected.Name
}

function Ui-SelectWorkflowPickerButton($root, [string]$stage, [string]$choiceType) {
  $stepTitles = @{ research = 'Research'; write = 'Write'; review = 'Review' }
  $stepIndices = @{ research = 0; write = 1; review = 2 }
  if (-not $stepTitles.ContainsKey($stage) -or $choiceType -notin @('Agent', 'Model')) {
    throw 'Unrecognized local workflow stage/choice type.'
  }
  $summaries = @((Ui-Records $root) | Where-Object {
    $_.Name -ceq 'Choose an agent or model using buttons' -and $_.Type -ceq 'ControlType.Button'
  })
  if ($summaries.Count -ne 3) {
    throw "Workflow accessible picker requires exactly three step summaries; discovered $($summaries.Count)."
  }
  $groupName = if ($choiceType -eq 'Agent') {
    'Agent choices for ' + $stepTitles[$stage]
  } else {
    'Model choices for ' + $stepTitles[$stage]
  }
  $group = Ui-Find $root $groupName ''
  if ($null -eq $group) {
    # Expand only when closed. The agent/model buttons share one <details>;
    # blindly invoking its summary for the second choice would close it.
    $stepSummary = $summaries[$stepIndices[$stage]]
    # UIA Invoke/ExpandCollapse remains inside the private-desktop HWND.
    Ui-Invoke $stepSummary
    $group = Ui-Wait $root $groupName '' 6
  }
  if ($null -eq $group) { throw "Workflow picker group '$groupName' not accessible after expand." }
  $buttons = @((Ui-Records $group.Element) | Where-Object {
    $_.Type -ceq 'ControlType.Button' -and $_.Name
  })
  # Each run creates exactly one throwaway agent/provider in a fresh isolated
  # DB. Refuse an ambiguous set rather than accidentally choosing a real API.
  if ($buttons.Count -ne 1) {
    throw "Workflow picker '$groupName' exposes $($buttons.Count) buttons; expected exactly one local option."
  }
  $selectedLabel = $buttons[0].Name
  Ui-Invoke $buttons[0]
  Start-Sleep -Milliseconds 180
  return $selectedLabel
}

function Ui-Discovery($root, [int]$maxItems = 55) {
  $records = @(Ui-Records $root)
  # Never output entered passphrases or pasted data as diagnostic records.
  return @($records | Where-Object {
    $_.Type -match 'Button|Edit|Document|Text|Group' -and
    ($_.Name -or $_.Id) -and
    $_.Name -notmatch 'passphrase|password'
  } | Select-Object -First $maxItems @{N='name';E={$_.Name.Substring(0,[Math]::Min(115,$_.Name.Length))}},@{N='id';E={$_.Id}},@{N='type';E={$_.Type}})
}

function Capture-Window([IntPtr]$window, [string]$stage) {
  $rectangle = New-Object ConstellationEvidenceWindow+Rect
  if (-not [ConstellationEvidenceWindow]::GetWindowRect($window, [ref]$rectangle)) {
    throw "GetWindowRect failed for stage '$stage'."
  }
  $width = $rectangle.Right - $rectangle.Left
  $height = $rectangle.Bottom - $rectangle.Top
  if ($width -lt 400 -or $height -lt 300 -or $width -gt 8192 -or $height -gt 8192) {
    throw "Invalid native window crop $width x $height."
  }
  $bitmap = New-Object Drawing.Bitmap($width, $height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $graphics = [Drawing.Graphics]::FromImage($bitmap)
  try {
    $dc = $graphics.GetHdc()
    try {
      $painted = [ConstellationEvidenceWindow]::PrintWindow($window, $dc, 2)
      if (-not $painted) { $painted = [ConstellationEvidenceWindow]::PrintWindow($window, $dc, 0) }
    } finally { $graphics.ReleaseHdc($dc) }
    if (-not $painted) { throw "PrintWindow failed for stage '$stage'." }
    $sample = New-Object 'System.Collections.Generic.HashSet[int]'
    foreach ($y in 0..15) {
      foreach ($x in 0..15) {
        [void]$sample.Add($bitmap.GetPixel(
          [Math]::Min($width - 1, [int](($x + .5) * $width / 16)),
          [Math]::Min($height - 1, [int](($y + .5) * $height / 16))
        ).ToArgb())
      }
    }
    $file = Join-Path $artifacts ("$stage-$runId.png")
    $bitmap.Save($file, [Drawing.Imaging.ImageFormat]::Png)
    return [pscustomobject]@{
      stage = $stage
      path = $file
      width = $width
      height = $height
      bytes = (Get-Item -LiteralPath $file).Length
      sha256 = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
      distinct_sample_colors = $sample.Count
      content_plausible = ($sample.Count -gt 5)
    }
  } finally {
    $graphics.Dispose()
    $bitmap.Dispose()
  }
}

# This immutable, small fixture has one distinctive expected source sentence.
# Search results must expose an exact source slice with matching offsets + hash.
$source = "The hippocampus supports spatial memory in this isolated fixture.`nThe cerebellum coordinates movement in a different sentence.`n"
$searchTerms = 'hippocampus spatial memory'
$requiredSentence = 'The hippocampus supports spatial memory in this isolated fixture.'
$sourceBytes = [Text.Encoding]::UTF8.GetBytes($source)
$sourceSha = [BitConverter]::ToString(
  [Security.Cryptography.SHA256]::Create().ComputeHash($sourceBytes)
).Replace('-', '').ToLowerInvariant()
if ($sourceBytes.Length -gt 128 * 1024) { throw 'Fixture exceeds native document limit.' }

$result = [ordered]@{
  run_id = $runId
  exe_path = $exe
  exe_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
  isolation = 'not_verified'
  verified_window_desktop = $null
  isolated_data_directory = $isolatedData
  pid = $null
  hwnd = $null
  initial_uia_descendants = 0
  evidence_discovery = @()
  evidence_status = 'not_run'
  fixture_source_sha256 = $sourceSha
  observed_quote = $null
  observed_byte_range = $null
  observed_source_sha256 = $null
  backup_status = if ($SkipBackup) { 'skipped_by_request' } else { 'not_run' }
  restore_status = if ($SkipBackup) { 'skipped_by_request' } else { 'not_run' }
  backup_file = $null
  recovered_file = $null
  project_archive_status = if ($TestProjectArchive) { 'not_run' } else { 'skipped_by_request' }
  project_restore_status = if ($TestProjectArchive) { 'not_run' } else { 'skipped_by_request' }
  project_archive_file = $null
  project_recovered_directory = $null
  workflow_status = if ($TestWorkflows) { 'not_run' } else { 'skipped_by_request' }
  draft_status = if ($TestDrafts) { 'not_run' } else { 'skipped_by_request' }
  workflow_discovery = @()
  workflow_after_action_discovery = @()
  workflow_plan_id = $null
  workflow_step_status = if ($TestWorkflowStep) { 'not_run' } else { 'skipped_by_request' }
  workflow_report_status = if ($TestWorkflowReport) { 'not_run' } else { 'skipped_by_request' }
  workflow_report_observed_chars = 0
  workflow_fixture_request_count = 0
  workflow_fixture_rejected_count = 0
  file_tool_status = if ($TestFileTools) { 'not_run' } else { 'skipped_by_request' }
  file_tool_create_status = 'not_run'
  file_tool_reject_status = 'not_run'
  file_tool_read_status = 'not_run'
  file_tool_no_clobber_status = 'not_run'
  file_tool_journal_status = 'not_run'
  file_tool_artifact_sha256 = $null
  file_tool_journal_file_count = 0
  native_consent_status = if ($TestNativeConsent) { 'not_run' } else { 'skipped_by_request' }
  native_consent_no_status = 'not_run'
  native_consent_create_yes_status = 'not_run'
  native_consent_read_yes_status = 'not_run'
  native_read_intent_no_status = 'not_run'
  native_read_intent_yes_status = 'not_run'
  native_read_intent_mutation_status = 'not_run'
  native_read_intent_dialog_count = 0
  native_consent_read_mutation_status = 'not_run'
  native_consent_no_clobber_status = 'not_run'
  native_consent_reviewed_bytes = 0
  native_consent_dialog_count = 0
  native_consent_operation_ids = @()
  native_consent_operation_digests = @()
  native_consent_journal_status = 'not_run'
  native_consent_journal_record_count = 0
  native_consent_source_sha256 = if ($TestNativeConsent) { $ExpectedNativeSourceSha256.ToLowerInvariant() } else { $null }
  native_consent_source_manifest_sha256 = $expectedManifestHash
  screenshots = @()
  process_closed = $null
  blockers = @()
}
$process = $null
$hwnd = [IntPtr]::Zero
$workflowFixture = $null
try {
  # A stale executable without the native flag would ignore the argument and
  # potentially open the real user's database. Refuse it before launch.
  if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe)).Contains('--isolated-data-dir')) {
    throw 'Safety blocker: built executable has no --isolated-data-dir support; nothing launched.'
  }
  if ($ExpectedExeSha256 -and (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
    throw 'Executable changed before hidden-desktop launch; refusing execution.'
  }
  if ($TestNativeConsent -and
      (Get-FileHash -LiteralPath $nativeSource -Algorithm SHA256).Hash -ine $ExpectedNativeSourceSha256) {
    throw 'Native consent source changed before hidden-desktop launch; refusing execution.'
  }
  if ($TestNativeConsent -and (Assert-PinnedConstellationSources $ExpectedSourceIdentityPath $ExpectedNativeSourceSha256) -cne $expectedManifestHash) {
    throw 'Pinned source manifest or one of its eleven files changed before launch.'
  }
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName = $exe
  $psi.Arguments = '--isolated-data-dir "' + $isolatedData + '"'
  $psi.WorkingDirectory = $exeInfo.DirectoryName
  $psi.UseShellExecute = $false
  $psi.EnvironmentVariables['TEMP'] = Join-Path $sandbox 'Temp'
  $psi.EnvironmentVariables['TMP'] = Join-Path $sandbox 'Temp'
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $sandbox 'WebView2'
  # APPDATA and USERPROFILE deliberately untouched.
  $process = New-Object Diagnostics.Process
  $process.StartInfo = $psi
  Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
  if (-not $process.Start()) { throw 'Could not start selected Constellation executable.' }
  $result.pid = $process.Id

  $until = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  $root = $null
  do {
    $process.Refresh()
    if ($process.HasExited) { throw "App exited before UIA readiness (code $($process.ExitCode))." }
    $hwnd = [ConstellationEvidenceWindow]::FindOwnMainWindow($process.Id)
    if ($hwnd -ne [IntPtr]::Zero) {
      Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
      $result.verified_window_desktop = $HiddenDesktopWorkerName
      try {
        $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
        $records = @(Ui-Records $root)
        if ($records.Count -ge 10 -and
            @($records | Where-Object { $_.Name -ceq 'Evidence' }).Count -eq 1 -and
            @($records | Where-Object { $_.Name -ceq 'Settings' }).Count -eq 1) { break }
      } catch { }
    }
    Start-Sleep -Milliseconds 250
  } while ([DateTime]::UtcNow -lt $until)
  if ($null -eq $root -or $hwnd -eq [IntPtr]::Zero -or $records.Count -lt 10) {
    throw 'WebView2 UIA readiness failed: missing root or expected navigation controls.'
  }
  $result.hwnd = $hwnd.ToInt64()
  $result.initial_uia_descendants = $records.Count
  $database = Join-Path $isolatedData 'constellation.sqlite3'
  if (-not (Test-Path -LiteralPath $database -PathType Leaf)) {
    throw 'Isolation refused: no new SQLite database at the native isolated-data-dir path.'
  }
  $result.isolation = 'verified_native_sqlite_created'

  # Discover controls from the current UI Automation tree BEFORE interacting.
  $evidenceButton = Ui-Require $root 'Evidence' ''
  Ui-Invoke $evidenceButton
  $document = Ui-Wait $root 'Document' 'evidence-document' 10
  $queryInput = Ui-Wait $root 'Search terms' 'evidence-query' 10
  # The current Evidence form labels the actionable submit button explicitly.
  # Never accept the non-actionable workflow navigation text 'Find passages'.
  $searchButton = Ui-Wait $root 'Find in current text' '' 10
  $result.evidence_discovery = @(Ui-Discovery $root)
  if ($null -eq $document -or $null -eq $queryInput -or $null -eq $searchButton) {
    $result.evidence_status = 'blocked_controls_missing'
    $result.blockers += 'Evidence navigation worked, but document/query/submit control not uniquely found by current UIA Name or AutomationId.'
  } else {
    try {
      Ui-SetText $root $document $source
      Ui-SetText $root $queryInput $searchTerms
      Ui-Invoke $searchButton
      $deadline = [DateTime]::UtcNow.AddSeconds(12)
      do {
        $hits = @(Ui-Records $root)
        $names = @($hits | ForEach-Object { $_.Name } | Where-Object { $_ })
        $rangeText = @($names | Where-Object { $_ -match 'UTF-8 bytes \[\d+,\s*\d+\)' })
        if ($rangeText.Count -gt 0) { break }
        Start-Sleep -Milliseconds 250
      } while ([DateTime]::UtcNow -lt $deadline)
      if (-not $rangeText.Count) {
        throw 'No UIA evidence-result byte-range control appeared after invoking Find passages.'
      }
      # The app displays byte-range, a source SHA256 and exact quote. Check
      # that all three correspond to the submitted UTF-8 bytes, not just a
      # generic success toast.
      $rangeMatch = [regex]::Match(($rangeText -join ' '), 'UTF-8 bytes \[(\d+),\s*(\d+)\)')
      if (-not $rangeMatch.Success) { throw 'UIA byte-range label not parseable.' }
      $start = [int]$rangeMatch.Groups[1].Value
      $end = [int]$rangeMatch.Groups[2].Value
      if ($start -lt 0 -or $end -le $start -or $end -gt $sourceBytes.Length) {
        throw "UIA source byte range [$start,$end) is outside fixture."
      }
      $expectedQuote = [Text.Encoding]::UTF8.GetString($sourceBytes, $start, $end - $start)
      $result.observed_byte_range = "[$start,$end)"
      $result.observed_quote = $expectedQuote
      if (-not $expectedQuote.Contains($requiredSentence)) {
        throw 'Byte range did not span the known hippocampus source sentence.'
      }
      if (-not @($names | Where-Object { $_ -ceq $expectedQuote }).Count) {
        throw 'Exact byte-sliced quote was not present as a UIA text element (could be a WebView2 accessibility blocker).'
      }
      # The current Evidence UI hides full source identity inside a disclosure.
      # Open the actual native accessibility toggle before checking the digest;
      # absence of this independently visible control is a hard failure.
      $shaDisclosures = @((Ui-Records $root) | Where-Object {
        $_.Type -ceq 'ControlType.Button' -and $_.Name -ceq 'Check exact source SHA-256'
      })
      if ($shaDisclosures.Count -ne 1) {
        throw "Expected one source-hash disclosure; found $($shaDisclosures.Count)."
      }
      Ui-Invoke $shaDisclosures[0]
      $shaLabel = @()
      $shaUntil = [DateTime]::UtcNow.AddSeconds(6)
      do {
        $shaRecords = @(Ui-Records $root)
        $names = @($shaRecords | ForEach-Object { $_.Name } | Where-Object { $_ })
        $shaLabel = @($names | Where-Object { $_ -match '^Source SHA-256:\s*[0-9a-fA-F]{64}$' } | Select-Object -First 1)
        if ($shaLabel.Count) { break }
        Start-Sleep -Milliseconds 180
      } while ([DateTime]::UtcNow -lt $shaUntil)
      if (-not $shaLabel.Count) {
        $result.evidence_hash_uia_samples = @($shaRecords | Where-Object { $_.Name -match '(?i)source|sha-256|verification' } |
          Select-Object -First 16 | ForEach-Object { $_.Type + ': ' + $_.Name })
        throw 'Evidence SHA-256 label missing from UIA tree after expanding its real disclosure.'
      }
      $seenSha = ($shaLabel[0] -replace '^Source SHA-256:\s*', '').ToLowerInvariant()
      $result.observed_source_sha256 = $seenSha
      if ($seenSha -cne $sourceSha) { throw 'Displayed SHA256 does not equal fixture SHA256.' }
      if (-not @($names | Where-Object { $_ -match '\bmatching passages\b' }).Count) {
        throw 'Evidence heading does not confirm matching passages.'
      }
      $result.evidence_status = 'pass_exact_source_quote_span_hash'
    } catch {
      $result.evidence_status = 'blocked_or_failed_uia_interaction'
      $result.blockers += $_.Exception.Message
    }
  }
  try { $result.screenshots += Capture-Window $hwnd 'evidence' }
  catch { $result.blockers += "Evidence screenshot: $($_.Exception.Message)" }

  if (-not $SkipBackup) {
    try {
      Ui-Invoke (Ui-Require $root 'Settings' '')
      $advanced = Ui-Wait $root 'Advanced Studio' '' 8
      if ($null -eq $advanced) { throw 'Advanced Studio UIA control not found.' }
      Ui-Invoke $advanced
      $newPass = Ui-Wait $root 'New backup passphrase' 'backup-password' 8
      $confirmPass = Ui-Wait $root 'Confirm backup passphrase' 'backup-confirm' 8
      $create = Ui-Wait $root 'Create encrypted backup' '' 8
      if ($null -eq $newPass -or $null -eq $confirmPass -or $null -eq $create) {
        throw 'Backup form input/control not discoverable through UIA.'
      }
      $throwawayPassphrase = 'QA-' + [Guid]::NewGuid().ToString('N')
      Ui-SetText $root $newPass $throwawayPassphrase
      Ui-SetText $root $confirmPass $throwawayPassphrase
      Ui-Invoke (Ui-WaitEnabled $root 'Create encrypted backup' 5)
      $backupFolder = Join-Path $isolatedData 'backups-and-recovery'
      $deadline = [DateTime]::UtcNow.AddSeconds(20)
      do {
        $packages = @(Get-ChildItem -LiteralPath $backupFolder -Filter '*.cstlbkp' -File -ErrorAction SilentlyContinue)
        if ($packages.Count -eq 1 -and $packages[0].Length -gt 0) { break }
        Start-Sleep -Milliseconds 250
      } while ([DateTime]::UtcNow -lt $deadline)
      if ($packages.Count -ne 1 -or $packages[0].Length -le 0) {
        throw 'No new encrypted .cstlbkp found in isolated recovery directory.'
      }
      $result.backup_file = $packages[0].FullName
      $success = Ui-Wait $root 'Encrypted backup created' '' 5
      if ($null -eq $success) { throw 'Encrypted file exists, but UIA success confirmation not accessible.' }
      $result.backup_status = 'pass_isolated_encrypted_file_and_uia_confirmation'
      try { $result.screenshots += Capture-Window $hwnd 'backup' }
      catch { $result.blockers += "Backup screenshot: $($_.Exception.Message)" }

      $restorePath = Ui-Wait $root 'Full path to the encrypted backup' 'restore-path' 8
      $restorePassword = Ui-Wait $root 'Original backup passphrase' 'restore-password' 8
      $restore = Ui-Wait $root 'Verify and recover to a new file' '' 8
      if ($null -eq $restorePath -or $null -eq $restorePassword -or $null -eq $restore) {
        throw 'Restore form controls not discoverable through UIA.'
      }
      Ui-SetText $root $restorePath $packages[0].FullName
      Ui-SetText $root $restorePassword $throwawayPassphrase
      Ui-Invoke (Ui-WaitEnabled $root 'Verify and recover to a new file' 5)
      $deadline = [DateTime]::UtcNow.AddSeconds(20)
      do {
        $restored = @(Get-ChildItem -LiteralPath $backupFolder -Filter 'constellation-restored-*.sqlite3' -File -ErrorAction SilentlyContinue)
        if ($restored.Count -eq 1 -and $restored[0].Length -gt 0) { break }
        Start-Sleep -Milliseconds 250
      } while ([DateTime]::UtcNow -lt $deadline)
      if ($restored.Count -ne 1 -or $restored[0].Length -le 0) {
        throw 'No separately restored SQLite found after UIA recovery request.'
      }
      $result.recovered_file = $restored[0].FullName
      $restoredNotice = Ui-Wait $root 'Recovery file created' '' 5
      if ($null -eq $restoredNotice) { throw 'Restored file exists but UIA confirmation is not accessible.' }
      $result.restore_status = 'pass_isolated_separate_sqlite_and_uia_confirmation'
      try { $result.screenshots += Capture-Window $hwnd 'recovery' }
      catch { $result.blockers += "Recovery screenshot: $($_.Exception.Message)" }
    } catch {
      if ($result.backup_status -ne 'pass_isolated_encrypted_file_and_uia_confirmation') {
        $result.backup_status = 'blocked_or_failed_uia_interaction'
      } else {
        $result.restore_status = 'blocked_or_failed_uia_interaction'
      }
      $result.blockers += $_.Exception.Message
      try { $result.screenshots += Capture-Window $hwnd 'backup-blocker' }
      catch { $result.blockers += "Backup blocker screenshot: $($_.Exception.Message)" }
    } finally {
      $throwawayPassphrase = $null
    }
  }
  if ($TestProjectArchive) {
    $projectQaPassphrase = 'Constellation-QA-' + [Guid]::NewGuid().ToString('N')
    try {
      if ($null -eq (Ui-Wait $root 'Create full project archive' '' 2)) {
        $settingsButtons = @((Ui-Records $root) | Where-Object {
          $_.Name -ceq 'Settings' -and $_.Type -ceq 'ControlType.Button'
        })
        if ($settingsButtons.Count -ne 1) {
          throw 'Settings navigation button is not uniquely discoverable.'
        }
        Ui-Invoke $settingsButtons[0]
        if ($null -eq (Ui-Wait $root 'Create full project archive' '' 2)) {
          Ui-Invoke (Ui-Require $root 'Advanced Studio' '')
        }
      }
      $projectInput = Ui-Wait $root 'Project archive passphrase' 'project-passphrase' 8
      $projectConfirm = Ui-Wait $root 'Confirm project archive passphrase' 'project-confirm' 8
      if ($null -eq $projectInput -or $null -eq $projectConfirm) {
        throw 'Full-project archive passphrase inputs are inaccessible.'
      }
      Ui-SetText $root $projectInput $projectQaPassphrase
      Ui-SetText $root $projectConfirm $projectQaPassphrase
      Ui-Invoke (Ui-WaitEnabled $root 'Create full project archive' 6)
      $backupFolder = Join-Path $isolatedData 'backups-and-recovery'
      $until = [DateTime]::UtcNow.AddSeconds(25)
      do {
        $projectPackages = @(Get-ChildItem -LiteralPath $backupFolder -Filter '*.cstlproj' -File -ErrorAction SilentlyContinue)
        if ($projectPackages.Count -eq 1 -and $projectPackages[0].Length -gt 120) { break }
        Start-Sleep -Milliseconds 200
      } while ([DateTime]::UtcNow -lt $until)
      if ($projectPackages.Count -ne 1 -or $projectPackages[0].Length -le 120) {
        throw 'No complete encrypted .cstlproj package was written.'
      }
      if ($null -eq (Ui-Wait $root 'Both databases archived' '' 6)) {
        throw 'The native project archive exists, but the UI did not confirm both databases.'
      }
      $result.project_archive_file = $projectPackages[0].FullName
      $result.project_archive_status = 'pass_two_db_encrypted_file_and_uia_confirmation'
      try { $result.screenshots += Capture-Window $hwnd 'project-archive' }
      catch { $result.blockers += "Project archive screenshot: $($_.Exception.Message)" }

      $restoreInput = Ui-Wait $root 'Full project archive path' 'project-restore-path' 8
      $restorePass = Ui-Wait $root 'Original project archive passphrase' 'project-restore-passphrase' 8
      if ($null -eq $restoreInput -or $null -eq $restorePass) {
        throw 'Full-project recovery form is inaccessible.'
      }
      Ui-SetText $root $restoreInput $projectPackages[0].FullName
      Ui-SetText $root $restorePass $projectQaPassphrase
      Ui-Invoke (Ui-WaitEnabled $root 'Recover full project to new directory' 6)
      $until = [DateTime]::UtcNow.AddSeconds(25)
      do {
        $projectDirs = @(Get-ChildItem -LiteralPath $backupFolder -Filter 'restored-project-*' -Directory -ErrorAction SilentlyContinue)
        if ($projectDirs.Count -eq 1 -and
            (Test-Path -LiteralPath (Join-Path $projectDirs[0].FullName 'studio.sqlite3') -PathType Leaf) -and
            (Test-Path -LiteralPath (Join-Path $projectDirs[0].FullName 'workflows.sqlite3') -PathType Leaf)) { break }
        Start-Sleep -Milliseconds 200
      } while ([DateTime]::UtcNow -lt $until)
      if ($projectDirs.Count -ne 1) { throw 'No complete new project restore directory found.' }
      foreach ($name in @('studio.sqlite3', 'workflows.sqlite3')) {
        $db = Join-Path $projectDirs[0].FullName $name
        if (-not (Test-Path -LiteralPath $db -PathType Leaf)) { throw "Recovered database $name missing." }
        $stream = [IO.File]::OpenRead($db)
        try {
          $header = New-Object byte[] 16
          if ($stream.Read($header, 0, 16) -ne 16 -or
              [Text.Encoding]::ASCII.GetString($header) -cne "SQLite format 3`0") {
            throw "Recovered database $name does not have a SQLite header."
          }
        } finally { $stream.Dispose() }
      }
      if ($null -eq (Ui-Wait $root 'Full project recovered' '' 6)) {
        throw 'Recovered both database files but the native UI did not confirm recovery.'
      }
      $result.project_recovered_directory = $projectDirs[0].FullName
      $result.project_restore_status = 'pass_two_verified_sqlite_files_in_new_directory'
      try { $result.screenshots += Capture-Window $hwnd 'project-recovery' }
      catch { $result.blockers += "Project recovery screenshot: $($_.Exception.Message)" }
    } catch {
      if ($result.project_archive_status -ne 'pass_two_db_encrypted_file_and_uia_confirmation') {
        $result.project_archive_status = 'blocked_or_failed_uia_interaction'
      } else {
        $result.project_restore_status = 'blocked_or_failed_uia_interaction'
      }
      $result.blockers += "Full project archive: $($_.Exception.Message)"
      try { $result.screenshots += Capture-Window $hwnd 'project-archive-blocker' }
      catch { $result.blockers += "Project blocker screenshot: $($_.Exception.Message)" }
    } finally {
      $projectQaPassphrase = $null
    }
  }
  if ($TestWorkflows) {
    try {
      if ($TestWorkflowStep) {
        . (Join-Path $PSScriptRoot 'helper-local-workflow-fixture.ps1')
        $workflowFixture = New-Object ConstellationLocalWorkflowFixture
        if (-not $workflowFixture.Port -or $workflowFixture.RequestCount -ne 0) {
          throw 'Isolated localhost workflow fixture could not start safely.'
        }
      }
      # Set up *only* this throwaway profile using user-facing UI controls.
      # Saving a local endpoint profile never sends an inference request.
      Ui-Invoke (Ui-Require $root 'Library' '')
      Ui-Invoke (Ui-WaitEnabled $root 'Create agent' 7)
      Ui-Invoke (Ui-WaitEnabled $root 'Add teammate' 7)
      if ($null -eq (Ui-Wait $root 'Research agent' '' 1)) {
        # The role is arbitrary for the test; the workflow must derive it from
        # whichever real saved agent the UI assigns later.
        Start-Sleep -Milliseconds 250
      }
      Ui-Invoke (Ui-Require $root 'Settings' '')
      Ui-Invoke (Ui-WaitEnabled $root 'Add provider' 8)
      if ($TestWorkflowStep) {
        $name = Ui-Wait $root 'DISPLAY NAME' 'provider-name' 8
        $endpoint = Ui-Wait $root 'API BASE URL' 'provider-endpoint' 8
        if ($null -eq $name -or $null -eq $endpoint) {
          throw 'Cannot guarantee a loopback-only workflow provider; refusing step dispatch.'
        }
        Ui-SetText $root $name 'Constellation localhost QA fixture'
        Ui-SetText $root $endpoint $workflowFixture.Url
      }
      $model = Ui-Wait $root 'MODEL IDENTIFIER' 'provider-model' 8
      if ($null -eq $model) { throw 'Model identifier input missing in local provider dialog.' }
      Ui-SetText $root $model 'workflow-local-fixture-no-model'
      Ui-Invoke (Ui-WaitEnabled $root 'Save connection locally' 6)
      Ui-Invoke (Ui-Require $root 'Tasks' '')
      Ui-Invoke (Ui-WaitEnabled $root 'Workflows' 8)
      $goal = Ui-Wait $root 'What should the team accomplish?' 'workflow-goal' 8
      $result.workflow_discovery = @(Ui-Discovery $root)
      if ($null -eq $goal) { throw 'Tasks > Workflows goal textarea not discoverable.' }
      Ui-SetText $root $goal 'Summarize the evidence of hippocampal involvement in spatial memory. Offline planning fixture only.'
      $nativePickerAvailable = @((Ui-Records $root) | Where-Object {
        $_.Name -ceq 'Choose an agent or model using buttons' -and $_.Type -ceq 'ControlType.Button'
      }).Count -eq 3
      foreach ($stage in @('research','write','review')) {
        if ($nativePickerAvailable) {
          [void](Ui-SelectWorkflowPickerButton $root $stage 'Agent')
          $selectedProvider = Ui-SelectWorkflowPickerButton $root $stage 'Model'
        } else {
          $expectedAgentLabel = 'Literature Scout ' + [char]0xB7 + ' Literature Scout'
          $expectedProviderLabel = 'Constellation localhost QA fixture ' + [char]0xB7 + ' workflow-local-fixture-no-model'
          [void](Ui-SelectFirstOption $root "workflow-agent-$stage" 'Choose an agent' $expectedAgentLabel)
          $selectedProvider = Ui-SelectFirstOption $root "workflow-provider-$stage" 'Choose explicitly' $expectedProviderLabel
        }
        if ($TestWorkflowStep -and -not $selectedProvider.Contains('Constellation localhost QA fixture')) {
          throw 'Provider selector did not select the unique localhost fixture; refusing workflow execution.'
        }
      }
      Ui-Invoke (Ui-WaitEnabled $root 'Preview dependencies' 8)
      if ($null -eq (Ui-Wait $root 'Save local plan' '' 7)) { throw 'Preview did not render Save local plan control.' }
      if (-not @((Ui-Records $root) | Where-Object { $_.Name -match '^Valid plan' }).Count) {
        throw 'Preview did not expose a validated local plan through UIA.'
      }
      Ui-Invoke (Ui-WaitEnabled $root 'Save local plan' 8)
      Start-Sleep -Milliseconds 350
      $result.workflow_after_action_discovery = @(Ui-Discovery $root 145)
      # Chromium UIA may expose the CSS-capitalized visual string 'Saved Plan'
      # even when the Svelte DOM source spells it 'Saved plan'.
      $saved = Ui-Wait $root 'Saved Plan' '' 3
      if ($null -eq $saved) { $saved = Ui-Wait $root 'Saved plan' '' 5 }
      if ($null -eq $saved) { throw 'Saved workflow did not appear with the expected pending status.' }
      $workflowRecords = @(Ui-Records $root)
      $idRecord = @($workflowRecords | Where-Object { $_.Name -match '^workflow-[0-9a-f]{8}-' } | Select-Object -First 1)
      if (-not $idRecord.Count) { throw 'Saved workflow ID not discoverable in UIA list.' }
      $result.workflow_plan_id = $idRecord[0].Name
      # Verify the real native search control, not merely a source contract.
      # It must locate the durable workflow by ID, produce a real no-match
      # state, and recover after clearing the filter. No provider is called.
      $findWorkflow = Ui-Wait $root 'Find workflow ID' 'workflow-id-search' 8
      if ($null -eq $findWorkflow) { throw 'Native workflow history ID search control missing.' }
      Ui-SetText $root $findWorkflow 'qa-unmatched-workflow-20261102'
      Ui-Invoke (Ui-WaitEnabled $root 'Search' 8)
      if ($null -eq (Ui-Wait $root 'No saved workflows match that ID search.' '' 8)) {
        throw 'Native workflow search did not expose a real no-match state.'
      }
      Ui-Invoke (Ui-WaitEnabled $root 'Clear filter' 8)
      $findWorkflow = Ui-Wait $root 'Find workflow ID' 'workflow-id-search' 8
      if ($null -eq $findWorkflow) { throw 'Workflow search input did not survive clearing the filter.' }
      Ui-SetText $root $findWorkflow $result.workflow_plan_id
      Ui-Invoke (Ui-WaitEnabled $root 'Search' 8)
      if ($null -eq (Ui-Wait $root $result.workflow_plan_id '' 8)) {
        throw 'Native workflow search failed to rediscover the saved workflow by ID.'
      }
      try { $result.screenshots += Capture-Window $hwnd 'workflow-saved' }
      catch { $result.blockers += "Workflow saved screenshot: $($_.Exception.Message)" }
      # Re-enter Tasks > Workflows to force a native list reload, not merely
      # rely on the transient optimistic save state.
      # On the Tasks screen both navigation and the local tab are named
      # "Tasks". Select the last button in document order (the view tab).
      $taskButtons = @((Ui-Records $root) | Where-Object { $_.Name -ceq 'Tasks' -and $_.Type -ceq 'ControlType.Button' })
      if ($taskButtons.Count -lt 2) { throw 'Could not distinguish Tasks navigation from the Tasks view tab.' }
      Ui-Invoke $taskButtons[$taskButtons.Count - 1]
      Ui-Invoke (Ui-WaitEnabled $root 'Workflows' 8)
      if ($null -eq (Ui-Wait $root $result.workflow_plan_id '' 7)) {
        throw 'Workflow ID not visible after leaving and reopening Workflows.'
      }
      if ($TestWorkflowStep) {
        # This UI action is explicitly authorized only after the provider is
        # proved to be our loopback TcpListener. It will send one synthetic
        # request, never a paid/external model or device tool invocation.
        # Current native UI nests per-step actions in a closed <details>.
        # Expand the unique actionable summary; never infer a hidden control
        # from its source or manufacture a provider response.
        $reviewSummaries = @((Ui-Records $root) | Where-Object {
          $_.Type -ceq 'ControlType.Button' -and
          $_.Name -cin @('Review steps, provider results and export', 'Review Steps, Provider Results And Export')
        })
        if ($reviewSummaries.Count -ne 1) {
          throw "Expected one saved workflow review summary; found $($reviewSummaries.Count)."
        }
        Ui-Invoke $reviewSummaries[0]
        $runStepLabel = 'Run text step ' + [char]0xB7 + ' may use provider credits'
        Ui-Invoke (Ui-WaitEnabled $root $runStepLabel 8)
        $stepDeadline = [DateTime]::UtcNow.AddSeconds(15)
        do {
          if ($workflowFixture.RequestCount -gt 1 -or $workflowFixture.RejectedCount -gt 0) {
            throw 'Loopback fixture observed unexpected or repeated requests.'
          }
          # CSS text-transform can capitalize this label in Windows UIA.
          $partial = Ui-Find $root 'Partly Completed' ''
          if ($null -eq $partial) { $partial = Ui-Find $root 'Partly completed' '' }
          if ($workflowFixture.RequestCount -eq 1 -and $null -ne $partial) { break }
          Start-Sleep -Milliseconds 200
        } while ([DateTime]::UtcNow -lt $stepDeadline)
        if ($workflowFixture.RequestCount -ne 1 -or $null -eq $partial) {
          throw 'Workflow first step did not complete via exactly one localhost request.'
        }
        # Chromium exposes HTML <summary> as both a Button and a child Text
        # with the same accessible Name. Select the one actionable Button.
        $details = $null
        $detailDeadline = [DateTime]::UtcNow.AddSeconds(8)
        do {
          $detailButtons = @((Ui-Records $root) | Where-Object {
            $_.Name -ceq 'View saved result for research' -and $_.Type -ceq 'ControlType.Button'
          })
          if ($detailButtons.Count -gt 1) { throw 'Ambiguous research result summary buttons.' }
          if ($detailButtons.Count -eq 1) { $details = $detailButtons[0]; break }
          Start-Sleep -Milliseconds 180
        } while ([DateTime]::UtcNow -lt $detailDeadline)
        if ($null -eq $details) { throw 'Completed workflow result summary button was not exposed by UIA.' }
        Ui-Invoke $details
        if ($null -eq (Ui-Wait $root ([ConstellationLocalWorkflowFixture]::Result) '' 5)) {
          throw 'Localhost fixture response was not saved/exposed in UIA result.'
        }
        $result.workflow_step_status = 'pass_one_loopback_request_persisted_text_result'
        try { $result.screenshots += Capture-Window $hwnd 'workflow-local-step' }
        catch { $result.blockers += "Workflow local step screenshot: $($_.Exception.Message)" }
      }
      Ui-Invoke (Ui-WaitEnabled $root 'Cancel pending steps' 8)
      # Cancellation is intentionally a separate, explicit confirmation.
      # Merely opening the confirmation is not a terminal native decision.
      $confirmCancel = Ui-WaitEnabled $root 'Confirm cancel pending steps' 8
      if ($confirmCancel.Type -cne 'ControlType.Button') {
        throw 'Workflow cancellation confirmation is not a native UIA button.'
      }
      # Do not claim cancellation from merely opening the confirmation. The
      # UIA pattern must really activate the confirmation; unsupported
      # Chromium ExpandCollapse action is a recorded release blocker.
      Ui-Invoke $confirmCancel
      if ($null -eq (Ui-Wait $root 'Cancelled' '' 8)) {
        throw 'Native workflow cancellation did not appear as Cancelled in UIA list.'
      }
      if ($TestWorkflowReport) {
        # The in-app report viewer renders the native export as accessible
        # text. Copy would modify WinSta0's clipboard, which is shared with
        # the active desktop, and must not be invoked by this test.
        Ui-Invoke (Ui-WaitEnabled $root 'View Markdown report' 8)
        $reportLabel = 'Saved Markdown report for ' + $result.workflow_plan_id
        $reportSection = Ui-Wait $root $reportLabel '' 8
        if ($null -eq $reportSection) {
          throw 'In-app Markdown export section was not exposed by UIA.'
        }
        $reportRecords = @(Ui-Records $reportSection.Element)
        $reportText = (@($reportSection.Name) + @($reportRecords | ForEach-Object { $_.Name } | Where-Object { $_ })) -join "`n"
        $result.workflow_report_observed_chars = $reportText.Length
        $requiredText = @('# Constellation workflow report', $result.workflow_plan_id,
          'Cancelled: true', '## Step: research', '## Step: write', '## Step: review')
        if ($TestWorkflowStep) {
          $requiredText += 'CONSTELLATION_LOCALHOST_WORKFLOW_E2E_SUCCESS'
          $requiredText += 'Saved text result (unverified provider output):'
        }
        foreach ($required in $requiredText) {
          if (-not $reportText.Contains($required)) {
            throw "In-app workflow Markdown export lacks '$required' (observed chars=$($reportText.Length))."
          }
        }
        $result.workflow_report_status = 'pass_native_markdown_rendered_and_checked_without_clipboard'
        try { $result.screenshots += Capture-Window $hwnd 'workflow-markdown-report' }
        catch { $result.blockers += "Workflow Markdown screenshot: $($_.Exception.Message)" }
      }
      $result.workflow_status = if ($TestWorkflowStep) {
        'pass_preview_save_reload_localhost_step_cancel'
      } else {
        'pass_preview_save_reload_cancel_without_provider_calls'
      }
      try { $result.screenshots += Capture-Window $hwnd 'workflow-cancelled' }
      catch { $result.blockers += "Workflow cancelled screenshot: $($_.Exception.Message)" }
    } catch {
      $result.workflow_status = 'blocked_or_failed_uia_interaction'
      if ($TestWorkflowStep -and $result.workflow_step_status -ne 'pass_one_loopback_request_persisted_text_result') {
        $result.workflow_step_status = 'blocked_or_failed_localhost_step'
      }
      if ($TestWorkflowReport -and $result.workflow_report_status -ne 'pass_native_markdown_rendered_and_checked_without_clipboard') {
        $result.workflow_report_status = 'blocked_or_failed_ui_markdown_export'
      }
      $result.workflow_after_action_discovery = @(Ui-Discovery $root 145)
      $result.blockers += "Workflow: $($_.Exception.Message)"
      try { $result.screenshots += Capture-Window $hwnd 'workflow-blocker' }
      catch { $result.blockers += "Workflow blocker screenshot: $($_.Exception.Message)" }
    }
  }
  if ($TestFileTools) {
    try {
      if ($TestNativeConsent) {
        . (Join-Path $PSScriptRoot 'helper-native-consent.ps1')
      }
      # A disposable room and the one agent created above provide a pending
      # task. All filesystem effects remain below this run's isolated Data
      # directory; no model/provider request or global keyboard input occurs.
      if (-not $TestWorkflows) {
        Ui-Invoke (Ui-Require $root 'Library' '')
        Ui-Invoke (Ui-WaitEnabled $root 'Create agent' 8)
        Ui-Invoke (Ui-WaitEnabled $root 'Add teammate' 8)
      }
      Ui-Invoke (Ui-Require $root 'Rooms' '')
      $newRoomButton = Ui-Wait $root 'Create your first room' '' 4
      if ($null -eq $newRoomButton) { $newRoomButton = Ui-Wait $root 'Create room' '' 4 }
      if ($null -eq $newRoomButton) { throw 'Isolated room creation control is not discoverable.' }
      Ui-Invoke $newRoomButton
      $roomName = Ui-RequireUniqueEdit $root 'ROOM NAME'
      Ui-SetText $root $roomName 'Constellation Private QA Room'
      Ui-InvokeModalCreateRoom $root
      Ui-Invoke (Ui-Require $root 'Tasks' '')
      # Route transitions are async; wait for the Tasks/Workflows sub-navigation
      # before enumerating it. With no saved tasks, the CTA is instead named
      # 'Plan your first task' (rather than 'Plan task').
      if ($null -eq (Ui-Wait $root 'Workflows' '' 8)) {
        throw 'Tasks/Workflows sub-navigation did not appear after opening Tasks.'
      }
      # The Tasks navigation does not reset the persistent Tasks/Workflows
      # subview. After the workflow E2E, the page remains on Workflows even
      # though Plan task can still be opened. Select the actual Tasks tab.
      $tasksButtons = @((Ui-Records $root) | Where-Object {
        $_.Name -ceq 'Tasks' -and $_.Type -ceq 'ControlType.Button'
      })
      if ($tasksButtons.Count -ge 2) {
        Ui-Invoke $tasksButtons[$tasksButtons.Count - 1]
      }
      $planButton = Ui-Wait $root 'Plan task' '' 4
      if ($null -eq $planButton) { $planButton = Ui-Wait $root 'Plan your first task' '' 8 }
      if ($null -eq $planButton) { throw 'Neither initial nor existing-task planning CTA is available.' }
      Ui-Invoke $planButton
      $instructions = Ui-RequireUniqueEdit $root 'INSTRUCTIONS'
      Ui-SetText $root $instructions 'Review one disposable local test file; do not invoke external providers.'
      Ui-Invoke (Ui-WaitEnabled $root 'Save pending task' 7)
      # Manual file actions are inside a collapsed native details element on
      # current builds. Select the exact unique actionable disclosure toggle.
      $fileActionSummaries = @((Ui-Records $root) | Where-Object {
        $_.Type -ceq 'ControlType.Button' -and
        $_.Name -cin @('Optional file actions · approval required', 'Optional File Actions · Approval Required')
      })
      if ($fileActionSummaries.Count -ne 1) {
        throw "Expected one optional native file action disclosure; found $($fileActionSummaries.Count)."
      }
      Ui-Invoke $fileActionSummaries[0]
      if ($null -eq (Ui-Wait $root 'Review a local file action' '' 9)) {
        throw 'Saved pending task does not expose manual local file controls.'
      }
      Ui-Invoke (Ui-WaitEnabled $root 'Review a local file action' 7)

      $privateFiles = Join-Path $isolatedData 'local-file-artifacts'
      $privateJournal = Join-Path $isolatedData 'local-tool-audit'
      if (-not (Test-Path -LiteralPath $privateFiles -PathType Container) -or
          -not (Test-Path -LiteralPath $privateJournal -PathType Container)) {
        throw 'Native private files and journal were not created under isolated Data.'
      }
      $runFile = 'acceptance-' + $runId.Substring(0, 12) + '.txt'
      $denyFile = 'denied-' + $runId.Substring(0, 12) + '.txt'
      $nativeNoFile = 'native-no-' + $runId.Substring(0, 12) + '.txt'
      $acceptedText = 'Local approval fixture: ' + $runId + "`n"
      if ($TestNativeConsent) {
        # Exercise a genuinely scrollable native preview/hex document rather
        # than claiming complete review from just four visible hex rows.
        # The repeated UTF-8 fixture crosses 16-byte row and viewport bounds;
        # tabs, newlines, and literal backslashes also test escaped preview.
        $unicodeProbe = ([string][char]0x03BB) + ([string][char]0x6F22)
        $reviewLine = "Review complete UTF-8 bytes: $unicodeProbe `t path\\segment 0123456789abcdef`n"
        $acceptedText += ($reviewLine * 64)
        if ([Text.Encoding]::UTF8.GetByteCount($acceptedText) -gt 16 * 1024) {
          throw 'Native scrollable review fixture exceeds the app content bound.'
        }
        $result.native_consent_reviewed_bytes = [Text.Encoding]::UTF8.GetByteCount($acceptedText)
      }
      $rejectedText = 'This operation must never write a file.'

      # Denied operations must leave no file behind and consume their approval.
      $filePath = Ui-Require $root '' 'file-tool-path'
      $fileContent = Ui-Require $root '' 'file-tool-content'
      Ui-SetText $root $filePath $denyFile
      Ui-SetText $root $fileContent $rejectedText
      Ui-Invoke (Ui-WaitEnabled $root 'Preview exact file action' 8)
      if ($null -eq (Ui-Wait $root 'Approval required for proposed file action' '' 8)) {
        throw 'Denied operation never displayed an immutable preview.'
      }
      if (Test-Path -LiteralPath (Join-Path $privateFiles $denyFile)) {
        throw 'An unapproved local file appeared before human denial.'
      }
      Ui-Invoke (Ui-WaitEnabled $root 'Reject without executing' 8)
      if ($null -eq (Ui-Wait $root 'The file action was rejected. No file operation was executed.' '' 7)) {
        throw 'Native file denial was not confirmed by UIA.'
      }
      if (Test-Path -LiteralPath (Join-Path $privateFiles $denyFile)) {
        throw 'A denied local file operation created a file.'
      }
      $result.file_tool_reject_status = 'pass_denial_confirmed_no_file_created'

      if ($TestNativeConsent) {
        # A distinct denial: renderer requests consent; the native scrollable
        # review must default to No and the owned No UIA button is chosen.
        Ui-SetText $root (Ui-Require $root '' 'file-tool-path') $nativeNoFile
        Ui-SetText $root (Ui-Require $root '' 'file-tool-content') $rejectedText
        Ui-Invoke (Ui-WaitEnabled $root 'Preview exact file action' 8)
        if ($null -eq (Ui-Wait $root 'Approval required for proposed file action' '' 8)) {
          throw 'Native-No scenario did not show the immutable file action preview.'
        }
        $denyDigest = [BitConverter]::ToString(
          [Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($rejectedText))
        ).Replace('-', '').ToLowerInvariant()
        $denyPreview = Ui-NativeConsentPreview $root $nativeNoFile $privateFiles $denyDigest
        $denyArgs = @{
          Root=$root; ApplicationWindow=$hwnd; ProcessId=$process.Id; DesktopName=$HiddenDesktopWorkerName
          Decision='No'; Action='create_new'; ExpectedPath=$denyPreview.canonical_path
          ExpectedTaskId=$denyPreview.task_id; ExpectedContentSha256=$denyDigest
          ExpectedContent=$rejectedText
        }
        $nativeNoResult = Invoke-ConstellationPrivateNativeConsent @denyArgs
        $result.native_consent_dialog_count++
        if ($nativeNoResult.screenshot) { $result.screenshots += $nativeNoResult.screenshot }
        $result.native_consent_operation_ids += $nativeNoResult.operation_id
        $result.native_consent_operation_digests += $nativeNoResult.operation_digest
        if ($nativeNoResult.default_button_id -ne 7 -or
            $nativeNoResult.verified_native_control_count -ne 8 -or
            -not $nativeNoResult.verified_full_exact_hex_bytes) {
          throw 'OS native denial lacked eight verified Win32 controls, default-No focus, or authoritative complete hex review.'
        }
        if ($null -eq (Ui-Wait $root 'You declined the native confirmation. No file action was executed.' '' 8)) {
          throw 'The renderer did not report the exact native No rejection receipt.'
        }
        if (Test-Path -LiteralPath (Join-Path $privateFiles $nativeNoFile)) {
          throw 'Native No in the owned Win32 consent review unexpectedly created a file.'
        }
        $result.native_consent_no_status = 'pass_owned_native_dialog_default_no_explicit_denial_no_file'
      }

      # New file: preview must precede native approval; compare exact bytes
      # read back from the isolated filesystem (including UTF-8 encoding).
      $filePath = Ui-Require $root '' 'file-tool-path'
      $fileContent = Ui-Require $root '' 'file-tool-content'
      Ui-SetText $root $filePath $runFile
      Ui-SetText $root $fileContent $acceptedText
      Ui-Invoke (Ui-WaitEnabled $root 'Preview exact file action' 8)
      if ($null -eq (Ui-Wait $root 'Approval required for proposed file action' '' 8)) {
        throw 'Create-new file immutable approval preview was not displayed.'
      }
      if (Test-Path -LiteralPath (Join-Path $privateFiles $runFile)) {
        throw 'Create-new operation wrote before approval.'
      }
      if ($TestNativeConsent) {
        $expectedDigest = [BitConverter]::ToString(
          [Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($acceptedText))
        ).Replace('-', '').ToLowerInvariant()
        $createPreview = Ui-NativeConsentPreview $root $runFile $privateFiles $expectedDigest
        $createArgs = @{
          Root=$root; ApplicationWindow=$hwnd; ProcessId=$process.Id; DesktopName=$HiddenDesktopWorkerName
          Decision='Yes'; Action='create_new'; ExpectedPath=$createPreview.canonical_path
          ExpectedTaskId=$createPreview.task_id; ExpectedContentSha256=$expectedDigest
          ExpectedContent=$acceptedText
        }
        $nativeYesResult = Invoke-ConstellationPrivateNativeConsent @createArgs
        if ($nativeYesResult.verified_native_control_count -ne 8 -or
            -not $nativeYesResult.verified_full_exact_hex_bytes) {
          throw 'Native create-new YES did not expose eight verified Win32 controls and complete immutable hex bytes.'
        }
        $result.native_consent_dialog_count++
        if ($nativeYesResult.screenshot) { $result.screenshots += $nativeYesResult.screenshot }
        $result.native_consent_operation_ids += $nativeYesResult.operation_id
        $result.native_consent_operation_digests += $nativeYesResult.operation_digest
        $result.native_consent_create_yes_status = 'pass_owned_default_no_dialog_yes_created_new_file'
      } else {
        Ui-Invoke (Ui-WaitEnabled $root 'Approve this exact file action' 8)
      }
      if ($null -eq (Ui-Wait $root 'Audit result: succeeded' '' 8)) {
        throw 'Native create-new approval did not expose a successful audit receipt.'
      }
      $actualFile = Join-Path $privateFiles $runFile
      if (-not (Test-Path -LiteralPath $actualFile -PathType Leaf)) {
        throw 'Approved create-new file missing under private Data.'
      }
      $expectedBytes = [Text.Encoding]::UTF8.GetBytes($acceptedText)
      $observedBytes = [IO.File]::ReadAllBytes($actualFile)
      if ([Convert]::ToBase64String($observedBytes) -cne [Convert]::ToBase64String($expectedBytes)) {
        throw 'Approved file content differs from exact UTF-8 fixture bytes.'
      }
      $result.file_tool_artifact_sha256 = (Get-FileHash -LiteralPath $actualFile -Algorithm SHA256).Hash.ToLowerInvariant()
      $result.file_tool_create_status = 'pass_explicit_approval_and_exact_bytes'
      try { $result.screenshots += Capture-Window $hwnd 'file-create-approved' }
      catch { $result.blockers += "File create screenshot: $($_.Exception.Message)" }

      # First a separate MessageBoxW permission to inspect the lexical path;
      # no read preview or file metadata is allowed in the renderer before it.
      # NO must reject preparation without exposing any file metadata. Then
      # YES permits inspection, but not the second SHA-bound V2 approval.
      Ui-Invoke (Ui-WaitEnabled $root 'Read an existing text file' 8)
      Ui-SetText $root (Ui-Require $root '' 'file-tool-path') $runFile
      if ($TestNativeConsent) {
        $firstStage = @{
          Root=$root; ApplicationWindow=$hwnd; ProcessId=$process.Id; DesktopName=$HiddenDesktopWorkerName
          ExpectedTaskId=$createPreview.task_id; ExpectedRelativePath=$runFile
          ExpectedPrivatePath=(Join-Path $privateFiles $runFile)
          ExpectedFixtureSha256=$result.file_tool_artifact_sha256
          ExpectedFixtureByteCount=$expectedBytes.Length
        }
        $readIntentNo = Invoke-ConstellationPrivateReadIntent @firstStage -Decision 'No' -AttemptTag 'initial'
        $result.native_read_intent_dialog_count++
        if ($readIntentNo.screenshot) { $result.screenshots += $readIntentNo.screenshot }
        if ($null -ne (Ui-Wait $root 'Approval required for proposed file action' '' 2)) {
          throw 'First native NO incorrectly prepared an existing file read.'
        }
        if ($null -eq (Ui-Wait $root 'Could not prepare the local action. Confirm that the task is pending, the path is inside the private artifacts folder, and the OS credential vault is available.' '' 8)) {
          throw 'First native NO did not produce the fail-closed read preparation error.'
        }
        if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($actualFile)) -cne
            [Convert]::ToBase64String($expectedBytes)) {
          throw 'First native NO modified fixture bytes.'
        }
        $result.native_read_intent_no_status = 'pass_first_native_no_denied_prepare_no_read_preview'
        $readIntentYes = Invoke-ConstellationPrivateReadIntent @firstStage -Decision 'Yes' -AttemptTag 'initial'
        $result.native_read_intent_dialog_count++
        if ($readIntentYes.screenshot) { $result.screenshots += $readIntentYes.screenshot }
        $result.native_read_intent_yes_status = 'pass_first_native_yes_then_separate_sha_bound_review_required'
      } else {
        Ui-Invoke (Ui-WaitEnabled $root 'Request native permission to inspect' 8)
      }
      if ($null -eq (Ui-Wait $root 'Approval required for proposed file action' '' 8)) {
        throw 'Read action had no immutable preview AFTER first native YES.'
      }
      if ($TestNativeConsent) {
        # Rust privately pins the exact UTF-8 SHA before any consent, while the
        # renderer MUST disclose neither its hash nor byte count. The native
        # OS dialog separately exposes the fingerprint for independent review.
        $readDigest = (Get-FileHash -LiteralPath $actualFile -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($readDigest -cne $result.file_tool_artifact_sha256) {
          throw 'Existing file changed between successful creation and read preview.'
        }
        $readPreview = Ui-NativeConsentPreview $root $runFile $privateFiles $readDigest $expectedBytes.Length
        $readArgs = @{
          Root=$root; ApplicationWindow=$hwnd; ProcessId=$process.Id; DesktopName=$HiddenDesktopWorkerName
          Decision='Yes'; Action='read'; ExpectedPath=$readPreview.canonical_path
          ExpectedTaskId=$readPreview.task_id; ExpectedContentSha256=$readDigest
          ExpectedReadByteCount=$expectedBytes.Length
        }
        $nativeReadResult = Invoke-ConstellationPrivateNativeConsent @readArgs
        if ($nativeReadResult.verified_native_control_count -ne 8 -or
            -not $nativeReadResult.verified_file_sha256) {
          throw 'Native read YES omitted eight verified Win32 controls or the immutable SHA-256.'
        }
        $result.native_consent_dialog_count++
        if ($nativeReadResult.screenshot) { $result.screenshots += $nativeReadResult.screenshot }
        $result.native_consent_operation_ids += $nativeReadResult.operation_id
        $result.native_consent_operation_digests += $nativeReadResult.operation_digest
        $result.native_consent_read_yes_status = 'pass_owned_default_no_dialog_yes_read_existing'
      } else {
        Ui-Invoke (Ui-WaitEnabled $root 'Approve this exact file action' 8)
      }
      if ($null -eq (Ui-Wait $root 'Audit result: succeeded' '' 8)) {
        throw 'Approved read has no successful native audit receipt.'
      }
      # Chromium UIA exposes the entire <pre> including its trailing LF;
      # trimming the fixture would falsely reject an exact byte match.
      if ($null -eq (Ui-Wait $root $acceptedText '' 7)) {
        throw 'Approved read result does not expose exact persisted file content.'
      }
      $result.file_tool_read_status = 'pass_separately_approved_exact_existing_content'

      # No-clobber: preparing a second create is allowed, but successful
      # replacement is not. Whether preview or execution rejects the attempt,
      # existing bytes must remain identical and the outcome cannot be PASS.
      Ui-Invoke (Ui-WaitEnabled $root 'Create a new text file' 8)
      Ui-SetText $root (Ui-Require $root '' 'file-tool-path') $runFile
      Ui-SetText $root (Ui-Require $root '' 'file-tool-content') 'This must not overwrite the approved file.'
      Ui-Invoke (Ui-WaitEnabled $root 'Preview exact file action' 8)
      $clobberPreview = Ui-Wait $root 'Approval required for proposed file action' '' 4
      if ($null -ne $clobberPreview) {
        if ($TestNativeConsent) {
          $clobberText = 'This must not overwrite the approved file.'
          $clobberDigest = [BitConverter]::ToString(
            [Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($clobberText))
          ).Replace('-', '').ToLowerInvariant()
          $clobberFields = Ui-NativeConsentPreview $root $runFile $privateFiles $clobberDigest
          $clobberArgs = @{
            Root=$root; ApplicationWindow=$hwnd; ProcessId=$process.Id; DesktopName=$HiddenDesktopWorkerName
            Decision='Yes'; Action='create_new'; ExpectedPath=$clobberFields.canonical_path
            ExpectedTaskId=$clobberFields.task_id; ExpectedContentSha256=$clobberDigest
            ExpectedContent=$clobberText
          }
          $nativeClobber = Invoke-ConstellationPrivateNativeConsent @clobberArgs
          $result.native_consent_dialog_count++
          if ($nativeClobber.screenshot) { $result.screenshots += $nativeClobber.screenshot }
          $result.native_consent_operation_ids += $nativeClobber.operation_id
          $result.native_consent_operation_digests += $nativeClobber.operation_digest
        } else {
          Ui-Invoke (Ui-WaitEnabled $root 'Approve this exact file action' 8)
        }
        if ($null -ne (Ui-Wait $root 'Audit result: succeeded' '' 3)) {
          throw 'A second create-new unexpectedly reported success on an occupied path.'
        }
      } elseif ($null -eq (Ui-Wait $root 'Could not prepare the local action. Confirm that the task is pending, the path is inside the private artifacts folder, and the OS credential vault is available.' '' 5)) {
        throw 'Second create-new neither showed a preview nor an explicit refusal.'
      }
      if ([Convert]::ToBase64String($expectedBytes) -cne [Convert]::ToBase64String([IO.File]::ReadAllBytes($actualFile))) {
        throw 'The no-clobber attempt changed the original file.'
      }
      $result.file_tool_no_clobber_status = 'pass_second_create_refused_original_bytes_unchanged'
      if ($TestNativeConsent) {
        $result.native_consent_no_clobber_status = if ($null -eq $clobberPreview) {
          'pass_prepared_create_refused_before_native_consent'
        } else {
          'pass_second_native_yes_refused_existing_file'
        }
      }

      if ($TestNativeConsent) {
        # Real TOCTOU regression: the read preview has already committed to
        # the original bytes and SHA. Change one byte AFTER that preview but
        # BEFORE the genuine native Yes, keeping the byte count identical.
        # A successful receipt or disclosure of old/new content is a failure.
        Ui-Invoke (Ui-WaitEnabled $root 'Read an existing text file' 8)
        Ui-SetText $root (Ui-Require $root '' 'file-tool-path') $runFile
        $mutationIntent = Invoke-ConstellationPrivateReadIntent @firstStage -Decision 'Yes' -AttemptTag 'mutation'
        $result.native_read_intent_dialog_count++
        if ($mutationIntent.screenshot) { $result.screenshots += $mutationIntent.screenshot }
        $result.native_read_intent_mutation_status = 'pass_new_first_native_yes_before_mutation_snapshot'
        if ($null -eq (Ui-Wait $root 'Approval required for proposed file action' '' 8)) {
          throw 'Adversarial read did not prepare the private immutable SHA snapshot.'
        }
        $snapshotSha = (Get-FileHash -LiteralPath $actualFile -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($snapshotSha -cne $result.file_tool_artifact_sha256) {
          throw 'Original read fixture changed before the mutation test.'
        }
        $mutationPreview = Ui-NativeConsentPreview $root $runFile $privateFiles $snapshotSha $expectedBytes.Length
        $mutatedBytes = [byte[]]$expectedBytes.Clone()
        $mutatedBytes[0] = if ($mutatedBytes[0] -eq 0x41) { 0x42 } else { 0x41 }
        [IO.File]::WriteAllBytes($actualFile, $mutatedBytes)
        if ([IO.File]::ReadAllBytes($actualFile).Length -ne $expectedBytes.Length -or
            (Get-FileHash -LiteralPath $actualFile -Algorithm SHA256).Hash -ieq $snapshotSha) {
          throw 'Adversarial fixture mutation did not preserve length and alter the exact hash.'
        }
        $mutationArgs = @{
          Root=$root; ApplicationWindow=$hwnd; ProcessId=$process.Id; DesktopName=$HiddenDesktopWorkerName
          Decision='Yes'; Action='read'; ExpectedPath=$mutationPreview.canonical_path
          ExpectedTaskId=$mutationPreview.task_id; ExpectedContentSha256=$snapshotSha
          ExpectedReadByteCount=$expectedBytes.Length
        }
        $mutationResult = Invoke-ConstellationPrivateNativeConsent @mutationArgs
        if ($mutationResult.verified_native_control_count -ne 8 -or -not $mutationResult.verified_file_sha256) {
          throw 'Mutated-read native consent omitted verified immutable SHA commitment.'
        }
        $result.native_consent_dialog_count++
        if ($mutationResult.screenshot) { $result.screenshots += $mutationResult.screenshot }
        $result.native_consent_operation_ids += $mutationResult.operation_id
        $result.native_consent_operation_digests += $mutationResult.operation_digest
        if ($null -eq (Ui-Wait $root 'Audit result: failed' '' 8) -or
            $null -eq (Ui-Wait $root 'file contents changed since read approval' '' 8)) {
          throw 'Changed read bytes were not rejected with the expected content conflict receipt.'
        }
        if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($actualFile)) -cne
            [Convert]::ToBase64String($mutatedBytes)) {
          throw 'The rejected changed-content read modified the on-disk bytes.'
        }
        $visibleOriginalContent = @((Ui-Records $root) | Where-Object {
          $_.Name -ceq $acceptedText
        })
        if ($visibleOriginalContent.Count -ne 0) {
          throw 'The failed read disclosed original file content despite a changed-content rejection.'
        }
        $result.native_consent_read_mutation_status = 'pass_same_length_changed_sha_read_refused_no_content_disclosed'
      }

      # A journal exists only under this isolated profile, is nonempty, and
      # remains after each UI approval/denial. Never print its authenticated
      # contents or any local credential key.
      $journals = @(Get-ChildItem -LiteralPath $privateJournal -Filter 'file-operations-*.jsonl' -File -ErrorAction Stop)
      $result.file_tool_journal_file_count = $journals.Count
      if ($journals.Count -ne 1 -or $journals[0].Length -lt 100) {
        throw 'Native authenticated file-operation journal missing or empty.'
      }
      $result.file_tool_journal_status = 'pass_nonempty_private_journal_persisted'
      if ($TestNativeConsent) {
        if (@($result.native_consent_operation_ids | Select-Object -Unique).Count -ne $result.native_consent_dialog_count) {
          throw 'The native dialog reused an operation ID for distinct approval attempts.'
        }
        # The digest commits to the prepared action, scope, path and content,
        # so two distinct reads of the same snapshot can legitimately share
        # the digest. The one-use proof is scoped by both the unique operation
        # ID and digest; verify every ID/digest pair against the journal.
        if ($result.native_consent_operation_digests.Count -ne $result.native_consent_dialog_count -or
            @($result.native_consent_operation_digests | Where-Object {
              $_ -notmatch '^[a-fA-F0-9]{64}$'
            }).Count -ne 0) {
          throw 'A native approval omitted a valid immutable operation digest.'
        }
        # Match each independent OS consent commitment to this profile's
        # durable journal. Do not expose the journal, the credential vault key,
        # or user content. Chain syntax alone does not verify its secret HMAC.
        $journalRecords = @()
        $previousTag = '0' * 64
        # The app intentionally keeps the authenticated journal writer open.
        # ReadAllLines uses FileShare.Read on .NET Framework, which conflicts
        # with that live writer on Windows. Share both reads and writes while
        # independently checking the persisted, newline-terminated snapshot.
        $journalStream = [IO.File]::Open($journals[0].FullName,
          [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
        try {
          $journalReader = New-Object IO.StreamReader($journalStream)
          try { $journalText = $journalReader.ReadToEnd() }
          finally { $journalReader.Dispose() }
        } finally { $journalStream.Dispose() }
        if (-not $journalText.EndsWith("`n")) {
          throw 'Isolated audit journal ended with an incomplete record.'
        }
        foreach ($line in ($journalText -split "`n")) {
          $line = $line.TrimEnd("`r")
          if (-not $line) { continue }
          $entry = ConvertFrom-Json -InputObject $line
          if ($null -eq $entry.record -or $entry.sequence -ne $journalRecords.Count -or
              $entry.previous_tag -cne $previousTag -or $entry.tag -notmatch '^[0-9a-fA-F]{64}$') {
            throw 'The isolated audit journal is missing a sequential authenticated-record envelope.'
          }
          $journalRecords += $entry.record
          $previousTag = $entry.tag
        }
        $result.native_consent_journal_record_count = $journalRecords.Count
        for ($index = 0; $index -lt $result.native_consent_operation_ids.Count; $index++) {
          $nativeId = $result.native_consent_operation_ids[$index]
          $nativeDigest = $result.native_consent_operation_digests[$index]
          $matching = @($journalRecords | Where-Object {
            if ($_.event -eq 'finish') { $_.receipt.operation_id -ieq $nativeId }
            else { $_.id -ieq $nativeId }
          })
          $expectedEvents = if ($index -eq 0) { @('deny') } else { @('approve', 'start', 'finish') }
          if ($matching.Count -ne $expectedEvents.Count) {
            throw 'Isolated journal event count differs from one native operation decision.'
          }
          foreach ($eventName in $expectedEvents) {
            $eventRecords = @($matching | Where-Object { $_.event -ceq $eventName })
            if ($eventRecords.Count -ne 1) { throw 'Native operation journal is missing an exact expected state transition.' }
            $journalDigest = if ($eventName -eq 'finish') {
              [string]$eventRecords[0].receipt.digest
            } else {
              [string]$eventRecords[0].digest
            }
            if ($journalDigest -ine $nativeDigest) {
              throw 'Native consent commitment differs from durable journal operation digest.'
            }
          }
        }
        $result.native_consent_journal_status = 'pass_exact_native_ids_digests_match_private_journal'
      }
      $result.file_tool_status = 'pass_denial_create_read_no_clobber_journal'
      # A correct fail-closed no-clobber path may reject the second create at
      # PREPARE, without ever presenting a fifth V2 dialog.
      $expectedV2Dialogs = if ($null -eq $clobberPreview) { 4 } else { 5 }
      if ($TestNativeConsent -and
          $result.native_read_intent_no_status -eq 'pass_first_native_no_denied_prepare_no_read_preview' -and
          $result.native_read_intent_yes_status -eq 'pass_first_native_yes_then_separate_sha_bound_review_required' -and
          $result.native_read_intent_mutation_status -eq 'pass_new_first_native_yes_before_mutation_snapshot' -and
          $result.native_read_intent_dialog_count -eq 3 -and
          $result.native_consent_no_status -eq 'pass_owned_native_dialog_default_no_explicit_denial_no_file' -and
          $result.native_consent_create_yes_status -eq 'pass_owned_default_no_dialog_yes_created_new_file' -and
          $result.native_consent_read_yes_status -eq 'pass_owned_default_no_dialog_yes_read_existing' -and
          $result.native_consent_read_mutation_status -eq 'pass_same_length_changed_sha_read_refused_no_content_disclosed' -and
          $result.native_consent_no_clobber_status -match '^pass_' -and
          $result.native_consent_journal_status -eq 'pass_exact_native_ids_digests_match_private_journal' -and
          $result.native_consent_dialog_count -eq $expectedV2Dialogs) {
        $result.native_consent_status = 'pass_sha_bound_read_no_create_read_no_clobber_and_read_mutation'
      }
    } catch {
      $result.file_tool_status = 'blocked_or_failed_uia_or_native_file_action'
      $result.blockers += "Manual file tools: $($_.Exception.Message)"
      $result.file_tool_discovery = @(Ui-Discovery $root 170)
      try { $result.screenshots += Capture-Window $hwnd 'file-action-blocker' }
      catch { $result.blockers += "File tools screenshot: $($_.Exception.Message)" }
    }
  }
  if ($TestDrafts) {
    try {
      if ($TestWorkflows -or $TestFileTools) {
        throw 'Draft restart QA requires its own isolated run without other room creation workflows.'
      }
      Ui-Invoke (Ui-Require $root 'Rooms' '')
      $newRoomButton = Ui-Wait $root 'Create your first room' '' 5
      if ($null -eq $newRoomButton) { $newRoomButton = Ui-Wait $root 'Create room' '' 5 }
      if ($null -eq $newRoomButton) { throw 'Draft QA cannot locate native create-room action.' }
      Ui-Invoke $newRoomButton
      $roomName = Ui-RequireUniqueEdit $root 'ROOM NAME'
      Ui-SetText $root $roomName 'Private restart draft QA'
      Ui-InvokeModalCreateRoom $root
      $composer = Ui-Wait $root 'Write a local message' '' 8
      if ($null -eq $composer) { throw 'Native room composer not discoverable after room creation.' }
      $text = "Private unsent text $runId" # Synthetic only; never a provider credential.
      Ui-SetText $root $composer $text
      $until = [DateTime]::UtcNow.AddSeconds(8)
      do {
        if (@((Ui-Records $root) | Where-Object { $_.Name -ceq 'Draft saved locally in this project' }).Count -gt 0) { break }
        Start-Sleep -Milliseconds 200
      } while ([DateTime]::UtcNow -lt $until)
      if ([DateTime]::UtcNow -ge $until) { throw 'Native UI never confirmed draft persisted; refusing restart PASS.' }
      $marker = Join-Path $isolatedData '.constellation-owned-qa-v1'
      if (-not (Test-Path -LiteralPath $marker -PathType Leaf) -or
          -not (Test-Path -LiteralPath (Join-Path $isolatedData 'constellation.sqlite3') -PathType Leaf) -or
          -not (Test-Path -LiteralPath (Join-Path $isolatedData 'constellation-workflows.sqlite3') -PathType Leaf)) {
        throw 'Private restart identity or its two SQLite databases are missing; refusing resume.'
      }
      # Close exactly the process launched in this sandbox, never the user's app.
      [void]$process.CloseMainWindow()
      if (-not $process.WaitForExit(2500)) {
        # WebView2 can refuse CloseMainWindow on a private Win32 desktop.
        # The draft was already confirmed durable. Kill only this exact
        # disposable PID, making the next launch a useful crash-recovery test.
        $process.Kill()
        if (-not $process.WaitForExit(5000)) { throw 'Isolated draft process did not exit after targeted stop.' }
      }
      $process.Dispose()
      $process = New-Object Diagnostics.Process
      $psi.Arguments = '--resume-isolated-data-dir "' + $isolatedData + '"'
      $process.StartInfo = $psi
      Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
      if (-not $process.Start()) { throw 'Native draft QA restart failed.' }
      $result.pid_after_draft_restart = $process.Id
      $root = $null
      $hwnd = [IntPtr]::Zero
      $until = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
      do {
        $process.Refresh()
        if ($process.HasExited) { throw 'Native app crashed during persisted draft reopen.' }
        $hwnd = [ConstellationEvidenceWindow]::FindOwnMainWindow($process.Id)
        if ($hwnd -ne [IntPtr]::Zero) {
          Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
          $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
          $fresh = Ui-Find $root 'Write a local message' ''
          if ($null -ne $fresh) {
            $valueObject = $null
            if ($fresh.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$valueObject)) {
              $observed = [string]([Windows.Automation.ValuePattern]$valueObject).Current.Value
              if ($observed -ceq $text) { break }
            }
          }
        }
        Start-Sleep -Milliseconds 220
      } while ([DateTime]::UtcNow -lt $until)
      if ($observed -cne $text) { throw 'Reopened native composer did not recover exact unsent draft text.' }
      $result.screenshots += Capture-Window $hwnd 'draft-recovered-after-restart'
      # This is a local-save-only button. It must never issue a provider call.
      Ui-Invoke (Ui-WaitEnabled $root 'Save message locally' 8)
      if ($null -eq (Ui-Wait $root '1 messages' '' 8)) {
        throw 'Locally sent draft did not appear as exactly one saved room message.'
      }
      $clearedComposer = Ui-Wait $root 'Write a local message' '' 8
      $clearedPattern = $null
      if ($null -eq $clearedComposer -or
          -not $clearedComposer.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$clearedPattern) -or
          [string]([Windows.Automation.ValuePattern]$clearedPattern).Current.Value -cne '') {
        throw 'After saving the recovered message, its composer must visibly clear without making any provider request.'
      }
      $result.screenshots += Capture-Window $hwnd 'draft-locally-sent-compact-room'
      $result.draft_status = 'pass_native_unsent_draft_recovered_exact_after_restart_and_locally_sent'
    } catch {
      $result.draft_status = 'blocked_or_failed_private_native_draft_reopen'
      $result.blockers += "Unsent draft: $($_.Exception.Message)"
    }
  }
} catch {
  $result.blockers += $_.Exception.Message
} finally {
  if ($TestNativeConsent) {
    try {
      if ((Assert-PinnedConstellationSources $ExpectedSourceIdentityPath $ExpectedNativeSourceSha256) -cne $expectedManifestHash) {
        throw 'Source manifest changed during GUI acceptance.'
      }
    } catch {
      $result.blockers += "One of eleven pinned sources changed during GUI acceptance: $($_.Exception.Message)"
      $result.native_consent_status = 'blocked_source_changed_during_execution'
    }
  }
  if ($TestNativeConsent -and
      (Get-FileHash -LiteralPath $nativeSource -Algorithm SHA256).Hash -ine $ExpectedNativeSourceSha256) {
    $result.blockers += 'Native consent source changed during execution; observations cannot certify this source revision.'
    $result.native_consent_status = 'blocked_source_changed_during_execution'
  }
  if ($ExpectedExeSha256 -and (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
    $result.blockers += 'Executable changed during hidden-desktop acceptance; prior observations have no stable binary identity.'
    $result.native_consent_status = 'blocked_executable_changed_during_execution'
  }
  if ($null -ne $workflowFixture) {
    $result.workflow_fixture_request_count = $workflowFixture.RequestCount
    $result.workflow_fixture_rejected_count = $workflowFixture.RejectedCount
    $workflowFixture.Dispose()
  }
  if ($null -ne $process) {
    try {
      $process.Refresh()
      if (-not $process.HasExited) {
        [void]$process.CloseMainWindow()
        if (-not $process.WaitForExit(4000)) {
          $process.Kill() # Own process object, never by executable name/tree.
          [void]$process.WaitForExit(5000)
        }
      }
      $result.process_closed = $process.HasExited
    } catch {
      $result.blockers += 'Could not confirm spawned PID cleanup.'
    } finally { $process.Dispose() }
  }
}

$result | ConvertTo-Json -Depth 8
# A UIA blocker is an exit failure; never silently certify evidence/backup.
if ($result.evidence_status -ne 'pass_exact_source_quote_span_hash' -or -not $result.process_closed) { exit 1 }
if (-not $SkipBackup -and (
  $result.backup_status -ne 'pass_isolated_encrypted_file_and_uia_confirmation' -or
  $result.restore_status -ne 'pass_isolated_separate_sqlite_and_uia_confirmation'
)) { exit 1 }
if ($TestProjectArchive -and (
  $result.project_archive_status -ne 'pass_two_db_encrypted_file_and_uia_confirmation' -or
  $result.project_restore_status -ne 'pass_two_verified_sqlite_files_in_new_directory'
)) { exit 1 }
$expectedWorkflowStatus = if ($TestWorkflowStep) {
  'pass_preview_save_reload_localhost_step_cancel'
} else {
  'pass_preview_save_reload_cancel_without_provider_calls'
}
if ($TestWorkflows -and $result.workflow_status -ne $expectedWorkflowStatus) { exit 1 }
if ($TestWorkflowStep -and ($result.workflow_step_status -ne 'pass_one_loopback_request_persisted_text_result' -or
  $result.workflow_fixture_request_count -ne 1 -or $result.workflow_fixture_rejected_count -ne 0)) { exit 1 }
if ($TestWorkflowReport -and $result.workflow_report_status -ne 'pass_native_markdown_rendered_and_checked_without_clipboard') { exit 1 }
if ($TestFileTools -and $result.file_tool_status -ne 'pass_denial_create_read_no_clobber_journal') { exit 1 }
if ($TestNativeConsent -and $result.native_consent_status -ne 'pass_sha_bound_read_no_create_read_no_clobber_and_read_mutation') { exit 1 }
if ($TestDrafts -and $result.draft_status -ne 'pass_native_unsent_draft_recovered_exact_after_restart_and_locally_sent') { exit 1 }
