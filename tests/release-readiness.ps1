# Read-only release preflight for the Constellation master acceptance contract.
# Emits one JSON object; a failed gate exits 1. This script never builds,
# installs, signs, stages, commits, publishes, or accesses user credentials.
[CmdletBinding()]
param(
  [string]$RepositoryRoot = '',
  [ValidateRange(1, 1000)][int]$RequiredAcceptanceRows = 188,
  [ValidateRange(1, 1000)][int]$RequiredSkills = 200,
  [ValidateRange(1, 1000)][int]$RequiredRoles = 200
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $RepositoryRoot) { $RepositoryRoot = Join-Path $PSScriptRoot '..' }
$root = (Resolve-Path -LiteralPath $RepositoryRoot -ErrorAction Stop).ProviderPath
$results = [System.Collections.Generic.List[object]]::new()

function Add-Gate([string]$name, [bool]$passed, [string]$details) {
  $results.Add([pscustomobject]@{
    name = $name
    passed = $passed
    details = $details
  })
}

function Invoke-RepositoryCommand([string]$executable, [string[]]$arguments) {
  Push-Location -LiteralPath $root
  # Windows PowerShell treats ordinary native stderr on an expected nonzero
  # exit (git ls-files --error-unmatch, incomplete release validator) as a
  # PowerShell ErrorRecord when ErrorActionPreference is Stop. A negative
  # gate is evidence, not a reason to abort all following preflight checks.
  $ErrorActionPreference = 'Continue'
  try {
    $output = & $executable @arguments 2>&1 | Out-String
    $code = $LASTEXITCODE
    return [pscustomobject]@{ exit_code = $code; output = $output }
  } finally {
    Pop-Location
  }
}

try {
  $git = Invoke-RepositoryCommand 'git' @('rev-parse', '--show-toplevel')
  if ($git.exit_code -ne 0) {
    throw 'The selected root is not a valid Git checkout.'
  }
  $gitStatus = Invoke-RepositoryCommand 'git' @('status', '--porcelain=v1', '--untracked-files=all')
  Add-Gate 'clean_git_worktree' ($gitStatus.exit_code -eq 0 -and -not $gitStatus.output.Trim()) (
    'A release must be generated from a committed, clean Git revision; any source, docs, screenshots or lockfiles still untracked/modified block this gate.'
  )
  $head = Invoke-RepositoryCommand 'git' @('rev-parse', 'HEAD')
  Add-Gate 'full_source_commit' ($head.exit_code -eq 0 -and $head.output.Trim() -match '^[0-9a-f]{40}$') (
    'Current Git HEAD: ' + $head.output.Trim()
  )

  $remotes = Invoke-RepositoryCommand 'git' @('remote')
  Add-Gate 'configured_git_remote' ($remotes.exit_code -eq 0 -and -not [string]::IsNullOrWhiteSpace($remotes.output)) (
    'A verified remote is needed for public source, hosted CI and release URLs; this check does not authenticate publication.'
  )

  $lockPaths = @(Get-ChildItem -LiteralPath (Join-Path $root 'crates') -Directory | ForEach-Object {
    Join-Path $_.FullName 'Cargo.toml'
  })
  $lockPaths += Join-Path $root 'apps/desktop/src-tauri/Cargo.toml'
  $untracked = [System.Collections.Generic.List[string]]::new()
  foreach ($manifest in $lockPaths) {
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
      $untracked.Add("missing manifest: $manifest")
      continue
    }
    $lock = Join-Path (Split-Path -Parent $manifest) 'Cargo.lock'
    if (-not (Test-Path -LiteralPath $lock -PathType Leaf)) {
      $untracked.Add("missing lockfile: $lock")
      continue
    }
    # Windows PowerShell 5.1 runs on .NET Framework, which does not expose
    # Path.GetRelativePath. The resolved manifest is already inside $root.
    $prefix = $root.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $lock.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
      $untracked.Add("lockfile escaped repository root: $lock")
      continue
    }
    $relative = $lock.Substring($prefix.Length).Replace('\', '/')
    $tracked = Invoke-RepositoryCommand 'git' @('ls-files', '--error-unmatch', '--', $relative)
    if ($tracked.exit_code -ne 0) { $untracked.Add($relative) }
  }
  Add-Gate 'versioned_rust_lockfiles' ($untracked.Count -eq 0) (
    "Examined $($lockPaths.Count) standalone manifests; missing/untracked lockfiles: " + $(if ($untracked.Count) { $untracked -join ', ' } else { 'none' })
  )

  # A missing crate directory is invisible to a dynamic crates/* manifest
  # scan. Require every implementation tree promised by the CI configuration,
  # and require all present Rust/frontend source files to be versioned.
  $expectedCrates = @(
    'constellation-agent-loop', 'constellation-backup', 'constellation-backup-cli',
    'constellation-core', 'constellation-credential-vault', 'constellation-evidence',
    'constellation-project-backup', 'constellation-provider-adapters',
    'constellation-provider-probe', 'constellation-runner-protocol',
    'constellation-runner-service',
    'constellation-sync', 'constellation-tools', 'constellation-workflows'
  )
  $requiredSources = [System.Collections.Generic.List[string]]::new()
  foreach ($crate in $expectedCrates) {
    foreach ($path in @('Cargo.toml', 'Cargo.lock', 'src/lib.rs')) {
      $requiredSources.Add("crates/$crate/$path")
    }
  }
  foreach ($path in @(
    'crates/constellation-backup-cli/src/main.rs',
    'apps/desktop/package.json', 'apps/desktop/pnpm-lock.yaml',
    'apps/desktop/src-tauri/Cargo.toml', 'apps/desktop/src-tauri/Cargo.lock',
    'apps/desktop/src-tauri/src/lib.rs', 'apps/desktop/src-tauri/tauri.conf.json',
    'apps/desktop/src/routes/+page.svelte', 'apps/desktop/src/routes/studio.css',
    'catalog/build-roles.mjs', 'catalog/roles.test.mjs',
    'skills-manifest/inventory.json', 'skills-manifest/validate.mjs',
    'skills-manifest/validate.test.mjs', 'skills-manifest/reviewed-exercise.test.mjs',
    'tests/release-readiness.ps1'
  )) { $requiredSources.Add($path) }
  foreach ($sourceDir in @(
    'crates', 'apps/desktop/src-tauri/src', 'apps/desktop/src',
    'catalog', 'skills-manifest', 'tests'
  )) {
    $absoluteDir = Join-Path $root $sourceDir
    if (-not (Test-Path -LiteralPath $absoluteDir -PathType Container)) { continue }
    Get-ChildItem -LiteralPath $absoluteDir -File -Recurse | Where-Object {
      $_.Extension -in @('.rs', '.svelte', '.css', '.ts', '.js', '.mjs', '.ps1') -and
      $_.FullName -notmatch '[\\/](target|node_modules)[\\/]'
    } | ForEach-Object {
      $prefix = $root.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
      $requiredSources.Add($_.FullName.Substring($prefix.Length).Replace('\', '/'))
    }
  }
  $missingSources = [System.Collections.Generic.List[string]]::new()
  foreach ($source in @($requiredSources | Select-Object -Unique)) {
    if (-not (Test-Path -LiteralPath (Join-Path $root $source) -PathType Leaf)) {
      $missingSources.Add($source)
      continue
    }
    $tracked = Invoke-RepositoryCommand 'git' @('ls-files', '--error-unmatch', '--', $source)
    if ($tracked.exit_code -ne 0) { $missingSources.Add($source) }
  }
  Add-Gate 'committed_source_completeness' ($missingSources.Count -eq 0) (
    "Checked $(@($requiredSources | Select-Object -Unique).Count) required/present sources; missing or untracked $($missingSources.Count): " +
    $(if ($missingSources.Count) { (@($missingSources | Select-Object -First 30) -join ', ') + $(if ($missingSources.Count -gt 30) { ', ...' } else { '' }) } else { 'none' })
  )

  # Check ALL reachable Git history objects, not merely HEAD or the working
  # directory. This is conservative: sum unique uncompressed blob sizes.
  # No filenames or contents are emitted, to avoid logging private data.
  Push-Location -LiteralPath $root
  try {
    $objectLines = @(& git rev-list --objects --all)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot list reachable Git history objects.' }
    $hashes = @($objectLines | ForEach-Object { ($_ -split ' ', 2)[0] })
    if ($hashes.Count -eq 0) { throw 'Git repository has no reachable history objects.' }
    $objectInfo = @($hashes | & git cat-file '--batch-check=%(objectname) %(objecttype) %(objectsize)')
    if ($LASTEXITCODE -ne 0 -or $objectInfo.Count -ne $hashes.Count) {
      throw 'Git object metadata is unavailable or incomplete.'
    }
    [long]$blobBytes = 0
    [long]$maxBlob = 0
    foreach ($info in $objectInfo) {
      $parts = $info -split '\s+'
      if ($parts.Count -ne 3 -or $parts[1] -notin @('blob','tree','commit','tag')) {
        throw 'Unexpected object type/size from Git.'
      }
      if ($parts[1] -eq 'blob') {
        [long]$size = $parts[2]
        $blobBytes += $size
        if ($size -gt $maxBlob) { $maxBlob = $size }
      }
    }
    $withinBudget = $blobBytes -le (500L * 1024 * 1024) -and $maxBlob -le (50L * 1024 * 1024)
    Add-Gate 'git_history_size_budget' $withinBudget (
      "All $($hashes.Count) reachable Git objects inspected; cumulative uncompressed blobs $blobBytes bytes (limit 524288000), largest blob $maxBlob bytes (limit 52428800). New untracked files are excluded until committed and checked again."
    )
  } finally { Pop-Location }

  $roles = Invoke-RepositoryCommand 'node' @('catalog/build-roles.mjs', '--check')
  $rolesPass = $roles.exit_code -eq 0 -and
    $roles.output -match ("Verified\s+$RequiredRoles\s+prompt-only roles")
  Add-Gate 'required_role_definitions' $rolesPass (
    "Expected exactly $RequiredRoles deterministic role templates; result exit $($roles.exit_code). Role templates alone are not proven real model capabilities."
  )

  # Strict local verifier independently checks installed sources, publisher/
  # license evidence and actual offline exercise receipts. Never infer target
  # from observed filenames, a manifest's self-described totals or mocks.
  $skills = Invoke-RepositoryCommand 'node' @('skills-manifest/validate.mjs', '--verify-local', '--require-target')
  Add-Gate 'required_installed_exercised_integrations' ($skills.exit_code -eq 0) (
    "Expected $RequiredSkills separately vetted/exercised integrations; strict validator exit $($skills.exit_code). Re-run after any relevant source changes."
  )

  $ledger = Join-Path $root 'docs/quality/acceptance-ledger.md'
  $body = [IO.File]::ReadAllLines($ledger)
  # Detect the stable requirement-ID syntax, including REMOTE and ANDROID.
  # Restrict to ALL-CAPS IDs so table headings and footnotes never count.
  $rows = @($body | Where-Object { $_ -match '^\|\s*[A-Z][A-Z0-9]*-\d+\s*\|' })
  $unfinished = @($rows | Where-Object { $_ -notmatch '\|\s*\*\*PASS\*\*\s*\|' })
  Add-Gate 'master_acceptance_ledger' ($rows.Count -eq $RequiredAcceptanceRows -and $unfinished.Count -eq 0) (
    "Required $RequiredAcceptanceRows platform-qualified rows; observed $($rows.Count), pending non-PASS $($unfinished.Count). Only supported platform/build evidence can close a row."
  )

  # The original specification is larger than either the 188-row platform
  # ledger or the 40 differentiators. Require the deterministic full execution
  # inventory, independently check every required category, and leave this
  # gate closed while any traceability task lacks its own verified evidence.
  $planFile = Join-Path $root 'docs/mission/COMPLETE_EXECUTION_PLAN.md'
  $planCheck = Invoke-RepositoryCommand 'node' @('docs/mission/generate-complete-execution-plan.mjs', '--check')
  $planRows = @()
  if (Test-Path -LiteralPath $planFile -PathType Leaf) {
    $planRows = @([IO.File]::ReadAllLines($planFile) | Where-Object {
      $_ -match '^- \[[ x]\] \*\*[A-Z][A-Z0-9-]+\*\*'
    })
  }
  $expectedCategories = @{
    'SPEC-' = 520
    'TRACK-' = 23
    'ACC-' = 188
    'FEATURE-' = 40
    'CI-' = 17
    'ROLE-' = 200
    'INTEGRATION-' = 200
  }
  $completeCategories = $true
  foreach ($category in $expectedCategories.Keys) {
    $matched = @($planRows | Where-Object { $_.StartsWith('- [') -and $_ -match ('\*\*' + $category) })
    if ($matched.Count -ne $expectedCategories[$category]) { $completeCategories = $false }
  }
  $discovered = @($planRows | Where-Object { $_ -match '\*\*DISCOVERY-' }).Count
  $pendingPlanRows = @($planRows | Where-Object { $_.StartsWith('- [ ] ') }).Count
  $executionPlanPass = $planCheck.exit_code -eq 0 -and $completeCategories -and
    $discovered -ge 20 -and $planRows.Count -ge 1200 -and $pendingPlanRows -eq 0
  Add-Gate 'complete_execution_inventory' $executionPlanPass (
    "Deterministic full-spec plan exit $($planCheck.exit_code); $($planRows.Count) traceability tasks, $pendingPlanRows not independently VERIFIED DONE; $discovered source-gap tasks. Requires all 520 original subtasks, 23 workstreams, 188 acceptance rows, 40 features, 17 CI families, 200 role evaluations, 200 integrations and at least 20 source-gap checks to be included and verified."
  )

  # A Git remote by itself is not a hosted green CI run, a signed bundle,
  # Android APK, paired runner, or a GitHub Release. These checks require
  # independently linked real-world evidence before publication.
  Add-Gate 'hosted_ci_signatures_and_public_release' $false (
    'No checked hosted CI run IDs, platform install receipts, signature chain or published release/download verification were provided to this preflight. Perform explicit external release verification before clearing.'
  )
} catch {
  Add-Gate 'preflight_execution' $false $_.Exception.Message
}

$failed = @($results | Where-Object { -not $_.passed })
$summary = [ordered]@{
  schema = 'constellation.release_preflight.v1'
  repository = $root
  read_only = $true
  release_ready = ($failed.Count -eq 0)
  gates = @($results)
  failed_count = $failed.Count
}
$summary | ConvertTo-Json -Depth 7
if ($failed.Count -gt 0) { exit 1 }
