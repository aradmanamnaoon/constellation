# Source-pinned private-desktop UIA test of the model-proposed CreateNew path.
# A deterministic localhost HTTP fixture responds with fixed structured JSON;
# it is NOT model inference. No user desktop input or NSIS installer is used.
[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string]$ExePath,
  [Parameter(Mandatory=$true)][string]$ExpectedExeSha256,
  [Parameter(Mandatory=$true)][string]$ExpectedNativeSourceSha256,
  [Parameter(Mandatory=$true)][string]$ExpectedSourceIdentityPath,
  [string]$OutputDirectory,
  [ValidateRange(10,90)][int]$TimeoutSeconds = 45,
  [switch]$TestNativeConsent,
  [string]$HiddenDesktopWorkerName
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows only' }
foreach ($hash in @($ExpectedExeSha256, $ExpectedNativeSourceSha256)) {
  if ($hash -notmatch '^[a-fA-F0-9]{64}$') { throw 'A confirmed SHA256 is required for each launch gate' }
}
$exe = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).ProviderPath
if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
  throw 'EXE does not match prime-confirmed source-frozen build. No launch.'
}
$script:qaRepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
function Assert-PinnedConstellationSources {
  param([string]$ManifestPath, [string]$NativeHash)
  $names = @(
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
  $file = (Resolve-Path -LiteralPath $ManifestPath -ErrorAction Stop).ProviderPath
  $rows = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($file))
  if ($rows.Count -ne $names.Count) { throw 'Eleven-source manifest has unexpected entry count' }
  foreach ($name in $names) {
    $candidates = @($rows | Where-Object { $_.Path -ceq $name })
    if ($candidates.Count -ne 1 -or [string]$candidates[0].Hash -notmatch '^[a-fA-F0-9]{64}$') {
      throw "Eleven-source manifest lacks a unique valid entry: $name"
    }
    $filePath = Join-Path $script:qaRepositoryRoot $name.Replace('/', '\')
    $info = Get-Item -LiteralPath $filePath -ErrorAction Stop
    $sha256 = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash
    if ($info.PSIsContainer -or $info.Length -ne [long]$candidates[0].Size -or
        $sha256 -ine [string]$candidates[0].Hash) {
      throw "Eleven-source fingerprint drift: $name"
    }
    if ($name -ceq 'apps/desktop/src-tauri/src/native_consent.rs' -and
        $sha256 -ine $NativeHash) { throw 'Native consent source differs from the pinned build' }
  }
  return (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
}
$fullSourceManifest = Join-Path $env:LOCALAPPDATA 'Temp\constellation-source-20261031-prime\current-source-pinned-187-20261031.json'
$fullSourceManifestSha256 = '577537DEB0DCE77C7A2FFF7E87AF6AC2ECE851FB57EC0D927BAE7A2135BEF94D'
function Assert-PinnedFullSourceAndAssets {
  param([string]$ManifestPath, [string]$ExpectedManifestSha256)
  $file = (Resolve-Path -LiteralPath $ManifestPath -ErrorAction Stop).ProviderPath
  $manifestSha256 = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
  if ($manifestSha256 -ine $ExpectedManifestSha256) {
    throw '187-source-and-assets manifest differs from prime-confirmed fingerprint'
  }
  $rows = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($file))
  if ($rows.Count -ne 187) { throw 'Complete source/asset manifest has unexpected entry count' }
  $rootDir = $script:qaRepositoryRoot
  $visited = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)
  foreach ($row in $rows) {
    $name = [string]$row.Path
    if ($name -notmatch '^[a-zA-Z0-9][a-zA-Z0-9./_+@ -]*$' -or
        $name.Contains('//') -or $name.Contains('/../') -or
        $name.StartsWith('../') -or $name.EndsWith('/..') -or
        $name.Contains('/./') -or -not $visited.Add($name) -or
        [string]$row.Hash -notmatch '^[a-fA-F0-9]{64}$') {
      throw 'Complete source/asset manifest contains unsafe or repeated entry'
    }
    $path = Join-Path $rootDir $name.Replace('/', '\')
    $entry = Get-Item -LiteralPath $path -ErrorAction Stop
    if ($entry.PSIsContainer -or $entry.Length -ne [long]$row.Size -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine [string]$row.Hash) {
      throw "Current source or embedded frontend asset differs from final build: $name"
    }
  }
  return $manifestSha256.ToLowerInvariant()
}
$fullManifestVerifiedSha256 = Assert-PinnedFullSourceAndAssets $fullSourceManifest $fullSourceManifestSha256
$nativeSource = Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\src\native_consent.rs'
$smokeSource = Join-Path $PSScriptRoot 'windows-evidence-backup-e2e.ps1'
$smokeText = [IO.File]::ReadAllText($smokeSource)

# Reuse the existing QA suite's exact eleven-source manifest validator and
# proven UI Automation routines, without executing its top-level script.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($smokeText, [ref]$tokens, [ref]$parseErrors)
if (@($parseErrors).Count -gt 0) { throw 'Existing UIA helper source has parse errors' }
$names = @('Ui-Records', 'Ui-Find',
  'Ui-Require', 'Ui-RequireUniqueEdit', 'Ui-Invoke', 'Ui-Wait',
  'Ui-WaitEnabled', 'Ui-SetText', 'Ui-InvokeModalCreateRoom',
  'Ui-NativeConsentPreview', 'Ui-Discovery', 'Capture-Window')
$funcs = @($ast.FindAll({
  param($node)
  $node -is [Management.Automation.Language.FunctionDefinitionAst]
}, $true))
foreach ($name in $names) {
  $candidate = @($funcs | Where-Object { $_.Name -ceq $name })
  if ($candidate.Count -ne 1) { throw "Missing or ambiguous existing helper $name" }
  Invoke-Expression $candidate[0].Extent.Text
}
function Wait-UniqueEditableControl($root, [string]$accessibleName, [int]$seconds = 8) {
  # WebView2 may expose the clicked modal button before Svelte commits the
  # newly created input to its UIA tree. Wait for the *real* uniquely named
  # editable control; never synthesize a value or skip the visible dialog.
  $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
  $matches = @()
  do {
    $matches = @((Ui-Records $root) | Where-Object {
      $_.Name -ceq $accessibleName -and $_.Type -ceq 'ControlType.Edit'
    })
    if ($matches.Count -eq 1) { return $matches[0] }
    if ($matches.Count -gt 1) {
      throw "Ambiguous UIA editable control '$accessibleName': $($matches.Count) matches."
    }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $deadline)
  $available = @((Ui-Records $root) | Where-Object {
    $_.Type -ceq 'ControlType.Edit'
  } | ForEach-Object { $_.Name + ' [' + $_.Id + ']' }) -join '; '
  throw "Timed out waiting for one real editable '$accessibleName' field; available edits: $available"
}
$win32Match = [regex]::Match($smokeText, '(?s)\$win32\s*=\s*@''\r?\n(?<body>.*?)\r?\n''@')
if (-not $win32Match.Success) { throw 'Missing existing private-window Win32 interop definition' }
Add-Type -TypeDefinition $win32Match.Groups['body'].Value -ReferencedAssemblies 'System' -ErrorAction Stop
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing -ErrorAction Stop
$sourceIdentitySha = Assert-PinnedConstellationSources $ExpectedSourceIdentityPath $ExpectedNativeSourceSha256
if ((Get-FileHash -LiteralPath $nativeSource -Algorithm SHA256).Hash -ine $ExpectedNativeSourceSha256) {
  throw 'Native consent source differs from prime-confirmed SHA256. No launch.'
}
. (Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1')
if (-not $HiddenDesktopWorkerName) {
  $params = @{
    ScriptPath = $PSCommandPath; ExePath = $exe; TimeoutSeconds = $TimeoutSeconds
    TestNativeConsent = $true; ExpectedExeSha256 = $ExpectedExeSha256
    ExpectedNativeSourceSha256 = $ExpectedNativeSourceSha256
    ExpectedSourceIdentityPath = $ExpectedSourceIdentityPath
  }
  if ($OutputDirectory) { $params.OutputDirectory = $OutputDirectory }
  Invoke-ConstellationHiddenDesktopWorker @params
  return
}
Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
. (Join-Path $PSScriptRoot 'helper-native-consent.ps1')

$runId = [Guid]::NewGuid().ToString('N')
$sandbox = Join-Path ([IO.Path]::GetTempPath()) "constellation-model-qa-$runId"
$isolatedData = Join-Path $sandbox 'Data'
$artifacts = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else {
  Join-Path $sandbox 'artifacts'
}
foreach ($path in @($sandbox, $isolatedData, (Join-Path $sandbox 'Temp'),
    (Join-Path $sandbox 'WebView2'), $artifacts)) {
  [void](New-Item -ItemType Directory -Path $path -Force)
}
if (@(Get-ChildItem -LiteralPath $isolatedData -Force).Count -ne 0) {
  throw 'QA profile must start empty'
}
$fileName = 'mock-model-proposal.txt'
$fileText = "Deterministic localhost model-proposal fixture. No AI inference.`n"
$proposal = @{ type = 'tool'; action = 'create_file'; path = $fileName; content = $fileText } |
  ConvertTo-Json -Compress
# The reused HTTP fixture concatenates its Result into a JSON string; supply
# correctly JSON-escaped content. Embed it in C# as base64 to avoid string or
# PowerShell interpolation ambiguities.
$escapedProposal = $proposal.Replace('\', '\\').Replace('"', '\"')
$encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($escapedProposal))
$fixtureText = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'helper-local-workflow-fixture.ps1'))
$fixtureMatch = [regex]::Match($fixtureText, '(?s)\$localFixtureSource\s*=\s*@''\r?\n(?<body>.*?)\r?\n''@')
if (-not $fixtureMatch.Success) { throw 'Pinned local HTTP mock helper is missing' }
$fixtureCsharp = $fixtureMatch.Groups['body'].Value
$oldResult = 'public const string Result = "CONSTELLATION_LOCALHOST_WORKFLOW_E2E_SUCCESS";'
$replacement = 'public static readonly string Result = Encoding.UTF8.GetString(Convert.FromBase64String("' + $encoded + '"));'
if (-not $fixtureCsharp.Contains($oldResult)) { throw 'HTTP helper result contract changed' }
$fixtureCsharp = $fixtureCsharp.Replace($oldResult, $replacement)
Add-Type -TypeDefinition $fixtureCsharp -ReferencedAssemblies 'System' -ErrorAction Stop
$fixture = New-Object ConstellationLocalWorkflowFixture
$modelName = [ConstellationLocalWorkflowFixture]::Model
$fileSha = [BitConverter]::ToString(
  [Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($fileText))
).Replace('-', '').ToLowerInvariant()
$result = [ordered]@{
  fixture_type = 'explicit_simulated_localhost_openai_chat_completions_not_ai'
  run_id = $runId
  exe_sha256 = $ExpectedExeSha256.ToLowerInvariant()
  native_source_sha256 = $ExpectedNativeSourceSha256.ToLowerInvariant()
  source_identity_manifest_sha256 = $sourceIdentitySha
  source_and_assets_187_manifest_sha256 = $fullManifestVerifiedSha256
  helper_smoke_sha256 = (Get-FileHash -LiteralPath $smokeSource -Algorithm SHA256).Hash.ToLowerInvariant()
  helper_fixture_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'helper-local-workflow-fixture.ps1') -Algorithm SHA256).Hash.ToLowerInvariant()
  hidden_desktop = $HiddenDesktopWorkerName
  pid = 0; requests = 0; rejected_requests = 0
  model_proposal_status = 'pending'
  model_origin_label_uia = 'pending'
  manual_file_create_status = 'pending'
  native_consent_status = 'pending'
  first_read_intent_status = 'pending'
  final_read_approval_status = 'pending'
  native_operation_id = ''
  native_operation_digest = ''
  audit_event_count = 0
  audit_status = 'pending'
  output_file_sha256 = ''
  screenshot = $null
  provider_selection_screenshot = $null
  model_preapproval_screenshot = $null
  native_create_screenshot = $null
  native_first_read_screenshot = $null
  native_final_read_screenshot = $null
  provider_dialog_discovery = @()
  task_provider_discovery = @()
  task_provider_combo_patterns = @()
  process_closed = $false; blockers = @()
}
$process = $null
try {
  if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe)).Contains('--isolated-data-dir')) {
    throw 'Binary lacks --isolated-data-dir; refusing execution'
  }
  if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) { throw 'EXE drift before launch' }
  if ((Assert-PinnedConstellationSources $ExpectedSourceIdentityPath $ExpectedNativeSourceSha256) -cne $sourceIdentitySha) {
    throw 'Source manifest drift before launch'
  }
  if ((Assert-PinnedFullSourceAndAssets $fullSourceManifest $fullSourceManifestSha256) -cne $fullManifestVerifiedSha256) {
    throw 'Complete 187-source/asset identity drift before launch'
  }
  Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName = $exe
  $psi.Arguments = '--isolated-data-dir "' + $isolatedData + '"'
  $psi.WorkingDirectory = (Split-Path -Parent $exe)
  $psi.UseShellExecute = $false
  $psi.EnvironmentVariables['TEMP'] = Join-Path $sandbox 'Temp'
  $psi.EnvironmentVariables['TMP'] = Join-Path $sandbox 'Temp'
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $sandbox 'WebView2'
  $process = New-Object Diagnostics.Process
  $process.StartInfo = $psi
  if (-not $process.Start()) { throw 'Could not start source-pinned EXE' }
  $result.pid = $process.Id
  $hwnd = [IntPtr]::Zero
  $root = $null
  $until = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    $process.Refresh()
    if ($process.HasExited) { throw "Native app exited before UI readiness: $($process.ExitCode)" }
    $hwnd = [ConstellationEvidenceWindow]::FindOwnMainWindow($process.Id)
    if ($hwnd -ne [IntPtr]::Zero) {
      Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
      $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
      $initial = @(Ui-Records $root)
      if (@($initial | Where-Object { $_.Name -ceq 'Settings' }).Count -eq 1 -and
          @($initial | Where-Object { $_.Name -ceq 'Tasks' }).Count -ge 1) { break }
    }
    Start-Sleep -Milliseconds 220
  } while ([DateTime]::UtcNow -lt $until)
  if ($null -eq $root -or $hwnd -eq [IntPtr]::Zero) { throw 'Hidden WebView2 readiness not established' }
  if (-not (Test-Path -LiteralPath (Join-Path $isolatedData 'constellation.sqlite3') -PathType Leaf)) {
    throw 'No isolated SQLite database was created'
  }

  # All UIA actions are scoped to the exact HWND on the private desktop.
  Ui-Invoke (Ui-Require $root 'Library' '')
  Ui-Invoke (Ui-WaitEnabled $root 'Create agent' 8)
  Ui-Invoke (Ui-WaitEnabled $root 'Add teammate' 8)
  Ui-Invoke (Ui-Require $root 'Rooms' '')
  $create = Ui-Wait $root 'Create your first room' '' 4
  if ($null -eq $create) { $create = Ui-Wait $root 'Create room' '' 4 }
  if ($null -eq $create) { throw 'No isolated room creation UI' }
  Ui-Invoke $create
  Ui-SetText $root (Wait-UniqueEditableControl $root 'ROOM NAME') 'Private deterministic proposal QA'
  Ui-InvokeModalCreateRoom $root

  Ui-Invoke (Ui-Require $root 'Settings' '')
  Ui-Invoke (Ui-WaitEnabled $root 'Add provider' 8)
  $nameEdit = Ui-Wait $root 'DISPLAY NAME' 'provider-name' 8
  $endpointEdit = Ui-Wait $root 'API BASE URL' 'provider-endpoint' 8
  $modelEdit = Ui-Wait $root 'MODEL IDENTIFIER' 'provider-model' 8
  if ($null -eq $nameEdit -or $null -eq $endpointEdit -or $null -eq $modelEdit) {
    $result.provider_dialog_discovery = @(Ui-Discovery $root 100)
    throw 'Cannot find uniquely accessible localhost provider name/endpoint/model inputs'
  }
  Ui-SetText $root $nameEdit 'Constellation mock proposal'
  Ui-SetText $root $endpointEdit $fixture.Url
  Ui-SetText $root $modelEdit $modelName
  Ui-Invoke (Ui-WaitEnabled $root 'Save connection locally' 8)

  Ui-Invoke (Ui-Require $root 'Tasks' '')
  if ($null -eq (Ui-Wait $root 'Workflows' '' 8)) { throw 'Tasks view was not loaded' }
  $plan = Ui-Wait $root 'Plan task' '' 4
  if ($null -eq $plan) { $plan = Ui-Wait $root 'Plan your first task' '' 6 }
  if ($null -eq $plan) { throw 'Plan task button unavailable' }
  Ui-Invoke $plan
  Ui-SetText $root (Wait-UniqueEditableControl $root 'INSTRUCTIONS') 'Propose a small text artifact for this disposable QA task.'
  Ui-Invoke (Ui-WaitEnabled $root 'Save pending task' 8)
  # The current renderer keeps the one-use model request inside a collapsed details element.
  $optional = $null
  $optionalDeadline = [DateTime]::UtcNow.AddSeconds(8)
  do {
    $optionalMatches = @((Ui-Records $root) | Where-Object {
      $_.Name -ceq 'Optional file actions · approval required' -and
      $_.Type -ceq 'ControlType.Button'
    })
    if ($optionalMatches.Count -eq 1) { $optional = $optionalMatches[0]; break }
    if ($optionalMatches.Count -gt 1) { throw 'Multiple native-accessible optional file toggles; refusing ambiguous activation' }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $optionalDeadline)
  if ($null -eq $optional) {
    $nearby = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'file|Optional|Guided|task|approval|plan' -or $_.Type -match 'TreeItem'
    } | Select-Object -First 50 | ForEach-Object { $_.Type + ' ' + $_.Name + ' [' + $_.Id + ']' })
    throw ('Optional file action summary was not accessible; nearby UIA controls: ' + ($nearby -join ' | '))
  }
  Ui-Invoke $optional

  # The task view exposes a single saved localhost provider; select it via an
  # actual ComboBox SelectionItem or ValuePattern without keyboard input.
  $request = Ui-Wait $root 'Ask model for a file proposal · may use credits' '' 8
  if ($null -eq $request) { throw 'New model-proposal button absent from real UIA tree' }
  $result.task_provider_discovery = @(Ui-Discovery $root 100)
  # Prefer the explicit accessible model choice: unlike changing a native
  # Chromium ComboBox's ValuePattern, InvokePattern on this owned WebView
  # button dispatches the actual Svelte click handler and updates its state.
  $quickChoice = Ui-Wait $root 'Use Constellation mock proposal for task execution' '' 8
  if ($null -ne $quickChoice) {
    if ($quickChoice.Type -cne 'ControlType.Button' -or -not $quickChoice.Element.Current.IsEnabled) {
      throw 'Local model quick-choice is not an enabled accessible UIA button'
    }
    Ui-Invoke $quickChoice
    # This button uses a real Svelte click handler. Require the task operation
    # to become enabled before issuing any simulated provider request.
    $request = Ui-WaitEnabled $root 'Ask model for a file proposal · may use credits' 8
    $result.provider_selection_screenshot = Capture-Window $hwnd 'model-provider-quick-choice'
    if (-not $result.provider_selection_screenshot.content_plausible) {
      throw 'Accessible provider selection screenshot is not visually plausible'
    }
  }
  $request = Ui-Find $root 'Ask model for a file proposal · may use credits' ''
  if ($null -eq $request) { throw 'Model action disappeared after model selection' }
  if (-not $request.Element.Current.IsEnabled) {
    $comboRecords = @((Ui-Records $root) | Where-Object {
      $_.Type -ceq 'ControlType.ComboBox' -and $_.Name -match 'Model for task execution|Choose a provider'
    })
    if ($comboRecords.Count -ne 1) {
      throw "Cannot uniquely identify task provider selector (found $($comboRecords.Count))"
    }
    $combo = $comboRecords[0]
    $expand = $null
    if ($combo.Element.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$expand)) {
      ([Windows.Automation.ExpandCollapsePattern]$expand).Expand()
    }
    $options = @((Ui-Records $root) | Where-Object {
      $_.Type -match 'ListItem|MenuItem' -and $_.Name -match '^Constellation mock proposal'
    })
    $result.task_provider_combo_patterns = @($combo.Element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
    if ($options.Count -eq 1) {
      $selectedPattern = $null
      if (-not $options[0].Element.TryGetCurrentPattern(
          [Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selectedPattern)) {
        throw 'Local mock option lacks SelectionItemPattern'
      }
      ([Windows.Automation.SelectionItemPattern]$selectedPattern).Select()
    } elseif ($options.Count -eq 0) {
      # WebView2 on an unselected Win32 desktop may expose no HTML option
      # elements. The one synthetic provider has one exact known label.
      # ValuePattern is permitted only on that verified private ComboBox.
      $expectedOption = 'Constellation mock proposal ' + [char]0xB7 + ' ' + $modelName +
        ' (Custom ' + [char]0xB7 + ' OpenAI-compatible)'
      $rawValue = $null
      if (-not $combo.Element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$rawValue)) {
        throw 'Local provider option not exposed and ComboBox lacks ValuePattern'
      }
      $valuePattern = [Windows.Automation.ValuePattern]$rawValue
      if ($valuePattern.Current.IsReadOnly) {
        throw 'Local provider options hidden and ComboBox ValuePattern is read-only'
      }
      $valuePattern.SetValue($expectedOption)
      Start-Sleep -Milliseconds 250
      # Chromium may replace a native UIA ComboBox entirely after Svelte's
      # change event. The authoritative verification is the task's previously
      # disabled provider-bound action becoming enabled; it is then re-queried.
    } else {
      throw "Multiple localhost provider options visible: $($options.Count)"
    }
    Start-Sleep -Milliseconds 200
  }
  $path = Join-Path (Join-Path $isolatedData 'local-file-artifacts') $fileName
  if (Test-Path -LiteralPath $path) { throw 'Mock file exists before request' }
  $request = Ui-Find $root 'Ask model for a file proposal · may use credits' ''
  $modelActionEnabled = $null -ne $request -and $request.Element.Current.IsEnabled
  if ($modelActionEnabled) {
    Ui-Invoke $request
    $until = [DateTime]::UtcNow.AddSeconds(15)
    $preview = $null
    do {
      if ($fixture.RejectedCount -ne 0 -or $fixture.RequestCount -gt 1) {
        throw 'Local fixture observed an invalid or duplicate provider request'
      }
      $preview = Ui-Find $root 'Approval required for proposed file action' ''
      if ($preview -and $fixture.RequestCount -eq 1) { break }
      Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $until)
    if ($null -eq $preview -or $fixture.RequestCount -ne 1) {
      throw "No pending model-proposed file action; mock calls=$($fixture.RequestCount)"
    }
    # Chromium's UIA may omit noninteractive <strong> text inside a role=group.
    # One exact localhost model completion plus an immutable preview, without
    # opening manual controls, proves this path reached the model proposal
    # handler. Report label accessibility separately rather than fabricating
    # a successful UIA assertion for non-exposed text.
    $originLabels = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'Model-proposed action' -and $_.Type -match 'Text|Group'
    })
    $manualLabels = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'Review before allowing this one operation' -and $_.Type -match 'Text|Group'
    })
    if ($manualLabels.Count) { throw 'Model reply produced a manual-origin renderer label' }
    $result.model_origin_label_uia = if ($originLabels.Count) {
      'pass_accessible_model_origin_text'
    } else {
      'text_not_exposed_by_private_webview_uia'
    }
    # Capture an actual PrintWindow image of the private, non-input desktop
    # while the task still displays both the accessible provider quick-choice
    # and the model-proposed immutable preview, BEFORE native approval.
    $result.model_preapproval_screenshot = Capture-Window $hwnd 'model-proposal-preapproval'
    if (-not $result.model_preapproval_screenshot.content_plausible) {
      throw 'Model-origin preapproval screenshot is not visually plausible'
    }
    $result.model_proposal_status = 'pass_one_simulated_localhost_tool_proposal_prepared_without_execution'
  } else {
    # Accessibility fallback is separately reported as a failed model-proposal
    # acceptance. It still exercises the native create/read security boundary
    # with a manual operation and an exact local file fixture.
    $result.model_proposal_status = 'blocked_provider_selector_did_not_enable_model_proposal'
    if ($fixture.RequestCount -ne 0 -or $fixture.RejectedCount -ne 0) {
      throw 'Model fixture unexpectedly contacted before an enabled request'
    }
    Ui-Invoke (Ui-WaitEnabled $root 'Review a local file action' 8)
    $manualPath = Ui-Wait $root '' 'file-tool-path' 8
    $manualContent = Ui-Wait $root '' 'file-tool-content' 8
    if ($null -eq $manualPath -or $null -eq $manualContent) {
      throw 'Manual fallback native file operation inputs are inaccessible'
    }
    Ui-SetText $root $manualPath $fileName
    Ui-SetText $root $manualContent $fileText
    Ui-Invoke (Ui-WaitEnabled $root 'Preview exact file action' 8)
    if ($null -eq (Ui-Wait $root 'Approval required for proposed file action' '' 8)) {
      throw 'Manual fallback did not produce immutable create-new preview'
    }
    $result.fixture_type = 'explicit_manual_file_action_fallback_after_model_provider_uia_blocker'
  }
  if (Test-Path -LiteralPath $path) { throw 'File action wrote before independent native consent' }

  $expectedRoot = Join-Path $isolatedData 'local-file-artifacts'
  $private = Ui-NativeConsentPreview $root $fileName $expectedRoot $fileSha
  $nativeArgs = @{
    Root = $root; ApplicationWindow = $hwnd; ProcessId = $process.Id
    DesktopName = $HiddenDesktopWorkerName; Decision = 'Yes'
    Action = 'create_new'; ExpectedPath = $private.canonical_path
    ExpectedTaskId = $private.task_id; ExpectedContentSha256 = $fileSha
    ExpectedContent = $fileText
  }
  $decision = Invoke-ConstellationPrivateNativeConsent @nativeArgs
  if ($decision.verified_native_control_count -ne 8 -or
      -not $decision.verified_full_exact_hex_bytes -or
      $decision.default_button_id -ne 7) {
    throw 'Native dialog did not verify exact full bytes and default No'
  }
  if ($null -eq (Ui-Wait $root 'Audit result: succeeded' '' 8)) {
    throw 'Approved model action lacked native audit success receipt'
  }
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Approved model file missing' }
  if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine $fileSha) {
    throw 'Approved file SHA differs from exactly reviewed model bytes'
  }
  if ([IO.File]::ReadAllText($path) -cne $fileText) {
    throw 'Approved file contents differ from deterministic proposal'
  }
  $result.manual_file_create_status = 'pass_private_native_create_exact_bytes'
  # Verify this independently approved model-origin operation has the exact
  # one-use event sequence recorded in the private authenticated journal.
  # This checks shape/identity; it does not recompute the secret HMAC key.
  $journalRoot = Join-Path $isolatedData 'local-tool-audit'
  $journals = @(Get-ChildItem -LiteralPath $journalRoot -Filter 'file-operations-*.jsonl' -File)
  if ($journals.Count -ne 1) { throw 'Expected one isolated authenticated journal' }
  $journalStream = [IO.File]::Open($journals[0].FullName,
    [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
  try {
    $journalReader = New-Object IO.StreamReader($journalStream)
    try { $journalText = $journalReader.ReadToEnd() }
    finally { $journalReader.Dispose() }
  } finally { $journalStream.Dispose() }
  if (-not $journalText.EndsWith("`n")) { throw 'Authenticated journal record truncated' }
  $rows = @()
  $previous = '0' * 64
  foreach ($line in ($journalText -split "`n")) {
    if (-not $line.Trim()) { continue }
    $entry = ConvertFrom-Json -InputObject $line
    if ($entry.sequence -ne $rows.Count -or $entry.previous_tag -cne $previous -or
        $entry.tag -notmatch '^[a-fA-F0-9]{64}$' -or $null -eq $entry.record) {
      throw 'Authenticated journal sequence or envelope invalid'
    }
    $rows += $entry.record
    $previous = [string]$entry.tag
  }
  $events = @($rows | Where-Object {
    if ($_.event -eq 'finish') { $_.receipt.operation_id -ieq $decision.operation_id }
    else { $_.id -ieq $decision.operation_id }
  })
  if ($events.Count -ne 3) { throw 'Native model proposal lacks three matching journal transitions' }
  # The current scoped approval journal records approve_scoped, not the
  # legacy unscoped approve event. Requiring that stronger event is essential:
  # it links the exact native scope and one-use receipt.
  foreach ($event in @('approve_scoped', 'start', 'finish')) {
    $selected = @($events | Where-Object { $_.event -ceq $event })
    if ($selected.Count -ne 1) { throw "Missing native audit event: $event" }
    $digest = if ($event -eq 'finish') { $selected[0].receipt.digest } else { $selected[0].digest }
    if ($digest -ine $decision.operation_digest) { throw 'Native consent digest differs from journal' }
  }
  $result.audit_status = 'pass_approve_start_finish_exact_native_id_digest'
  $result.audit_event_count = $rows.Count
  $result.native_operation_id = $decision.operation_id
  $result.native_operation_digest = $decision.operation_digest
  $result.native_consent_status = 'pass_real_native_yes_exact_bytes_one_use'
  $result.output_file_sha256 = $fileSha
  $result.screenshot = $decision.screenshot
  $result.native_create_screenshot = $decision.screenshot
  $result.requests = $fixture.RequestCount
  $result.rejected_requests = $fixture.RejectedCount

  # The latest Rust host adds a separate first Win32 MessageBox for read
  # inspection. Exercise both explicit decisions using only this private HWND.
  # Existing windows-evidence-backup-e2e.ps1 predates this first-stage dialog.
  Ui-Invoke (Ui-WaitEnabled $root 'Read an existing text file' 8)
  Ui-SetText $root (Ui-Require $root '' 'file-tool-path') $fileName
  $firstReadArgs = @{
    Root = $root; ApplicationWindow = $hwnd; ProcessId = $process.Id
    DesktopName = $HiddenDesktopWorkerName; Decision = 'Yes'
    ExpectedTaskId = $private.task_id; ExpectedRelativePath = $fileName
  }
  $firstRead = Invoke-ConstellationPrivateReadIntent @firstReadArgs
  if ($firstRead.default_button_id -ne 7 -or
      -not $firstRead.verified_no_file_metadata_in_native_prompt) {
    throw 'First read-inspection consent lacked default No or leaked metadata'
  }
  $result.first_read_intent_status = 'pass_first_owned_messagebox_yes_no_metadata_default_no'
  $result.native_first_read_screenshot = $firstRead.screenshot
  $readPreview = Ui-Wait $root 'Approval required for proposed file action' '' 8
  if ($null -eq $readPreview) { throw 'Second-stage immutable read preview absent after inspection consent' }
  $readFields = Ui-NativeConsentPreview $root $fileName $expectedRoot $fileSha ([Text.Encoding]::UTF8.GetByteCount($fileText))
  if ($readFields.task_id -cne $private.task_id) {
    throw 'Read consent changed task identity after independent inspection'
  }
  $readArgs = @{
    Root = $root; ApplicationWindow = $hwnd; ProcessId = $process.Id
    DesktopName = $HiddenDesktopWorkerName; Decision = 'Yes'
    Action = 'read'; ExpectedPath = $readFields.canonical_path
    ExpectedTaskId = $private.task_id; ExpectedContentSha256 = $fileSha
    ExpectedReadByteCount = [Text.Encoding]::UTF8.GetByteCount($fileText)
  }
  $readApproval = Invoke-ConstellationPrivateNativeConsent @readArgs
  $result.native_final_read_screenshot = $readApproval.screenshot
  if ($readApproval.verified_native_control_count -ne 8 -or
      -not $readApproval.verified_file_sha256 -or
      $readApproval.operation_id -eq $decision.operation_id) {
    throw 'Second-stage native read approval missing hash or unique operation ID'
  }
  if ($null -eq (Ui-Wait $root 'Audit result: succeeded' '' 8) -or
      $null -eq (Ui-Wait $root $fileText '' 8)) {
    throw 'Explicitly approved read did not expose exact original text and success receipt'
  }
  if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine $fileSha) {
    throw 'Read inspection modified the isolated source file'
  }
  $result.final_read_approval_status = 'pass_separate_sha_bound_native_yes_read_exact_content'
} catch {
  $result.blockers += $_.Exception.GetBaseException().Message
} finally {
  try {
    if ((Assert-PinnedConstellationSources $ExpectedSourceIdentityPath $ExpectedNativeSourceSha256) -cne $sourceIdentitySha) {
      $result.blockers += 'Eleven-source manifest drift during UI session'
    }
    if ((Assert-PinnedFullSourceAndAssets $fullSourceManifest $fullSourceManifestSha256) -cne $fullManifestVerifiedSha256) {
      $result.blockers += 'Complete 187-source-and-assets manifest drift during UI session'
    }
    if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {
      $result.blockers += 'EXE SHA drift during UI session'
    }
  } catch { $result.blockers += 'Identity recheck failed: ' + $_.Exception.Message }
  $result.requests = $fixture.RequestCount
  $result.rejected_requests = $fixture.RejectedCount
  $fixture.Dispose()
  if ($null -ne $process) {
    try {
      $process.Refresh()
      if (-not $process.HasExited) {
        [void]$process.CloseMainWindow()
        if (-not $process.WaitForExit(4000)) {
          $process.Kill()
          [void]$process.WaitForExit(5000)
        }
      }
      $result.process_closed = $process.HasExited
    } catch { $result.blockers += 'Could not close exclusively owned QA PID' }
    finally { $process.Dispose() }
  }
}
$receipt = Join-Path $artifacts ('model-proposal-' + $runId + '.json')
($result | ConvertTo-Json -Depth 8) | Set-Content -LiteralPath $receipt -Encoding UTF8
$result | ConvertTo-Json -Depth 8
if ($result.model_proposal_status -ne 'pass_one_simulated_localhost_tool_proposal_prepared_without_execution' -or
    $result.native_consent_status -ne 'pass_real_native_yes_exact_bytes_one_use' -or
    $result.audit_status -ne 'pass_approve_start_finish_exact_native_id_digest' -or
    $result.first_read_intent_status -ne 'pass_first_owned_messagebox_yes_no_metadata_default_no' -or
    $result.final_read_approval_status -ne 'pass_separate_sha_bound_native_yes_read_exact_content' -or
    -not $result.process_closed -or $result.blockers.Count -gt 0 -or
    $result.requests -ne 1 -or $result.rejected_requests -ne 0) { exit 1 }
