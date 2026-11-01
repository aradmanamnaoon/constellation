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
$fileName = 'native-multistep-read-fixture.txt'
$fileText = "Private preexisting local file; require two independent OS permissions. No AI inference.`n"
$proposal = @{ type = 'tool'; action = 'read_file'; path = $fileName } |
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

  # Independent real-WebView2 guided multi-step denial test. All provider
  # responses are explicit local fixtures, never actual AI inference.
  $result.fixture_type = 'simulated_localhost_openai_multistep_first_read_permission_denied_not_ai'
  $result | Add-Member -NotePropertyName multistep_status -NotePropertyValue 'pending'
  $result | Add-Member -NotePropertyName multistep_approval -NotePropertyValue 'pending'
  $result | Add-Member -NotePropertyName multistep_read_intent -NotePropertyValue 'pending'
  $result | Add-Member -NotePropertyName multistep_effects_journal -NotePropertyValue 'pending'
  $result | Add-Member -NotePropertyName multistep_signed_checkpoint -NotePropertyValue 'pending'
  $quickChoice = Ui-WaitEnabled $root 'Use Constellation mock proposal for task execution' 8
  Ui-Invoke $quickChoice
  $group = @((Ui-Records $root) | Where-Object {
    $_.Name -ceq 'Guided multi-step work · separate approval for every tool' -and
    $_.Type -ceq 'ControlType.Button'
  })
  if ($group.Count -ne 1) { throw "Cannot uniquely open guided multi-step section ($($group.Count) accessible toggles)" }
  Ui-Invoke $group[0]
  $request = Ui-WaitEnabled $root 'Request one next model step · may incur charges' 8
  $path = Join-Path (Join-Path $isolatedData 'local-file-artifacts') $fileName
  # This file is created only by the trusted harness as *preexisting* private
  # data, never by the model or from a native tool effect. Its bytes must stay
  # untouched through proposal, first permission and final read.
  if (Test-Path -LiteralPath $path) { throw 'A private read fixture already exists' }
  [IO.File]::WriteAllText($path, $fileText, (New-Object Text.UTF8Encoding($false)))
  if ((Get-FileHash $path -Algorithm SHA256).Hash -ine $fileSha) {
    throw 'Harness private read fixture digest mismatch'
  }
  Ui-Invoke $request
  $until = [DateTime]::UtcNow.AddSeconds(22)
  $review = $null
  $deny = $null
  do {
    if ($fixture.RejectedCount -ne 0 -or $fixture.RequestCount -gt 1) {
      throw 'Invalid or duplicate local model invocation during guided multi-step'
    }
    $review = Ui-Find $root 'Review exact operation in native OS dialog' ''
    $deny = Ui-Find $root 'Deny this operation' ''
    if ($review -and $deny -and $fixture.RequestCount -eq 1) { break }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $until)
  if ($null -eq $review -or $null -eq $deny -or $fixture.RequestCount -ne 1) {
    throw "Guided multi-step proposal not exposed; simulated model calls=$($fixture.RequestCount)"
  }
  if (-not $review.Element.Current.IsEnabled -or -not $deny.Element.Current.IsEnabled) {
    throw 'Authenticated tool review and refusal actions are not both enabled'
  }
  if ((Get-FileHash $path -Algorithm SHA256).Hash -ine $fileSha) {
    throw 'Model proposal modified private file before native inspection consent'
  }
  $result.multistep_status = 'pass_one_localhost_read_proposal_pending_without_metadata'
  $result.model_proposal_status = 'pass_one_simulated_localhost_tool_proposal_prepared_without_execution'
  $result.model_preapproval_screenshot = Capture-Window $hwnd 'multistep-pending-native-approval'
  if (-not $result.model_preapproval_screenshot.content_plausible) {
    throw 'Pending native approval screenshot did not show credible app rendering'
  }
  # Read-only inspection must not approve anything or contact the provider.
  Ui-Invoke (Ui-WaitEnabled $root 'Inspect step status · read-only' 8)
  if ($fixture.RequestCount -ne 1 -or (Get-FileHash $path -Algorithm SHA256).Hash -ine $fileSha) {
    throw 'Read-only inspection contacted provider or mutated private document'
  }
  # Independently inspect the local Core task database. This test receives
  # no renderer-supplied authority, credential or user-provided operation ID.
  $taskDb = Join-Path $isolatedData 'constellation.sqlite3'
  $query = "const {DatabaseSync}=require('node:sqlite');const db=new DatabaseSync(process.argv[1],{readOnly:true});const tasks=db.prepare('select id,data from tasks').all().filter(t=>JSON.parse(t.data).state==='pending');if(tasks.length!==1||JSON.parse(tasks[0].data).instructions!=='Propose a small text artifact for this disposable QA task.')process.exit(2);console.log(tasks[0].id);"
  $taskId = [string](& node --no-warnings -e $query $taskDb | Out-String).Trim()
  if ($LASTEXITCODE -ne 0 -or $taskId -notmatch '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$') {
    throw 'No unique independently saved pending Core task'
  }
  $canonicalExpected = '\\?\' + [IO.Path]::GetFullPath($path)
  $firstReadArgs = @{
    Root = $root; ApplicationWindow = $hwnd; ProcessId = $process.Id
    DesktopName = $HiddenDesktopWorkerName; Decision = 'No'
    ExpectedTaskId = $taskId; ExpectedRelativePath = $fileName
    ExpectedPrivatePath = $canonicalExpected; ExpectedFixtureSha256 = $fileSha
    ExpectedFixtureByteCount = [Text.Encoding]::UTF8.GetByteCount($fileText)
    RendererReviewLabel = 'Review exact operation in native OS dialog'
    TimeoutSeconds = 20
  }
  $firstDecision = Invoke-ConstellationPrivateReadIntent @firstReadArgs
  if ($firstDecision.default_button_id -ne 7 -or
      -not $firstDecision.verified_no_file_metadata_in_native_prompt) {
    throw 'First native read-inspection permission leaked bytes or lacked default No'
  }
  $result.native_first_read_screenshot = $firstDecision.screenshot
  $result.multistep_read_intent = 'pass_first_native_read_intent_denied_default_no'
  if ((Get-FileHash $path -Algorithm SHA256).Hash -ine $fileSha) {
    throw 'First read permission mutated the source document'
  }
  # No second native review is allowed after first-stage denial. The pending
  # proposal must settle as a denial with no execute-capable journal record.
  if ([ConstellationPrivateConsentDialog]::Find($process.Id, $hwnd) -ne [IntPtr]::Zero) {
    throw 'Second native file consent appeared despite denied inspection permission'
  }
  $until = [DateTime]::UtcNow.AddSeconds(12)
  $next = $null
  do {
    $next = Ui-Find $root 'Request one next model step · may incur charges' ''
    if ($next -and $next.Element.Current.IsEnabled) { break }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $until)
  if ($null -eq $next -or -not $next.Element.Current.IsEnabled) {
    $result | Add-Member -NotePropertyName post_approval_screenshot -NotePropertyValue (Capture-Window $hwnd 'multistep-post-denied-read-diagnostic')
    $observed = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'Authenticated step|provider attempts|model step|operation|audit|status|quarantined|unverified' -or $_.Type -ceq 'ControlType.Button'
    } | Select-Object -First 50 | ForEach-Object {
      $_.Type + ' ' + $_.Name + ' [enabled=' + $_.Element.Current.IsEnabled + ']'
    })
    throw ('First native read denial did not settle to an explicit next-model action: ' + ($observed -join ' | '))
  }
  if ($fixture.RequestCount -ne 1 -or $fixture.RejectedCount -ne 0) {
    throw 'Native read denial triggered an unauthorized extra provider request'
  }
  if (Ui-Find $root 'Review exact operation in native OS dialog' '') {
    throw 'Denied native read step remained offered for replay'
  }
  if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine $fileSha -or
      [IO.File]::ReadAllText($path) -cne $fileText) {
    throw 'Native read denial changed private file'
  }
  $result.multistep_approval = 'pass_first_read_permission_denied_no_second_dialog_or_effect'
  $result.output_file_sha256 = $fileSha
  $journal = Join-Path (Join-Path $isolatedData 'local-tool-audit') 'native-multi-step-effects.jsonl'
  $effectEvents = @()
  if (Test-Path -LiteralPath $journal -PathType Leaf) {
    foreach ($line in @(Get-Content -LiteralPath $journal)) {
      if (-not $line.Trim()) { continue }
      $envelope = ConvertFrom-Json -InputObject $line
      $event = [string]$envelope.record.event
      if ($event -in @('approve','approve_scoped','start','finish')) { $effectEvents += $event }
    }
  }
  if ($effectEvents.Count -ne 0) {
    throw ('Denied native read should have no executable effect journal events: ' + ($effectEvents -join ','))
  }
  $result.multistep_effects_journal = 'pass_no_effect_after_denied_first_read_inspection'
  $checkpoint = Join-Path (Join-Path $isolatedData 'local-tool-audit') 'native-multi-step-checkpoints.sqlite3'
  if (-not (Test-Path -LiteralPath $checkpoint -PathType Leaf) -or (Get-Item -LiteralPath $checkpoint).Length -lt 256) {
    throw 'Approved multi-step run has no durable signed state database'
  }
  $result.multistep_signed_checkpoint = 'pass_durable_local_checkpoint_present_after_approval'
  $result.requests = $fixture.RequestCount
  $result.rejected_requests = $fixture.RejectedCount
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
$receipt = Join-Path $artifacts ('native-multistep-first-read-denial-' + $runId + '.json')
($result | ConvertTo-Json -Depth 8) | Set-Content -LiteralPath $receipt -Encoding UTF8
$result | ConvertTo-Json -Depth 8
if ($result.multistep_status -ne 'pass_one_localhost_read_proposal_pending_without_metadata' -or
    $result.multistep_read_intent -ne 'pass_first_native_read_intent_denied_default_no' -or
    $result.multistep_approval -ne 'pass_first_read_permission_denied_no_second_dialog_or_effect' -or
    $result.multistep_effects_journal -ne 'pass_no_effect_after_denied_first_read_inspection' -or
    $result.multistep_signed_checkpoint -ne 'pass_durable_local_checkpoint_present_after_approval' -or
    $result.requests -ne 1 -or $result.rejected_requests -ne 0 -or
    -not $result.process_closed -or $result.blockers.Count -gt 0) { exit 1 }
