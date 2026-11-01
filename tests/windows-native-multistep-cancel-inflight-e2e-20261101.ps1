# Deterministic in-flight PROVIDER HTTP cancellation against an unsigned,
# source-manifest-pinned Constellation EXE on a unique, NON-INPUT Windows desktop.
# This does not test cancellation during a blocking file effect. A local server
# holds the response until Core cancellation is independently confirmed.
# No browser, user desktop, real model or outbound network is used.
[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][string]$ExePath,
  [Parameter(Mandatory=$true)][string]$ExpectedExeSha256,
  [Parameter(Mandatory=$true)][string]$ExpectedNativeSourceSha256,
  [Parameter(Mandatory=$true)][string]$ExpectedSourceIdentityPath,
  [string]$OutputDirectory,
  [ValidateRange(15,90)][int]$TimeoutSeconds = 45,
  [switch]$TestNativeConsent,
  [string]$HiddenDesktopWorkerName
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Native hidden-desktop QA requires Windows' }
foreach ($hash in @($ExpectedExeSha256, $ExpectedNativeSourceSha256)) {
  if ($hash -notmatch '^[0-9a-fA-F]{64}$') { throw 'Invalid SHA256 launch gate' }
}
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$exe = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).ProviderPath
$exeHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
if ($exeHash -ine $ExpectedExeSha256) { throw 'EXE identity differs; no launch' }
if ([IO.Path]::GetExtension($exe) -ine '.exe') { throw 'Expected a Windows production executable' }

# The 187-file immutable prebuild manifest was generated during the production
# build represented by this precise executable (see QA provenance report).
# Never treat a user-supplied EXE hash alone as evidence of source linkage.
$manifestPath = Join-Path $env:LOCALAPPDATA 'Temp\constellation-source-20261031-prime\current-source-pinned-187-20261031.json'
$expectedManifest = '577537DEB0DCE77C7A2FFF7E87AF6AC2ECE851FB57EC0D927BAE7A2135BEF94D'
$knownExe = '1E5C79BA1C255496B6EB351C512C3F12D429AA8B0EA57E90FDD0EE36C93EEA61'
$knownRuntime = 'D7A7DDEAF972FEC13E82F29E92CD1A3848CA5F2B65E55EF56E3567A7E3609A3F'
$knownLib = '1C09E5C3457B4F1B91DE36FB31281206EE7CE368FE247F0FFA5AC5BA307A6194'
$knownUi = '6776CFFAEC47836FAC4A27DD3410E0709B1643E371FE61248D8EB3ACEE0ACD59'
$knownNative = 'FEFB4CFB845421217A29C66267DD3EC0DA171AB50C08A99A94A17907D9B3BD99'
if ($exeHash -ine $knownExe) { throw 'This script has not qualified the supplied EXE for source-linked live cancellation. No launch.' }
if ($ExpectedNativeSourceSha256 -ine $knownNative) { throw 'Native consent hash not tied to the verified executable' }

function Assert-SourceIdentity {
  if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or
      (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash -ine $expectedManifest) {
    throw 'Production source/asset provenance manifest missing or altered'
  }
  $rows = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($manifestPath))
  if ($rows.Count -ne 187) { throw 'Production source manifest length changed' }
  $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)
  foreach ($entry in $rows) {
    $name = [string]$entry.Path
    if ($name -notmatch '^[a-zA-Z0-9][a-zA-Z0-9./_+@ -]*$' -or
        $name.Contains('//') -or $name.StartsWith('../') -or
        $name.Contains('/../') -or $name.Contains('/./') -or
        $name.EndsWith('/..') -or -not $seen.Add($name) -or
        [string]$entry.Hash -notmatch '^[0-9a-fA-F]{64}$') {
      throw 'Unsafe, repeated or malformed manifest entry'
    }
    $path = Join-Path $repo $name.Replace('/', '\')
    $file = Get-Item -LiteralPath $path -ErrorAction Stop
    if ($file.PSIsContainer -or $file.Length -ne [long]$entry.Size -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ine [string]$entry.Hash) {
      throw "Production source drift at $name"
    }
  }
  foreach ($pinned in @(
    @('apps/desktop/src-tauri/src/native_multi_step_runtime.rs', $knownRuntime),
    @('apps/desktop/src-tauri/src/lib.rs', $knownLib),
    @('apps/desktop/src/routes/+page.svelte', $knownUi),
    @('apps/desktop/src-tauri/src/native_consent.rs', $knownNative)
  )) {
    $matching = @($rows | Where-Object { $_.Path -ceq $pinned[0] })
    if ($matching.Count -ne 1 -or [string]$matching[0].Hash -ine $pinned[1]) {
      throw "Missing critical P1 or UI source pin: $($pinned[0])"
    }
  }
  $source11 = (Resolve-Path -LiteralPath $ExpectedSourceIdentityPath -ErrorAction Stop).ProviderPath
  $eleven = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($source11))
  if ($eleven.Count -ne 11) { throw 'Native eleven-source provenance malformed' }
  foreach ($row in $eleven) {
    $expected = @($rows | Where-Object { $_.Path -ceq [string]$row.Path })
    if ($expected.Count -ne 1 -or [string]$expected[0].Hash -ine [string]$row.Hash -or
        [long]$expected[0].Size -ne [long]$row.Size) { throw 'Native 11-file manifest differs from production identity' }
  }
  return (Get-FileHash -LiteralPath $source11 -Algorithm SHA256).Hash
}
$source11Sha = Assert-SourceIdentity

. (Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1')
if (-not $HiddenDesktopWorkerName) {
  $params = @{
    ScriptPath = $PSCommandPath; ExePath = $exe; TimeoutSeconds = $TimeoutSeconds
    ExpectedExeSha256 = $ExpectedExeSha256
    ExpectedNativeSourceSha256 = $ExpectedNativeSourceSha256
    ExpectedSourceIdentityPath = $ExpectedSourceIdentityPath
  }
  if ($OutputDirectory) { $params.OutputDirectory = $OutputDirectory }
  Invoke-ConstellationHiddenDesktopWorker @params
  return
}
Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName

# Import existing, source-pinned UIA utilities from their AST. Never execute
# another test's top-level actions or inspect the visible input desktop.
$uiPath = Join-Path $PSScriptRoot 'windows-evidence-backup-e2e.ps1'
$uiText = [IO.File]::ReadAllText($uiPath)
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($uiText, [ref]$tokens, [ref]$errors)
if (@($errors).Count) { throw 'Existing UIA helper has parser errors' }
$functions = @($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst]}, $true))
foreach ($name in @('Ui-Records','Ui-Find','Ui-Require','Ui-Wait','Ui-WaitEnabled',
    'Ui-Invoke','Ui-SetText','Ui-InvokeModalCreateRoom')) {
  $matches = @($functions | Where-Object { $_.Name -ceq $name })
  if ($matches.Count -ne 1) { throw "Missing unique audited UI helper $name" }
  Invoke-Expression $matches[0].Extent.Text
}
$interopMatch = [regex]::Match($uiText, '(?s)\$win32\s*=\s*@''\r?\n(?<body>.*?)\r?\n''@')
if (-not $interopMatch.Success) { throw 'Cannot locate tested native window ownership helper' }
Add-Type -TypeDefinition $interopMatch.Groups['body'].Value -ReferencedAssemblies 'System' -ErrorAction Stop
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing -ErrorAction Stop

function Wait-UniqueEdit($root, [string]$name) {
  $until = [DateTime]::UtcNow.AddSeconds(8)
  do {
    $items = @((Ui-Records $root) | Where-Object { $_.Name -ceq $name -and $_.Type -ceq 'ControlType.Edit' })
    if ($items.Count -eq 1) { return $items[0] }
    if ($items.Count -gt 1) { throw "Ambiguous edit $name" }
    Start-Sleep -Milliseconds 120
  } while ([DateTime]::UtcNow -lt $until)
  throw "Missing editable control $name"
}
function Wait-UniqueEnabledActionPrefix($root, [string]$prefix, [int]$seconds = 8) {
  $until = [DateTime]::UtcNow.AddSeconds($seconds)
  do {
    $items = @((Ui-Records $root) | Where-Object {
      $_.Type -ceq 'ControlType.Button' -and $_.Name.StartsWith($prefix, [StringComparison]::Ordinal)
    })
    if ($items.Count -gt 1) { throw "Ambiguous UIA button prefix $prefix" }
    if ($items.Count -eq 1 -and $items[0].Element.Current.IsEnabled) { return $items[0] }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $until)
  throw "Missing enabled UIA action $prefix"
}
function Invoke-ExactConfirmCancel($root) {
  $button = Ui-WaitEnabled $root 'Confirm cancel task' 6
  if ($button.Type -cne 'ControlType.Button' -or $button.Name -cne 'Confirm cancel task') {
    throw 'Unexpected accessible control for native Core cancellation'
  }
  # WebView2 exposes aria-expanded on this button. The generic UIA helper
  # selects ExpandCollapsePattern, whose Expand() fails because the button is
  # ALREADY expanded. Use its explicit default click action instead, rather
  # than collapsing the confirmation or sending desktop-wide input.
  $pattern = $null
  if ($button.Element.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    ([Windows.Automation.InvokePattern]$pattern).Invoke(); return
  }
  if ($button.Element.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
    $expander = [Windows.Automation.ExpandCollapsePattern]$pattern
    if ($expander.Current.ExpandCollapseState -ne [Windows.Automation.ExpandCollapseState]::Expanded) {
      throw 'Confirmation button not in explicitly expanded state'
    }
    # Chromium's aria-expanded implementation maps this exact button to
    # ExpandCollapsePattern. Collapse is its permitted second activation,
    # but acceptance still requires durable Core state to become cancelled.
    $expander.Collapse(); return
  }
  $supported = @($button.Element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
  throw ('Confirm cancel button has no true click pattern; refusing fallback: ' + ($supported -join ','))
}

# Build a fresh loopback-only blocking HTTP fixture using the already-tested
# parser/server. Its one mutation delays *only the response*; it never grants
# native consent or runs a tool. A bounded timeout releases it on test failure.
$fixturePath = Join-Path $PSScriptRoot 'helper-local-workflow-fixture.ps1'
$fixtureText = [IO.File]::ReadAllText($fixturePath)
$fixtureMatch = [regex]::Match($fixtureText, '(?s)\$localFixtureSource\s*=\s*@''\r?\n(?<body>.*?)\r?\n''@')
if (-not $fixtureMatch.Success) { throw 'Local HTTP fixture definition unavailable' }
$fixtureCs = $fixtureMatch.Groups['body'].Value
$oldField = 'private int stopped, requestCount, rejectedCount;'
$newField = $oldField + @'

  private int transportFailures;
  public int TransportFailures { get { return Interlocked.CompareExchange(ref transportFailures, 0, 0); } }
  public readonly ManualResetEventSlim ReleaseResponse = new ManualResetEventSlim(false);
'@
$oldLine = 'if (valid) Interlocked.Increment(ref requestCount);'
$newLine = 'if (valid) { Interlocked.Increment(ref requestCount); ReleaseResponse.Wait(40000); }'
$oldCatch = @'
if (Interlocked.CompareExchange(ref stopped, 0, 0) != 0) break;
        Interlocked.Increment(ref rejectedCount);
'@
$newCatch = @'
if (Interlocked.CompareExchange(ref stopped, 0, 0) != 0) break;
        Interlocked.Increment(ref transportFailures);
'@
if (-not $fixtureCs.Contains($oldField) -or -not $fixtureCs.Contains($oldLine) -or
    -not $fixtureCs.Contains($oldCatch)) {
  throw 'Shared fixture shape changed; refuse weak/non-blocking test'
}
$fixtureCs = $fixtureCs.Replace($oldField, $newField).Replace($oldLine, $newLine).Replace($oldCatch, $newCatch)
Add-Type -TypeDefinition $fixtureCs -ReferencedAssemblies 'System' -ErrorAction Stop
$fixture = New-Object ConstellationLocalWorkflowFixture

$runId = [Guid]::NewGuid().ToString('N')
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ("constellation-model-qa-$runId")
$data = Join-Path $sandbox 'Data'
$artifacts = if ($OutputDirectory) { Join-Path ([IO.Path]::GetFullPath($OutputDirectory)) $runId } else { Join-Path $sandbox 'artifacts' }
foreach ($dir in @($sandbox,$data,(Join-Path $sandbox 'Temp'),(Join-Path $sandbox 'WebView2'),$artifacts)) {
  [void](New-Item -ItemType Directory -Path $dir -Force)
}
if (@(Get-ChildItem -LiteralPath $data -Force).Count -gt 0) { throw 'Test data directory must be fresh' }
$receipt = [ordered]@{
  claim = 'not_verified'
  scenario = 'real_native_hidden_desktop_cancel_during_blocked_loopback_provider_http'
  not_covered = 'cancellation_during_blocking_file_effect'
  run_id = $runId; exe_sha256 = $exeHash.ToLowerInvariant()
  native_runtime_sha256 = $knownRuntime.ToLowerInvariant()
  source_manifest_sha256 = $expectedManifest.ToLowerInvariant()
  native_eleven_manifest_sha256 = $source11Sha.ToLowerInvariant()
  hidden_desktop = $HiddenDesktopWorkerName
  test_pid = 0; fixture_requests = 0; fixture_rejected = 0; fixture_transport_failures = 0
  observed_http_blocked = $false; cancel_enabled_during_http = $false
  cancelled_while_server_blocked = $false; task_state_after_release = ''
  no_tool_journal_effect = $false; no_created_file = $false
  no_provider_retry = $false; process_closed = $false; errors = @()
}
$app = $null
try {
  if (-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe)).Contains('--isolated-data-dir')) {
    throw 'Executable lacks explicit isolated-data-dir capability'
  }
  [void](Assert-SourceIdentity)
  Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
  $psi = New-Object Diagnostics.ProcessStartInfo
  $psi.FileName = $exe
  $psi.Arguments = '--isolated-data-dir "' + $data + '"'
  $psi.WorkingDirectory = (Split-Path -Parent $exe)
  $psi.UseShellExecute = $false
  $psi.EnvironmentVariables['TEMP'] = Join-Path $sandbox 'Temp'
  $psi.EnvironmentVariables['TMP'] = Join-Path $sandbox 'Temp'
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'] = Join-Path $sandbox 'WebView2'
  $app = New-Object Diagnostics.Process
  $app.StartInfo = $psi
  if (-not $app.Start()) { throw 'Could not launch owned QA EXE' }
  $receipt.test_pid = $app.Id
  $hwnd = [IntPtr]::Zero; $root = $null
  $until = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    $app.Refresh()
    if ($app.HasExited) { throw 'Application exited before test UIA readiness' }
    $hwnd = [ConstellationEvidenceWindow]::FindOwnMainWindow($app.Id)
    if ($hwnd -ne [IntPtr]::Zero) {
      Assert-ConstellationHiddenWindow $hwnd $HiddenDesktopWorkerName
      $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
      if (@((Ui-Records $root) | Where-Object { $_.Name -ceq 'Settings' }).Count -eq 1) { break }
    }
    Start-Sleep -Milliseconds 200
  } while ([DateTime]::UtcNow -lt $until)
  if ($null -eq $root) { throw 'No owned hidden WebView2 UIA tree' }

  Ui-Invoke (Ui-Require $root 'Library' '')
  Ui-Invoke (Ui-WaitEnabled $root 'Create agent' 8)
  Ui-Invoke (Ui-WaitEnabled $root 'Add teammate' 8)
  Ui-Invoke (Ui-Require $root 'Rooms' '')
  $room = Ui-Wait $root 'Create your first room' '' 4
  if (-not $room) { $room = Ui-Wait $root 'Create room' '' 4 }
  if (-not $room) { throw 'Room create UI missing' }
  Ui-Invoke $room
  Ui-SetText $root (Wait-UniqueEdit $root 'ROOM NAME') 'Cancel in-flight isolated QA'
  Ui-InvokeModalCreateRoom $root
  Ui-Invoke (Ui-Require $root 'Settings' '')
  Ui-Invoke (Ui-WaitEnabled $root 'Add provider' 8)
  Ui-SetText $root (Ui-Wait $root 'DISPLAY NAME' 'provider-name' 8) 'Blocking loopback QA'
  Ui-SetText $root (Ui-Wait $root 'API BASE URL' 'provider-endpoint' 8) $fixture.Url
  Ui-SetText $root (Ui-Wait $root 'MODEL IDENTIFIER' 'provider-model' 8) ([ConstellationLocalWorkflowFixture]::Model)
  Ui-Invoke (Ui-WaitEnabled $root 'Save connection locally' 8)
  Ui-Invoke (Ui-Require $root 'Tasks' '')
  $plan = Ui-Wait $root 'Plan task' '' 4
  if (-not $plan) { $plan = Ui-Wait $root 'Plan your first task' '' 6 }
  if (-not $plan) { throw 'Task creation unavailable' }
  Ui-Invoke $plan
  $instruction = 'Cancellation must win before blocked loopback provider replies.'
  Ui-SetText $root (Wait-UniqueEdit $root 'INSTRUCTIONS') $instruction
  Ui-Invoke (Ui-WaitEnabled $root 'Save pending task' 8)
  $optionalDeadline = [DateTime]::UtcNow.AddSeconds(8)
  $optional = @()
  do {
    $optional = @((Ui-Records $root) | Where-Object {
      $_.Name.StartsWith('Optional file actions', [StringComparison]::Ordinal) -and $_.Type -ceq 'ControlType.Button'
    })
    if ($optional.Count -eq 1) { break }
    if ($optional.Count -gt 1) { throw 'Ambiguous optional file actions summary' }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $optionalDeadline)
  if ($optional.Count -ne 1) {
    $nearby = @((Ui-Records $root) | Where-Object {
      $_.Name -match 'file|Optional|Guided|task|approval|plan|agent|room|error|save' -or
      $_.Type -match 'TreeItem'
    } | Select-Object -First 55 | ForEach-Object { $_.Type + ' ' + $_.Name + ' [' + $_.Id + ']' })
    throw ('Cannot uniquely expand optional file actions; nearby UIA controls: ' + ($nearby -join ' | '))
  }
  Ui-Invoke $optional[0]
  Ui-Invoke (Ui-WaitEnabled $root 'Use Blocking loopback QA for task execution' 8)
  $multi = @((Ui-Records $root) | Where-Object {
    $_.Name.StartsWith('Guided multi-step work', [StringComparison]::Ordinal) -and $_.Type -ceq 'ControlType.Button'
  })
  if ($multi.Count -ne 1) { throw 'Cannot uniquely expand guided multi-step' }
  Ui-Invoke $multi[0]
  $taskDb = Join-Path $data 'constellation.sqlite3'
  $taskQuery = "const {DatabaseSync}=require('node:sqlite');const db=new DatabaseSync(process.argv[1],{readOnly:true});const rows=db.prepare('select id,data from tasks').all().filter(r=>JSON.parse(r.data).instructions===process.argv[2]);if(rows.length!==1||JSON.parse(rows[0].data).state!=='pending')process.exit(4);console.log(rows[0].id);"
  $taskId = [string](& node --no-warnings -e $taskQuery $taskDb $instruction | Out-String).Trim()
  if ($LASTEXITCODE -ne 0 -or $taskId -notmatch '^[0-9a-fA-F-]{36}$') {
    throw 'Could not verify unique Core task in isolated database'
  }
  Ui-Invoke (Wait-UniqueEnabledActionPrefix $root 'Request one next model step' 8)
  $until = [DateTime]::UtcNow.AddSeconds(16)
  do {
    if ($fixture.RejectedCount -gt 0 -or $fixture.RequestCount -gt 1) { throw 'Unexpected loopback request' }
    if ($fixture.RequestCount -eq 1) { break }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $until)
  if ($fixture.RequestCount -ne 1 -or $fixture.ReleaseResponse.IsSet) {
    throw 'Failed to establish exactly one server-blocked provider HTTP request'
  }
  $receipt.observed_http_blocked = $true
  $cancel = Ui-Wait $root 'Cancel task' '' 5
  if (-not $cancel -or -not $cancel.Element.Current.IsEnabled) {
    throw 'Cancel task not enabled during in-flight provider; cannot safely exercise native cancellation'
  }
  $receipt.cancel_enabled_during_http = $true
  Ui-Invoke $cancel
  Invoke-ExactConfirmCancel $root
  $stateQuery = "const {DatabaseSync}=require('node:sqlite');const db=new DatabaseSync(process.argv[1],{readOnly:true});const t=db.prepare('select data from tasks where id=?').get(process.argv[2]);if(!t)process.exit(5);console.log(JSON.parse(t.data).state);"
  $until = [DateTime]::UtcNow.AddSeconds(10)
  $state = ''
  do {
    $state = [string](& node --no-warnings -e $stateQuery $taskDb $taskId | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Independent Core task-state read failed' }
    if ($state -ceq 'cancelled') { break }
    if ($fixture.ReleaseResponse.IsSet) { throw 'Fixture response released before cancellation' }
    Start-Sleep -Milliseconds 150
  } while ([DateTime]::UtcNow -lt $until)
  if ($state -cne 'cancelled' -or $fixture.ReleaseResponse.IsSet) {
    throw "Native Core cancellation was not durable while provider response was held: $state"
  }
  $receipt.cancelled_while_server_blocked = $true
  $fixture.ReleaseResponse.Set()
  Start-Sleep -Milliseconds 1500
  $state = [string](& node --no-warnings -e $stateQuery $taskDb $taskId | Out-String).Trim()
  if ($LASTEXITCODE -ne 0 -or $state -cne 'cancelled') {
    throw "Late provider response changed cancelled Core task: $state"
  }
  $receipt.task_state_after_release = $state
  $journal = Join-Path (Join-Path $data 'local-tool-audit') 'native-multi-step-effects.jsonl'
  $events = @()
  if (Test-Path -LiteralPath $journal -PathType Leaf) {
    foreach ($line in @(Get-Content -LiteralPath $journal)) {
      if (-not $line.Trim()) { continue }
      $event = [string](ConvertFrom-Json -InputObject $line).record.event
      if ($event -in @('approve','approve_scoped','start','finish')) { $events += $event }
    }
  }
  if ($events.Count -ne 0) { throw 'Canceled provider unexpectedly reached native file execution journal' }
  $receipt.no_tool_journal_effect = $true
  $workspace = Join-Path $data 'local-file-artifacts'
  if (Test-Path -LiteralPath $workspace -PathType Container) {
    $files = @(Get-ChildItem -LiteralPath $workspace -Recurse -File)
    if ($files.Count -ne 0) { throw 'Canceled provider unexpectedly produced private file artifacts' }
  }
  $receipt.no_created_file = $true
  if ($fixture.RequestCount -ne 1 -or $fixture.RejectedCount -ne 0 -or $fixture.TransportFailures -gt 1) {
    throw 'Cancellation or late response triggered another provider request'
  }
  $receipt.no_provider_retry = $true
  [void](Assert-SourceIdentity)
  if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $exeHash) { throw 'EXE changed during QA' }
  $receipt.claim = 'pass_real_hidden_desktop_provider_http_cancel_before_response'
} catch {
  $receipt.errors += $_.Exception.GetBaseException().Message
} finally {
  $fixture.ReleaseResponse.Set()
  $receipt.fixture_requests = $fixture.RequestCount
  $receipt.fixture_rejected = $fixture.RejectedCount
  $receipt.fixture_transport_failures = $fixture.TransportFailures
  $fixture.Dispose()
  if ($app) {
    try {
      $app.Refresh()
      if (-not $app.HasExited) {
        [void]$app.CloseMainWindow()
        if (-not $app.WaitForExit(2500)) { $app.Kill(); [void]$app.WaitForExit(3000) }
      }
      $receipt.process_closed = $app.HasExited
    } catch { $receipt.errors += 'Failed to terminate the isolated QA PID' }
    finally { $app.Dispose() }
  }
}
if (-not $receipt.process_closed -or $receipt.errors.Count -gt 0) { $receipt.claim = 'not_verified' }
$file = Join-Path $artifacts ('native-cancel-inflight-' + $runId + '.json')
($receipt | ConvertTo-Json -Depth 8) | Set-Content -LiteralPath $file -Encoding UTF8
$receipt | ConvertTo-Json -Depth 8
if ($receipt.claim -cne 'pass_real_hidden_desktop_provider_http_cancel_before_response') { exit 1 }
