import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';
import {
  EXPECTED_CRATES,
  REQUIRED_ANDROID_FILES,
  REQUIRED_FILES,
  inspectSourceManifest,
} from '../.github/workflows/source-gate.mjs';

const root = resolve(import.meta.dirname, '..');
const workflow = readFileSync(resolve(root, '.github/workflows/acceptance.yml'), 'utf8');
const validationWorkflow = readFileSync(resolve(root, '.github/workflows/validate.yml'), 'utf8');
const standard = new Set(REQUIRED_FILES);
const fixture = (overrides = {}) => inspectSourceManifest({
  tracked: standard, actual: standard, sources: new Set(['crates/constellation-core/src/lib.rs']),
  ...overrides,
});

test('complete deterministic expected crate inventory and baseline', () => {
  assert.equal(EXPECTED_CRATES.length, 14);
  assert.equal(REQUIRED_ANDROID_FILES.length, 15);
  assert.equal(new Set(EXPECTED_CRATES).size, EXPECTED_CRATES.length);
  assert.equal(new Set(REQUIRED_ANDROID_FILES).size, REQUIRED_ANDROID_FILES.length);
  assert.equal(fixture().ok, true);
});

test('Android Gradle wrapper, manifest, activity and native build sources must all be committed', () => {
  for (const required of [
    'apps/desktop/src-tauri/gen/android/gradle/wrapper/gradle-wrapper.jar',
    'apps/desktop/src-tauri/gen/android/app/src/main/AndroidManifest.xml',
    'apps/desktop/src-tauri/gen/android/app/src/main/java/app/constellation/studio/MainActivity.kt',
    'apps/desktop/src-tauri/gen/android/buildSrc/build.gradle.kts',
  ]) {
    assert(REQUIRED_ANDROID_FILES.includes(required));
    const notCommitted = new Set(standard);
    notCommitted.delete(required);
    assert(fixture({ tracked: notCommitted }).errors.includes(`UNTRACKED_REQUIRED:${required}`));
  }
  const injected = 'apps/desktop/src-tauri/gen/android/app/src/main/java/app/constellation/studio/Unreviewed.kt';
  assert(fixture({ sources: new Set([injected]) }).errors.includes(`UNTRACKED_SOURCE:${injected}`));
});

test('missing source, untracked manifest and absent lock fail individually', () => {
  const absent = new Set(standard);
  absent.delete('crates/constellation-sync/src/lib.rs');
  assert(fixture({ actual: absent }).errors.includes('MISSING_REQUIRED:crates/constellation-sync/src/lib.rs'));
  const notCommitted = new Set(standard);
  notCommitted.delete('crates/constellation-sync/Cargo.toml');
  assert(fixture({ tracked: notCommitted }).errors.includes('UNTRACKED_REQUIRED:crates/constellation-sync/Cargo.toml'));
  const lockMissing = new Set(standard);
  lockMissing.delete('crates/constellation-sync/Cargo.lock');
  assert(fixture({ actual: lockMissing }).errors.includes('MISSING_LOCK:crates/constellation-sync/Cargo.lock'));
});

test('untracked new source, new crate without lock and forbidden tracked binaries fail', () => {
  assert(fixture({
    sources: new Set(['crates/constellation-core/src/lib.rs', 'crates/constellation-sync/src/uncommitted.rs']),
  }).errors.includes('UNTRACKED_SOURCE:crates/constellation-sync/src/uncommitted.rs'));
  const actual = new Set([...standard, 'crates/new-crate/Cargo.toml']);
  assert(fixture({ actual }).errors.includes('MISSING_LOCK:crates/new-crate/Cargo.lock'));
  const tracked = new Set([...standard, 'dist/installer.exe']);
  assert(fixture({ tracked, actual: new Set([...standard, 'dist/installer.exe']) })
    .errors.includes('FORBIDDEN_TRACKED:dist/installer.exe'));
  const key = new Set([...standard, 'secrets/private.key']);
  assert(fixture({ tracked: key, actual: key })
    .errors.includes('FORBIDDEN_TRACKED:secrets/private.key'));
  for (const secret of ['local.properties', 'key.properties', 'keystore.properties']) {
    const file = `apps/desktop/src-tauri/gen/android/${secret}`;
    const trackedSecrets = new Set([...standard, file]);
    assert(fixture({ tracked: trackedSecrets, actual: trackedSecrets })
      .errors.includes(`FORBIDDEN_TRACKED:${file}`));
  }
});

test('fail closed on an empty index and missing tracked object', () => {
  assert(fixture({ tracked: [] }).errors.includes('EMPTY_GIT_INDEX'));
  assert(fixture({ actual: [] }).errors.includes('EMPTY_CHECKOUT'));
  assert(fixture({
    tracked: new Set([...standard, 'README-absent.md']),
  }).errors.includes('TRACKED_MISSING:README-absent.md'));
});

const families = [
  'rust-format-lint', 'unit-and-integration', 'frontend-check',
  'security-and-supply-chain', 'schema-role-validation', 'skills-200-audit',
  'provider-contract-tests', 'mcp-acp-contract-tests', 'sync-tests',
  'runner-tests', 'desktop-windows', 'desktop-macos-arm', 'desktop-linux',
  'android-build', 'release-assets', 'docs-links', 'repo-size',
];
test('all 17 master CI job families exist exactly once and have an actual job body', () => {
  assert.equal(families.length, 17);
  for (const family of families) {
    const found = workflow.match(new RegExp(`^  ${family}:\\s*$`, 'gm')) || [];
    assert.equal(found.length, 1, family);
  }
  assert.match(workflow, /^permissions:\s*\n\s*contents: read/m);
  assert.match(workflow, /^\s*concurrency:/m);
  assert.match(workflow, /fail-fast: false/);
  assert.doesNotMatch(workflow, /pull_request_target:/);
});

test('every family depends on the source gate; installers only run from explicit dispatch', () => {
  const jobsBody = workflow.slice(workflow.indexOf('\njobs:\n') + '\njobs:\n'.length);
  const starts = [...jobsBody.matchAll(/^  ([a-z][a-z0-9-]+):\s*$/gm)]
    .map((match) => ({ name: match[1], index: match.index }));
  assert.equal(starts.length, 18);
  const jobs = new Map(starts.map((entry, i) => [
    entry.name,
    jobsBody.slice(entry.index, starts[i + 1]?.index ?? jobsBody.length),
  ]));
  for (const family of families) {
    assert.match(jobs.get(family), /needs:\s*\[source-manifest(?:,|\])/,
      `${family} must depend on the source manifest`);
  }
  for (const family of [
    'desktop-windows', 'desktop-macos-arm', 'desktop-linux', 'android-build',
  ]) {
    assert.match(jobs.get(family), /if: github\.event_name == 'workflow_dispatch'/,
      `${family} must require an explicit dispatch`);
  }
  assert.match(jobs.get('desktop-macos-arm'), /arch: arm64/);
  assert.match(jobs.get('desktop-macos-arm'), /arch: x86_64/);
});

test('source gate executes on checkout before dependent jobs and has adversarial self tests', () => {
  assert.match(workflow, /^  source-manifest:/m);
  assert.match(workflow, /node \.github\/workflows\/source-gate\.mjs/);
  assert.match(workflow, /node --test tests\/ci-source-gate-eleventh-wave\.test\.mjs/);
  assert.match(workflow, /needs: \[source-manifest\]/);
  assert.match(workflow, /--frozen-lockfile/);
  assert.match(workflow, /--locked/);
});

test('only immutable action SHAs, no user-influenced command interpolation or publication', () => {
  for (const match of workflow.matchAll(/^\s*- uses:\s*([^\s#]+)/gm)) {
    assert.match(match[1], /@[0-9a-f]{40}$/, `action is mutable: ${match[1]}`);
  }
  assert.doesNotMatch(workflow, /(?:^|\n)\s*pull_request_target:/);
  assert.doesNotMatch(workflow, /\$\{\{\s*github\.event\.(?:pull_request|issue|head_commit)/);
  assert.doesNotMatch(workflow, /\b(?:gh release create|softprops\/action-gh-release|actions\/create-release)\b/);
  assert.match(workflow, /contents: read/);
  assert.match(workflow, /persist-credentials: false/);
});

test('both hosted workflows pin every third-party GitHub Action to an immutable source commit', () => {
  for (const [name, source] of [
    ['acceptance', workflow],
    ['validation', validationWorkflow],
  ]) {
    const calls = [...source.matchAll(/^\s*- uses:\s*([^\s#]+)/gm)];
    assert(calls.length >= 5, `${name} must retain real hosted Action calls`);
    for (const match of calls) {
      assert.match(match[1], /^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+@[0-9a-f]{40}$/,
        `${name}: unpinned or dynamic Action: ${match[1]}`);
    }
    const forged = source.replace(calls[0][1], 'actions/checkout@v4');
    assert([...forged.matchAll(/^\s*- uses:\s*([^\s#]+)/gm)]
      .some((match) => !/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+@[0-9a-f]{40}$/.test(match[1])));
  }
});

test('every hosted checkout explicitly refuses to persist GitHub credentials', () => {
  for (const [name, source] of [
    ['acceptance', workflow],
    ['validation', validationWorkflow],
  ]) {
    const checkouts = [...source.matchAll(/^      - uses: actions\/checkout@[0-9a-f]{40}\r?\n([\s\S]*?)(?=^      - (?:uses:|name:|run:|shell:)|^  [a-z][a-z-]+:|\Z)/gm)];
    assert(checkouts.length >= 2, `${name}: expected native checkout gates`);
    for (const [, stepBody] of checkouts) {
      assert.match(stepBody, /^        with:\r?\n          persist-credentials: false(?:\r?\n|$)/m,
        `${name}: checkout must disable credential persistence`);
    }
  }
});

test('release path is opt-in and fail closed if platform, evidence or signing unverified', () => {
  const release = workflow.slice(workflow.indexOf('\n  release-assets:'));
  assert.match(release, /github\.event_name == 'workflow_dispatch'/);
  assert.match(release, /needs: \[/);
  assert.match(release, /release-readiness\.ps1/);
  assert.match(release, /exit 1/);
  assert.doesNotMatch(release, /\b(?:upload-release-asset|gh release upload|gh release create)\b/);
  assert.match(workflow, /android-build:[\s\S]*?workflow_dispatch/);
});
