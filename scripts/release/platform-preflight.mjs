#!/usr/bin/env node
/**
 * Read-only, deliberately fail-closed inventory/preflight. This is NOT a
 * release attestor and cannot authorize `gh release create`.
 *
 * Usage: node scripts/release/platform-preflight.mjs [--json]
 * Exit: 2 blocked, 64 invalid CLI invocation. There is presently no
 * successful publication exit: hosted install, signing, ADB, and provenance
 * require independent attestations rather than files in this workspace.
 */
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REFERENCE = Object.freeze({
  windowsExeSha: '1e5c79ba1c255496b6eb351c512c3f12d429aa8b0ea57e90fdd0ee36c93eea61',
  windowsNsisSha: 'ff2fcf056a65aae4c937464b701cf3417e708c5d8f6e409a61deb5b87ea8b652',
  sourceManifestSha: '577537deb0dce77c7a2fff7e87af6ac2ece851fb57ec0d927bae7a2135bef94d',
  expectedSourceFiles: 187,
});

function checksum(bytes) { return createHash('sha256').update(bytes).digest('hex'); }

function safeFile(path) {
  try { return lstatSync(path).isFile(); } catch { return false; }
}

function readText(path) {
  try { return readFileSync(path, 'utf8'); } catch { return null; }
}

function runReadOnly(executable, args, cwd, env) {
  const result = spawnSync(executable, args, {
    cwd, env: { ...env, GIT_OPTIONAL_LOCKS: '0' }, encoding: 'utf8',
    timeout: 12000, maxBuffer: 1024 * 1024, windowsHide: true,
  });
  return result.error ? { status: null, stdout: '', stderr: String(result.error) } : result;
}

export function validateSourceManifest(entries, root, expectedCount = REFERENCE.expectedSourceFiles) {
  const faults = [];
  if (!Array.isArray(entries) || entries.length !== expectedCount) {
    return [`Expected ${expectedCount} manifest entries; got ${Array.isArray(entries) ? entries.length : 'non-array'}`];
  }
  const names = new Set();
  const rootReal = realpathSync(root);
  for (const entry of entries) {
    const name = entry?.Path;
    if (typeof name !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9./_+@ -]*$/.test(name)
        || name.split('/').includes('..') || name.split('/').includes('.')
        || name.includes('//') || name.includes('\\') || names.has(name)) {
      faults.push('Duplicate or unsafe manifest path');
      continue;
    }
    names.add(name);
    if (!Number.isSafeInteger(entry.Size) || entry.Size < 0 || !/^[0-9a-fA-F]{64}$/.test(entry.Hash ?? '')) {
      faults.push(`Malformed manifest size/digest: ${name}`);
      continue;
    }
    const full = resolve(rootReal, ...name.split('/'));
    if (!full.startsWith(rootReal + sep) || !safeFile(full)) {
      faults.push(`Missing/non-regular source: ${name}`);
      continue;
    }
    // Never trust a symlink escape, even when a parent directory is symlinked.
    const real = realpathSync(full);
    if (!real.startsWith(rootReal + sep)) {
      faults.push(`Source escapes repository root: ${name}`);
      continue;
    }
    const bytes = readFileSync(full);
    if (bytes.length !== entry.Size || checksum(bytes) !== entry.Hash.toLowerCase()) {
      faults.push(`Source checksum/size drift: ${name}`);
    }
  }
  return faults;
}

function discoverArtifacts(root, env, platform) {
  const paths = [
    ...['dist/windows', 'dist/linux', 'dist/android',
      'apps/desktop/src-tauri/target/release/bundle/nsis',
      'apps/desktop/src-tauri/target/release/bundle/dmg',
      'apps/desktop/src-tauri/target/release/bundle/deb',
      'apps/desktop/src-tauri/target/release/bundle/appimage',
      'apps/desktop/src-tauri/gen/android/app/build/outputs/apk/release'].map(p => join(root, p)),
  ];
  if (platform === 'win32' && env.LOCALAPPDATA) {
    const base = join(env.LOCALAPPDATA, 'Temp', 'constellation-20261031-release-final', 'release');
    paths.push(base, join(base, 'bundle', 'nsis'));
  }
  const artifacts = [];
  const seen = new Set();
  for (const folder of paths) {
    if (!existsSync(folder)) continue;
    for (const name of readdirSync(folder).sort()) {
      if (!/\.(?:exe|deb|AppImage|dmg|pkg|apk|aab)$/i.test(name)) continue;
      const full = join(folder, name);
      if (!safeFile(full) || seen.has(full)) continue;
      seen.add(full);
      const bytes = readFileSync(full);
      artifacts.push({ path: full, bytes: bytes.length, sha256: checksum(bytes) });
    }
  }
  return artifacts;
}

function checkWindowsSignature(full, env) {
  const ps = runReadOnly('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
    '(Get-AuthenticodeSignature -LiteralPath $env:CONSTELLATION_PREFLIGHT_ARTIFACT).Status.ToString()'],
    dirname(full), { ...env, CONSTELLATION_PREFLIGHT_ARTIFACT: full });
  return ps.status === 0 ? ps.stdout.trim() : 'unavailable';
}

function findExecutable(name, env, platform) {
  const ext = platform === 'win32' ? ['', '.exe', '.cmd', '.bat'] : [''];
  for (const dir of (env.PATH ?? '').split(platform === 'win32' ? ';' : ':')) {
    if (!dir) continue;
    for (const suffix of ext) {
      if (safeFile(join(dir, name + suffix))) return true;
    }
  }
  return false;
}

export function workflowJob(source, name) {
  // Inspect only an exact top-level job, not similarly named comments or a
  // different job's `exit 1`. Unknown indentation/layout => no job evidence.
  const lines = source.split(/\r?\n/);
  const start = lines.findIndex(line => line === `  ${name}:`);
  if (start < 0) return null;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    if (/^  [a-z][\w-]*:\s*$/.test(lines[i])) { end = i; break; }
  }
  return lines.slice(start + 1, end).join('\n');
}

export function inspectPreflight(root, { env = process.env, platform = process.platform } = {}) {
  const issues = [];
  const issue = (id, area, detail) => issues.push({ id, area, detail });
  const repo = resolve(root);
  const configFile = join(repo, 'apps/desktop/src-tauri/tauri.conf.json');
  let config = null;
  try { config = JSON.parse(readFileSync(configFile, 'utf8')); } catch { issue('TAURI_CONFIG_MISSING_OR_INVALID', 'all', configFile); }
  if (config && (config.identifier !== 'app.constellation.studio' || config.bundle?.active !== true)) {
    issue('TAURI_IDENTITY_UNEXPECTED', 'all', 'Unexpected bundle identifier or disabled bundling.');
  }

  const workflowPath = join(repo, '.github/workflows/acceptance.yml');
  const workflow = readText(workflowPath);
  if (workflow === null) issue('ACCEPTANCE_WORKFLOW_MISSING', 'ci', workflowPath);
  else {
    // Check jobs independently. A future workflow rewrite should make the
    // predicate unrecognizable and thus require manual independent review.
    const android = workflowJob(workflow, 'android-build') ?? '';
    const release = workflowJob(workflow, 'release-assets') ?? '';
    if (/\bexit\s+1\b/.test(android)) issue('ANDROID_CI_INTENTIONAL_FAILURE', 'android', 'android-build job exits 1.');
    else issue('ANDROID_CI_NOT_ATTESTED', 'android', 'No successful signed APK/device installation receipts supplied.');
    if (/\bexit\s+1\b/.test(release)) issue('RELEASE_CI_INTENTIONAL_FAILURE', 'ci', 'release-assets job exits 1.');
    else issue('RELEASE_CI_NOT_ATTESTED', 'ci', 'No independently verified final release-assets success.');
    if (!/macos-15-intel/.test(workflow) || !/macos-15/.test(workflow)) {
      issue('MACOS_RUNNER_CONFIG_INCOMPLETE', 'macos', 'Missing expected two-architecture macOS CI matrix.');
    }
  }
  const tracked = runReadOnly('git', ['ls-files', '--error-unmatch', '--', '.github/workflows/acceptance.yml'], repo, env);
  if (tracked.status !== 0) issue('ACCEPTANCE_WORKFLOW_UNTRACKED', 'ci', 'Workflow not in Git index.');
  const remote = runReadOnly('git', ['remote'], repo, env);
  if (remote.status !== 0 || !remote.stdout.trim()) issue('REMOTE_NOT_VERIFIED', 'ci', 'No configured, verified Git remote.');
  else issue('REMOTE_RELEASE_TARGET_UNVERIFIED', 'ci', 'A configured Git remote alone does not attest its hosted release permissions, workflow runs, visibility or published asset integrity.');
  const status = runReadOnly('git', ['--no-optional-locks', 'status', '--porcelain=v1', '--untracked-files=normal'], repo, env);
  if (status.status !== 0 || status.stdout.trim()) {
    issue('SOURCE_NOT_FROZEN', 'all', 'Git source is dirty/untracked or repository status unreadable.');
  }

  let manifest = { status: 'not_checked' };
  if (platform === 'win32' && env.LOCALAPPDATA) {
    const manifestPath = join(env.LOCALAPPDATA, 'Temp', 'constellation-source-20261031-prime', 'current-source-pinned-187-20261031.json');
    if (!safeFile(manifestPath)) issue('WINDOWS_SOURCE_MANIFEST_MISSING', 'windows', manifestPath);
    else {
      const bytes = readFileSync(manifestPath);
      const hash = checksum(bytes);
      if (hash !== REFERENCE.sourceManifestSha) issue('WINDOWS_SOURCE_MANIFEST_DRIFT', 'windows', `Unexpected manifest SHA256 ${hash}`);
      try {
        const faults = validateSourceManifest(JSON.parse(bytes.toString('utf8')), repo);
        manifest = { path: manifestPath, sha256: hash, entries: REFERENCE.expectedSourceFiles, faults: faults.slice(0, 12) };
        if (faults.length) issue('WINDOWS_SOURCE_DRIFT', 'windows', `${faults.length} recorded source paths differ; ${faults.slice(0, 2).join('; ')}`);
      } catch (error) { issue('WINDOWS_SOURCE_MANIFEST_UNREADABLE', 'windows', String(error.message)); }
    }
  } else issue('WINDOWS_SOURCE_MANIFEST_NOT_VERIFIED', 'windows', 'No local Windows build manifest in this environment.');

  const artifacts = discoverArtifacts(repo, env, platform);
  const winExe = artifacts.find(a => a.sha256 === REFERENCE.windowsExeSha);
  const winSetup = artifacts.find(a => a.sha256 === REFERENCE.windowsNsisSha);
  if (!winExe) issue('CURRENT_WINDOWS_EXE_MISSING', 'windows', 'No exact current-source-pinned Windows executable found.');
  if (!winSetup) issue('CURRENT_WINDOWS_NSIS_MISSING', 'windows', 'No exact current-source-pinned Windows setup found.');
  if (winExe && winSetup && platform === 'win32') {
    const signExe = checkWindowsSignature(winExe.path, env);
    const signInstaller = checkWindowsSignature(winSetup.path, env);
    if (signExe !== 'Valid' || signInstaller !== 'Valid') {
      issue('WINDOWS_CODE_SIGNING_FAILED', 'windows', `Authenticode EXE=${signExe}, NSIS=${signInstaller}.`);
    }
    const existingInstall = runReadOnly('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      "if (Test-Path -LiteralPath 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Constellation') { 'PRESENT' } else { 'ABSENT' }"],
      repo, env);
    if (existingInstall.status !== 0 || existingInstall.stdout.trim() === 'PRESENT') {
      issue('WINDOWS_EXISTING_UNINSTALL_REGISTRATION', 'windows',
        'Current HKCU Constellation uninstall registration already exists or cannot be independently inspected; do NOT installer-test this account.');
    }
  } else issue('WINDOWS_CODE_SIGNING_UNVERIFIED', 'windows', 'No verified signed EXE+NSIS pair in this environment.');
  // Never install on the development account. A full clean isolated-user
  // NSIS install/upgrade/uninstall receipt is a separate hard prerequisite.
  issue('WINDOWS_INSTALL_LIFECYCLE_UNVERIFIED', 'windows', 'No independent isolated-user NSIS install/upgrade/uninstall and native GUI acceptance attestation.');

  const debs = artifacts.filter(a => /\.deb$/i.test(a.path));
  const appimages = artifacts.filter(a => /\.AppImage$/i.test(a.path));
  if (!debs.length) issue('LINUX_DEB_MISSING', 'linux', 'No Debian package found in inspected output paths.');
  if (!appimages.length) issue('LINUX_APPIMAGE_MISSING', 'linux', 'No AppImage found in inspected output paths.');
  issue('LINUX_CURRENT_SOURCE_BUILD_UNVERIFIED', 'linux', 'Historical WSL snapshot hashes do not bind Linux payloads to the latest frozen Git source.');
  issue('LINUX_NATIVE_INSTALL_MATRIX_UNVERIFIED', 'linux', 'Missing independent clean supported-distribution DEB/AppImage install and GUI receipts.');

  const dmgs = artifacts.filter(a => /\.dmg$/i.test(a.path));
  if (!dmgs.length) issue('MACOS_DMG_MISSING', 'macos', 'No DMG found in inspected output paths.');
  issue('MACOS_NATIVE_RUNNER_UNVERIFIED', 'macos', 'No authenticated hosted macOS arm64 + Intel build/install/runtime results.');
  issue('MACOS_SIGNING_NOTARIZATION_UNVERIFIED', 'macos', 'No Developer ID signature, notary ticket, stapling, or Gatekeeper proof.');

  const androidRoot = join(repo, 'apps/desktop/src-tauri/gen/android');
  if (!existsSync(androidRoot)) issue('ANDROID_TAURI_SCAFFOLD_MISSING', 'android', 'No src-tauri/gen/android native Gradle scaffold; `tauri android init` not performed.');
  for (const name of ['adb', 'apksigner', 'sdkmanager']) {
    if (!findExecutable(name, env, platform)) issue('ANDROID_TOOL_NOT_ON_PATH', 'android', `${name} not found on PATH (not proof it is absent from disk).`);
  }
  if (!env.ANDROID_HOME && !env.ANDROID_SDK_ROOT) issue('ANDROID_SDK_UNCONFIGURED', 'android', 'ANDROID_HOME/ANDROID_SDK_ROOT not configured.');
  if (!env.ANDROID_NDK_HOME) issue('ANDROID_NDK_UNCONFIGURED', 'android', 'ANDROID_NDK_HOME not configured.');
  if (!env.JAVA_HOME && !findExecutable('javac', env, platform)) {
    issue('ANDROID_JDK_UNCONFIGURED', 'android', 'Neither JAVA_HOME nor javac on PATH; Gradle/JDK readiness not proven.');
  }
  const rustTargets = runReadOnly('rustup', ['target', 'list', '--installed'], repo, env);
  if (rustTargets.status !== 0 || !/^aarch64-linux-android\s*$/m.test(rustTargets.stdout)) {
    issue('ANDROID_RUST_TARGET_MISSING', 'android', 'Rust aarch64-linux-android target not installed or rustup unavailable.');
  }
  const androidPackages = artifacts.filter(a => /\.(apk|aab)$/i.test(a.path));
  if (!androidPackages.length) issue('ANDROID_SIGNED_PACKAGE_MISSING', 'android', 'No APK or AAB in inspected output paths.');
  issue('ANDROID_SIGNATURE_DEVICE_UNVERIFIED', 'android', 'No independently verified release signing identity and ADB install/resume receipts.');

  // Local self-inspection can never certify hosted provenance, signing-key
  // custody, real native-install acceptance, or a freshly downloadable tag.
  issue('HOSTED_RELEASE_ACCEPTANCE_REQUIRED', 'release', 'Independent commit-bound CI receipts, downloaded asset hashes, signed package tests and release readiness review are required; this local script cannot attest them.');
  return {
    schema: 'constellation-platform-packaging-preflight-v1', ready: false, release_authorized: false,
    root: repo, platform, product: config?.identifier ?? null,
    manifest, artifacts, issues,
    checked_at: 'unspecified (deterministic read-only inventory; no timestamp used for passing)',
  };
}

function cli() {
  const args = process.argv.slice(2);
  if (args.some(a => !['--json', '--help'].includes(a))) {
    process.stderr.write('Usage: node scripts/release/platform-preflight.mjs [--json|--help]\n');
    process.exitCode = 64; return;
  }
  if (args.includes('--help')) {
    process.stdout.write('Read-only release inventory; ALWAYS fails closed until independently verified hosted release gates exist.\n');
    return;
  }
  const here = dirname(fileURLToPath(import.meta.url));
  const result = inspectPreflight(resolve(here, '..', '..'));
  if (args.includes('--json')) process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  else {
    process.stdout.write(`RELEASE BLOCKED (${result.issues.length} fail-closed gates). No packaging or publication performed.\n`);
    for (const item of result.issues) process.stdout.write(`[${item.area}] ${item.id}: ${item.detail}\n`);
    process.stdout.write(`Inspected ${result.artifacts.length} locally existing files; hashes available with --json.\n`);
  }
  process.exitCode = 2;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) cli();
