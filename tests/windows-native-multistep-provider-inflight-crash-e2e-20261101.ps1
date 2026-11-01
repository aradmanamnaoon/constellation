# Source-pinned private-desktop UIA test of a provider HTTP request interrupted
# after the localhost server has received it but BEFORE any HTTP response.
# A delayed localhost fixture is NOT a model or a paid/provider-backed test.
# No visible user-desktop input, live installation or NSIS lifecycle is used.
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
$oldFields = 'private int stopped, requestCount, rejectedCount;'
$newFields = 'private int stopped, requestCount, rejectedCount, replyCount, connectionCount; private readonly ManualResetEventSlim release = new ManualResetEventSlim(false); public int ReplyCount { get { return Interlocked.CompareExchange(ref replyCount, 0, 0); } } public int ConnectionCount { get { return Interlocked.CompareExchange(ref connectionCount, 0, 0); } } public void Release() { release.Set(); }'
if (-not $fixtureCsharp.Contains($oldFields)) { throw 'Local HTTP fixture fields changed; refuse uncertain instrumentation' }
$fixtureCsharp = $fixtureCsharp.Replace($oldFields, $newFields)
$acceptPoint = 'client.ReceiveTimeout = 5000;'
if (-not $fixtureCsharp.Contains($acceptPoint)) { throw 'Local HTTP accept accounting point missing' }
$fixtureCsharp = $fixtureCsharp.Replace($acceptPoint, 'Interlocked.Increment(ref connectionCount); ' + $acceptPoint)
$oldResult = 'public const string Result = "CONSTELLATION_LOCALHOST_WORKFLOW_E2E_SUCCESS";'
$replacement = 'public static readonly string Result = Encoding.UTF8.GetString(Convert.FromBase64String("' + $encoded + '"));'
if (-not $fixtureCsharp.Contains($oldResult)) { throw 'HTTP helper result contract changed' }
$fixtureCsharp = $fixtureCsharp.Replace($oldResult, $replacement)
$waitPoint = 'string json = valid'
if (-not $fixtureCsharp.Contains($waitPoint)) { throw 'Local HTTP response barrier insertion point missing' }
$fixtureCsharp = $fixtureCsharp.Replace($waitPoint, 'if (valid) release.Wait(); string json = valid')
$responsePoint = 'stream.Write(payload, 0, payload.Length);'
if (-not $fixtureCsharp.Contains($responsePoint)) { throw 'Local HTTP response accounting point missing' }
$fixtureCsharp = $fixtureCsharp.Replace($responsePoint, $responsePoint + ' Interlocked.Increment(ref replyCount);')
$disposePoint = 'Interlocked.Exchange(ref stopped, 1);'
if (-not $fixtureCsharp.Contains($disposePoint)) { throw 'Local HTTP fixture release-on-dispose point missing' }
$fixtureCsharp = $fixtureCsharp.Replace($disposePoint, $disposePoint + ' release.Set();')
Add-Type -TypeDefinition $fixtureCsharp -ReferencedAssemblies 'System' -ErrorAction Stop
$fixture = New-Object ConstellationLocalWorkflowFixture
$modelName = [ConstellationLocalWorkflowFixture]::Model
$fileSha = [BitConverter]::ToString(
  [Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($fileText))
).Replace('-', '').ToLowerInvariant()
$result = [ordered]@{
  fixture_type = 'explicit_localhost_delayed_http_request_no_model_no_response_before_crash'
  run_id = $runId
  exe_sha256 = $ExpectedExeSha256.ToLowerInvariant()
  native_source_sha256 = $ExpectedNativeSourceSha256.ToLowerInvariant()
  source_identity_manifest_sha256 = $sourceIdentitySha
  source_and_assets_187_manifest_sha256 = $fullManifestVerifiedSha256
  helper_smoke_sha256 = (Get-FileHash -LiteralPath $smokeSource -Algorithm SHA256).Hash.ToLowerInvariant()
  helper_fixture_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'helper-local-workflow-fixture.ps1') -Algorithm SHA256).Hash.ToLowerInvariant()
  hidden_desktop = $HiddenDesktopWorkerName
  pid = 0; requests = 0; connections = 0; rejected_requests = 0
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

  # The listener receives the real native HTTP request and blocks before
  # writing its HTTP status line. There is no model reply, tool proposal, or
  # authorization. The app PID is killed only while the provider is unresolved.
  $result.fixture_type = 'delayed_loopback_provider_request_received_no_http_response'
  $result | Add-Member -NotePropertyName multistep_status -NotePropertyValue 'pending'
  $result | Add-Member -NotePropertyName multistep_restart_quarantine -NotePropertyValue 'pending'
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
  if (Test-Path -LiteralPath $path) { throw 'Pre-existing fixture output before HTTP request' }
  Ui-Invoke $request
  $until = [DateTime]::UtcNow.AddSeconds(22)
  do {
    if ($fixture.RejectedCount -ne 0 -or $fixture.RequestCount -gt 1) {
      throw 'Unexpected or duplicate provider HTTP request during in-flight reservation'
    }
    if ($fixture.RequestCount -eq 1) { break }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $until)
  if ($fixture.RequestCount -ne 1 -or $fixture.ReplyCount -ne 0) {
    throw "Did not establish one unanswered provider HTTP request; received=$($fixture.RequestCount), replies=$($fixture.ReplyCount)"
  }
  if ($fixture.ConnectionCount -ne 1) {
    throw "Unexpected HTTP connection count before crash: $($fixture.ConnectionCount)"
  }
  if ((Ui-Find $root 'Review exact operation in native OS dialog' '') -or
      (Ui-Find $root 'Deny this operation' '')) {
    throw 'Native operation approval was exposed without receiving a model response'
  }
  if (Test-Path -LiteralPath $path) { throw 'Unanswered HTTP request created an unauthorized file' }
  $checkpoint = Join-Path (Join-Path $isolatedData 'local-tool-audit') 'native-multi-step-checkpoints.sqlite3'
  if (-not (Test-Path -LiteralPath $checkpoint -PathType Leaf) -or (Get-Item -LiteralPath $checkpoint).Length -lt 256) {
    throw 'No persisted authenticated checkpoint while provider HTTP was in flight'
  }
  $result.multistep_status = 'pass_one_provider_request_received_while_no_http_reply_sent'
  $result.model_proposal_status = 'pass_no_provider_reply_no_model_proposal_no_os_authorization'
  $result.model_preapproval_screenshot = Capture-Window $hwnd 'provider-http-unanswered-before-crash'
  if (-not $result.model_preapproval_screenshot.content_plausible) {
    throw 'In-flight provider screenshot did not show credible app rendering'
  }
  # Simulate host crash *before* the HTTP response is available. Only the
  # disposable source-pinned test PID may be killed.
  $originalPid = $process.Id
  $process.Kill()
  if (-not $process.WaitForExit(6000) -or -not $process.HasExited) {
    throw 'Failed to stop only the disposable first Constellation PID'
  }
  $process.Dispose()
  $process = $null
  $result | Add-Member -NotePropertyName first_process_terminated -NotePropertyValue $true
  if ($fixture.RequestCount -ne 1 -or $fixture.ConnectionCount -ne 1 -or
      $fixture.ReplyCount -ne 0 -or (Test-Path -LiteralPath $path)) {
    throw 'Provider-in-flight crash already replied, retried or created a file'
  }
  # Unblock the local listener after the test PID is dead. Its socket may have
  # been reset; this is NOT a confirmed provider-side rollback or charge refund.
  $fixture.Release()
  # Relaunch on the SAME isolated data and OS vault, a new process/window on
  # the SAME separate non-input desktop. The uncertain charged provider request
  # must quarantine without a second HTTP request or restored tool authority.
  # WebView2 browser subprocesses can outlive a force-killed Tauri host.
  # Use a fresh disposable browser-profile folder while keeping the EXACT
  # same persistent Constellation data directory and OS vault identity.
  $restartWebView2 = Join-Path $sandbox 'RestartWebView2'
  [void](New-Item -ItemType Directory -Path $restartWebView2)
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'] = $restartWebView2
  $psi.Arguments = '--resume-isolated-data-dir "' + $isolatedData + '"'
  Start-Sleep -Milliseconds 900
  $restart = New-Object Diagnostics.Process
  $restart.StartInfo = $psi
  if (-not $restart.Start()) { throw 'Cannot restart identical isolated app' }
  $process = $restart
  $result | Add-Member -NotePropertyName restarted_pid -NotePropertyValue $restart.Id
  if ($restart.Id -eq $originalPid) { throw 'Unexpected process PID reuse in immediate restart' }
  $root = $null
  $hwnd = [IntPtr]::Zero
  $readyUntil = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    $restart.Refresh()
    if ($restart.HasExited) { throw "Restarted app exited during recovery startup with exit_code=$($restart.ExitCode)" }
    $hwnd = [ConstellationEvidenceWindow]::FindOwnMainWindow($restart.Id)
    if ($hwnd -ne [IntPtr]::Zero) {
      Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
      $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
      $initial = @(Ui-Records $root)
      if (@($initial | Where-Object { $_.Name -ceq 'Tasks' }).Count -ge 1 -and
          @($initial | Where-Object { $_.Name -ceq 'Settings' }).Count -eq 1) { break }
    }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $readyUntil)
  if ($null -eq $root -or $hwnd -eq [IntPtr]::Zero) { throw 'Restarted WebView failed to load isolated home' }
  Ui-Invoke (Ui-Require $root 'Tasks' '')
  if ($null -eq (Ui-Wait $root 'Workflows' '' 8)) { throw 'Recovered Tasks view not loaded' }
  $optional = $null
  $untilOptional = [DateTime]::UtcNow.AddSeconds(8)
  do {
    $matches = @((Ui-Records $root) | Where-Object {
      $_.Name -ceq 'Optional file actions · approval required' -and
      $_.Type -ceq 'ControlType.Button'
    })
    if ($matches.Count -eq 1) { $optional = $matches[0]; break }
    if ($matches.Count -gt 1) { throw 'Ambiguous recovered optional panel' }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $untilOptional)
  if ($null -eq $optional) { throw 'No recovered task panel to inspect' }
  Ui-Invoke $optional
  $select = Ui-WaitEnabled $root 'Use Constellation mock proposal for task execution' 8
  Ui-Invoke $select
  $guided = @((Ui-Records $root) | Where-Object {
    $_.Name -ceq 'Guided multi-step work · separate approval for every tool' -and
    $_.Type -ceq 'ControlType.Button'
  })
  if ($guided.Count -ne 1) { throw 'No uniquely accessible recovered guided task' }
  Ui-Invoke $guided[0]
  Ui-Invoke (Ui-WaitEnabled $root 'Inspect step status · read-only' 8)
  $errorUntil = [DateTime]::UtcNow.AddSeconds(8)
  $notice = $null
  do {
    $notice = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'interrupted request or unresolved approval is quarantined' -and
      $_.Type -ceq 'ControlType.Text'
    })
    if ($notice.Count -gt 0) { break }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $errorUntil)
  if ($notice.Count -eq 0) {
    $result | Add-Member -NotePropertyName post_restart_screenshot -NotePropertyValue (Capture-Window $hwnd 'multistep-restart-uncertain-diagnostic')
    $observed = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'quarantined|interrupt|model step|review|not available|verified|status' -or
      $_.Type -eq 'ControlType.Button'
    } | Select-Object -First 35 -ExpandProperty Name)
    throw ('No visible authenticated quarantine after restart: ' + ($observed -join ' | '))
  }
  if (Ui-Find $root 'Review exact operation in native OS dialog' '') {
    throw 'Unanswered model request was restored with executable OS review authority'
  }
  $requestAfter = Ui-Find $root 'Request one next model step · may incur charges' ''
  if ($requestAfter -and $requestAfter.Element.Current.IsEnabled) {
    throw 'Uncertain model request recovered as a retryable provider step'
  }
  # A malformed retry would increment ConnectionCount without incrementing
  # RequestCount. The first connection has closed; this fixture does not
  # implement HTTP keepalive or multiple request parsing on one socket.
  Start-Sleep -Milliseconds 1500
  if ($fixture.RequestCount -ne 1 -or $fixture.ConnectionCount -ne 1 -or
      (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Recovered app replayed an uncertain provider/tool effect; connections=$($fixture.ConnectionCount), valid POSTs=$($fixture.RequestCount)"
  }
  $result | Add-Member -NotePropertyName recovered_quarantine_screenshot -NotePropertyValue (
    Capture-Window $hwnd 'multistep-recovered-quarantined')
  if (-not $result.recovered_quarantine_screenshot.content_plausible) {
    throw 'Recovered quarantine UI did not have plausible visible rendering'
  }
  $result.multistep_restart_quarantine = 'pass_restart_quarantines_unanswered_provider_request_no_retry_or_permission'
  $journal = Join-Path (Join-Path $isolatedData 'local-tool-audit') 'native-multi-step-effects.jsonl'
  $dangerousEvents = @()
  if (Test-Path -LiteralPath $journal -PathType Leaf) {
    foreach ($line in @(Get-Content -LiteralPath $journal)) {
      if (-not $line.Trim()) { continue }
      $envelope = ConvertFrom-Json -InputObject $line
      $event = [string]$envelope.record.event
      if ($event -in @('approve','approve_scoped','start','finish')) { $dangerousEvents += $event }
    }
  }
  if ($dangerousEvents.Count -ne 0) {
    throw ('Unanswered provider request generated an executable effect event: ' + ($dangerousEvents -join ','))
  }
  $result.multistep_effects_journal = 'pass_no_approve_start_or_finish_effect_event'
  $checkpoint = Join-Path (Join-Path $isolatedData 'local-tool-audit') 'native-multi-step-checkpoints.sqlite3'
  if (-not (Test-Path -LiteralPath $checkpoint -PathType Leaf) -or (Get-Item -LiteralPath $checkpoint).Length -lt 256) {
    throw 'Multi-step denied run has no durable signed state database'
  }
  $result.multistep_signed_checkpoint = 'pass_durable_checkpoint_existed_before_crash_and_after_restart'
  $result.requests = $fixture.RequestCount
  $result.connections = $fixture.ConnectionCount
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
  $result.connections = $fixture.ConnectionCount
  $result.rejected_requests = $fixture.RejectedCount
  $fixture.Release()
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
$receipt = Join-Path $artifacts ('native-provider-inflight-crash-' + $runId + '.json')
($result | ConvertTo-Json -Depth 8) | Set-Content -LiteralPath $receipt -Encoding UTF8
$result | ConvertTo-Json -Depth 8
if ($result.multistep_status -ne 'pass_one_provider_request_received_while_no_http_reply_sent' -or
    $result.multistep_restart_quarantine -ne 'pass_restart_quarantines_unanswered_provider_request_no_retry_or_permission' -or
    $result.multistep_effects_journal -ne 'pass_no_approve_start_or_finish_effect_event' -or
    $result.multistep_signed_checkpoint -ne 'pass_durable_checkpoint_existed_before_crash_and_after_restart' -or
    $result.requests -ne 1 -or $result.connections -ne 1 -or
    -not $result.process_closed -or $result.blockers.Count -gt 0) { exit 1 }
