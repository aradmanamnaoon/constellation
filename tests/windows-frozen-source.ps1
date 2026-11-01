# Copy the current Constellation Rust/native/frontend build inputs to a new,
# immutable-for-this-run source directory, then compare their byte identities.
# No GUI, package installation, network calls, Git mutation or user data.
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$SnapshotRoot,
  [switch]$Verify,
  [switch]$VerifyFrozenOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).ProviderPath
$destination = [IO.Path]::GetFullPath($SnapshotRoot)
$manifestName = 'source-frozen.sha256'
$manifest = Join-Path $destination $manifestName
if ($Verify -and $VerifyFrozenOnly) { throw 'Choose either live source or frozen-only verification' }
if ($VerifyFrozenOnly) {
  if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) { throw 'Pinned source manifest is missing' }
  $expected = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
  foreach ($line in [IO.File]::ReadAllLines($manifest)) {
    if ($line -notmatch '^([a-f0-9]{64})  ([a-zA-Z0-9._+/@-]+)$') {
      throw 'Malformed frozen source manifest entry'
    }
    $digest = $Matches[1]
    $relative = $Matches[2]
    if ($relative.Contains('../') -or $relative.StartsWith('/') -or -not $expected.Add($relative)) {
      throw 'Unsafe or duplicate frozen source path'
    }
    $file = Join-Path $destination $relative
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing frozen source file: $relative" }
    $item = Get-Item -LiteralPath $file
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Frozen source contains redirected file' }
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $digest) {
      throw "Frozen source digest mismatch: $relative"
    }
  }
  # Tauri writes exactly these generated schemas into src-tauri/gen while
  # compiling. They are output files, not an input revision of the source.
  $generated = @(
    'apps/desktop/src-tauri/gen/schemas/acl-manifests.json',
    'apps/desktop/src-tauri/gen/schemas/capabilities.json',
    'apps/desktop/src-tauri/gen/schemas/desktop-schema.json',
    'apps/desktop/src-tauri/gen/schemas/windows-schema.json'
  )
  $actual = @(
    Get-ChildItem -LiteralPath $destination -Recurse -File -Force |
      Where-Object {
        $relative = $_.FullName.Substring($destination.Length).TrimStart('\', '/').Replace('\', '/')
        $_.FullName -ne $manifest -and $relative -notin $generated
      }
  )
  if ($expected.Count -lt 150 -or $actual.Length -ne $expected.Count) {
    throw 'Frozen source contains missing or extra files'
  }
  Write-Output "SOURCE_SNAPSHOT_INTERNAL_VERIFIED $($expected.Count)"
  Write-Output "MANIFEST_SHA256=$((Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash)"
  return
}
$all = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

function Add-File([string]$Relative) {
  $relativeSlash = $Relative.Replace('\', '/')
  $path = Join-Path $source $relativeSlash
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return }
  $item = Get-Item -LiteralPath $path
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw "Unsafe source reparse point: $relativeSlash"
  }
  [void]$all.Add($relativeSlash)
}

function Add-Tree([string]$Relative) {
  $path = Join-Path $source $Relative
  if (-not (Test-Path -LiteralPath $path -PathType Container)) { return }
  $root = Get-Item -LiteralPath $path
  if ($root.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw "Unsafe source directory reparse point: $Relative"
  }
  $files = Get-ChildItem -LiteralPath $path -File -Recurse -Force
  foreach ($file in $files) {
    if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) {
      throw "Unsafe source reparse point: $($file.FullName)"
    }
    $name = $file.FullName.Substring($source.Length).TrimStart('\', '/').Replace('\', '/')
    Add-File $name
  }
}

foreach ($file in @('LICENSE','catalog/roles.json','apps/desktop/package.json',
    'apps/desktop/pnpm-lock.yaml','apps/desktop/tsconfig.json',
    'apps/desktop/vite.config.js','apps/desktop/svelte.config.js',
    'apps/desktop/src-tauri/Cargo.toml','apps/desktop/src-tauri/Cargo.lock',
    'apps/desktop/src-tauri/build.rs','apps/desktop/src-tauri/tauri.conf.json')) {
  Add-File $file
}
foreach ($directory in @('apps/desktop/src','apps/desktop/build',
    'apps/desktop/static','apps/desktop/src-tauri/src',
    'apps/desktop/src-tauri/icons','apps/desktop/src-tauri/capabilities',
    'catalog')) {
  Add-Tree $directory
}
$cratesRoot = Join-Path $source 'crates'
foreach ($crate in (Get-ChildItem -LiteralPath $cratesRoot -Directory)) {
  if ($crate.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw "Unsafe crate directory reparse point: $($crate.Name)"
  }
  foreach ($file in @('Cargo.toml','Cargo.lock','build.rs')) {
    Add-File "crates/$($crate.Name)/$file"
  }
  foreach ($directory in @('src','tests','fixtures','assets','templates')) {
    Add-Tree "crates/$($crate.Name)/$directory"
  }
}

$paths = [string[]]@($all | Sort-Object -CaseSensitive)
if ($paths.Length -lt 150) { throw "Source scan unexpectedly small: $($paths.Length)" }
if (-not $Verify) {
  if (Test-Path -LiteralPath $destination) {
    throw 'Refusing to replace or update an existing source snapshot'
  }
  [void](New-Item -ItemType Directory -Path $destination -ErrorAction Stop)
}
if (-not (Test-Path -LiteralPath $destination -PathType Container)) {
  throw 'Snapshot directory does not exist'
}

$lines = [System.Collections.Generic.List[string]]::new()
foreach ($relative in $paths) {
  $from = Join-Path $source $relative
  $to = Join-Path $destination $relative
  $before = (Get-FileHash -LiteralPath $from -Algorithm SHA256).Hash.ToLowerInvariant()
  if (-not $Verify) {
    $parent = Split-Path -Parent $to
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
      [void](New-Item -ItemType Directory -Path $parent -Force)
    }
    Copy-Item -LiteralPath $from -Destination $to -ErrorAction Stop
  }
  if (-not (Test-Path -LiteralPath $to -PathType Leaf)) {
    throw "Snapshot source absent: $relative"
  }
  $snapshotHash = (Get-FileHash -LiteralPath $to -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($snapshotHash -ne $before) {
    throw "Source drift or snapshot mismatch: $relative"
  }
  [void]$lines.Add("$before  $relative")
}
$output = ($lines -join "`n") + "`n"
if ($Verify) {
  $pinned = [IO.File]::ReadAllText($manifest)
  if ($pinned -cne $output) { throw 'SOURCE_DRIFT: file names or SHA-256 differ from frozen source' }
  Write-Output "SOURCE_SNAPSHOT_VERIFIED $($paths.Length)"
} else {
  [IO.File]::WriteAllText($manifest, $output, [System.Text.UTF8Encoding]::new($false))
  Write-Output "SOURCE_SNAPSHOT_CREATED $($paths.Length)"
}
Write-Output "SNAPSHOT=$destination"
Write-Output "MANIFEST_SHA256=$((Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash)"
