#!/usr/bin/env node
// Local/hosted, read-only Git source completeness gate. Standard library only.
// Never reads or prints secret contents. No network and no Git modifications.
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const EXPECTED_CRATES = Object.freeze([
  'constellation-agent-loop', 'constellation-backup',
  'constellation-backup-cli', 'constellation-core',
  'constellation-credential-vault', 'constellation-evidence',
  'constellation-project-backup', 'constellation-provider-adapters',
  'constellation-provider-probe', 'constellation-runner-protocol',
  'constellation-runner-service', 'constellation-sync',
  'constellation-tools', 'constellation-workflows',
]);

// These are build inputs, not generated APKs. Android packaging must never
// succeed from an untracked local Studio/Gradle project absent from GitHub.
export const REQUIRED_ANDROID_FILES = Object.freeze([
  'apps/desktop/src-tauri/gen/android/.gitignore',
  'apps/desktop/src-tauri/gen/android/build.gradle.kts',
  'apps/desktop/src-tauri/gen/android/settings.gradle',
  'apps/desktop/src-tauri/gen/android/gradle.properties',
  'apps/desktop/src-tauri/gen/android/gradlew',
  'apps/desktop/src-tauri/gen/android/gradlew.bat',
  'apps/desktop/src-tauri/gen/android/gradle/wrapper/gradle-wrapper.jar',
  'apps/desktop/src-tauri/gen/android/gradle/wrapper/gradle-wrapper.properties',
  'apps/desktop/src-tauri/gen/android/buildSrc/build.gradle.kts',
  'apps/desktop/src-tauri/gen/android/buildSrc/src/main/java/app/constellation/studio/kotlin/BuildTask.kt',
  'apps/desktop/src-tauri/gen/android/buildSrc/src/main/java/app/constellation/studio/kotlin/RustPlugin.kt',
  'apps/desktop/src-tauri/gen/android/app/build.gradle.kts',
  'apps/desktop/src-tauri/gen/android/app/src/main/AndroidManifest.xml',
  'apps/desktop/src-tauri/gen/android/app/src/main/java/app/constellation/studio/MainActivity.kt',
  'apps/desktop/src-tauri/gen/android/app/src/main/res/xml/file_paths.xml',
]);

export const REQUIRED_FILES = Object.freeze([
  ...REQUIRED_ANDROID_FILES,
  ...EXPECTED_CRATES.flatMap((name) => [
    `crates/${name}/Cargo.toml`, `crates/${name}/Cargo.lock`,
    `crates/${name}/src/lib.rs`,
  ]),
  'crates/constellation-backup-cli/src/main.rs',
  'apps/desktop/package.json', 'apps/desktop/pnpm-lock.yaml',
  'apps/desktop/src-tauri/Cargo.toml', 'apps/desktop/src-tauri/Cargo.lock',
  'apps/desktop/src-tauri/tauri.conf.json', 'apps/desktop/src-tauri/src/lib.rs',
  'apps/desktop/src/routes/+page.svelte', 'apps/desktop/src/routes/studio.css',
  'catalog/build-roles.mjs', 'catalog/roles.test.mjs',
  'skills-manifest/inventory.json', 'skills-manifest/validate.mjs',
  'skills-manifest/validate.test.mjs',
  'skills-manifest/reviewed-exercise.test.mjs',
  'tests/release-readiness.ps1',
  '.github/workflows/validate.yml',
  '.github/workflows/acceptance.yml',
  '.github/workflows/source-gate.mjs',
  'tests/ci-source-gate-eleventh-wave.test.mjs',
]);

// Skill qualification receipts are local evidence, not distributable source.
// Explicit configuration JSON is covered by REQUIRED_FILES instead.
const ALLOWED_SOURCE_EXT = /\.(?:rs|svelte|css|ts|js|mjs|ps1|sh|toml|yml|yaml|kt|kts|xml|gradle)$/i;
const FORBIDDEN_TRACKED = /(^|\/)(?:target|node_modules|\.svelte-kit|\.git|\.env(?:\..*)?|__pycache__|dist|local\.properties|key\.properties|keystore\.properties)(?:\/|$)|\.(?:pem|p12|pfx|keystore|jks|key|exe|dll|dmg|appimage|apk|msi)$/i;
const BASE_SOURCE_DIRS = Object.freeze([
  '.github/workflows', 'apps/desktop/src', 'apps/desktop/src-tauri/src',
  'apps/desktop/src-tauri/gen/android/app/src/main',
  'apps/desktop/src-tauri/gen/android/buildSrc/src/main',
  'catalog', 'crates', 'skills-manifest', 'tests',
]);
const IGNORED_DIRS = new Set([
  'target', 'node_modules', '.svelte-kit', '.git', 'dist', 'build', '__pycache__',
]);

function normalize(path) {
  return path.replaceAll('\\', '/');
}

// Work from a tracked Git inventory, not counts of local files or .gitignore.
export function inspectSourceManifest({
  tracked, actual, sources, required = REQUIRED_FILES,
}) {
  const errors = [];
  const trackedSet = new Set(tracked);
  const actualSet = new Set(actual);
  if (!trackedSet.size) errors.push('EMPTY_GIT_INDEX');
  if (!actualSet.size) errors.push('EMPTY_CHECKOUT');
  for (const path of required) {
    if (!actualSet.has(path)) errors.push(`MISSING_REQUIRED:${path}`);
    if (!trackedSet.has(path)) errors.push(`UNTRACKED_REQUIRED:${path}`);
  }
  for (const path of sources) {
    if (!trackedSet.has(path)) errors.push(`UNTRACKED_SOURCE:${path}`);
  }
  for (const path of trackedSet) {
    if (!actualSet.has(path)) errors.push(`TRACKED_MISSING:${path}`);
    if (FORBIDDEN_TRACKED.test(path)) errors.push(`FORBIDDEN_TRACKED:${path}`);
  }
  // Do not silently skip a newly added crate simply because it was omitted
  // from the static inventory.
  for (const path of actualSet) {
    if (/^crates\/[^/]+\/Cargo\.toml$/.test(path)) {
      const lock = path.slice(0, -'Cargo.toml'.length) + 'Cargo.lock';
      if (!actualSet.has(lock) || !trackedSet.has(lock)) {
        errors.push(`MISSING_LOCK:${lock}`);
      }
    }
  }
  return { ok: errors.length === 0, errors, checked: required.length, tracked: trackedSet.size };
}

export function scanCheckout(root, dirs = BASE_SOURCE_DIRS) {
  const present = new Set();
  const sources = new Set();
  const unsafe = [];
  function walk(directory) {
    if (!existsSync(directory)) return;
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const location = join(directory, entry.name);
      const key = normalize(relative(root, location));
      if (entry.isSymbolicLink()) {
        unsafe.push(`SYMLINK_SOURCE:${key}`);
      } else if (entry.isDirectory()) {
        if (!IGNORED_DIRS.has(entry.name)) walk(location);
      } else if (entry.isFile()) {
        present.add(key);
        if (ALLOWED_SOURCE_EXT.test(entry.name)) sources.add(key);
      }
    }
  }
  for (const dir of dirs) walk(join(root, dir));
  // Required paths include files outside the traversed source directories.
  for (const path of REQUIRED_FILES) {
    const resolved = join(root, path);
    if (existsSync(resolved)) {
      const metadata = lstatSync(resolved);
      if (metadata.isSymbolicLink()) unsafe.push(`SYMLINK_REQUIRED:${path}`);
      else if (metadata.isFile()) present.add(path);
    }
  }
  return { present, sources, unsafe };
}

function main() {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  const tracked = execFileSync('git', ['ls-files', '-z', '--cached'], {
    cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024,
  }).split('\0').filter(Boolean).map(normalize);
  const { present, sources, unsafe } = scanCheckout(root);
  // Resolve paths in the index without reading any file contents.
  for (const path of tracked) {
    if (existsSync(join(root, path))) present.add(path);
  }
  const result = inspectSourceManifest({
    tracked, actual: present, sources,
  });
  const errors = [...unsafe, ...result.errors];
  if (errors.length) {
    for (const issue of errors.slice(0, 150)) {
      process.stderr.write(`CI_SOURCE_GATE_FAIL ${issue}\n`);
    }
    process.stderr.write(`Source manifest rejected: ${errors.length} problems (up to 150 shown)\n`);
    process.exitCode = 1;
    return;
  }
  process.stdout.write(`CI_SOURCE_GATE_PASS required=${result.checked} tracked=${result.tracked} source_files=${sources.size}\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    // Avoid printing file content or credential-bearing environment variables.
    process.stderr.write(`CI_SOURCE_GATE_ERROR ${error?.code || 'validation_failed'}\n`);
    process.exitCode = 1;
  }
}
