import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { inspectPreflight, validateSourceManifest, workflowJob } from './platform-preflight.mjs';

const script = fileURLToPath(new URL('./platform-preflight.mjs', import.meta.url));
const root = resolve(dirname(script), '..', '..');

test('workflow section isolation catches real fail-closed Android and release gates', () => {
  const yaml = readFileSync(resolve(root, '.github/workflows/acceptance.yml'), 'utf8');
  const android = workflowJob(yaml, 'android-build');
  const release = workflowJob(yaml, 'release-assets');
  assert.ok(android?.includes('exit 1'));
  assert.ok(release?.includes('exit 1'));
  assert.equal(workflowJob(yaml, 'imaginary-runner'), null);
  assert.equal(workflowJob('  android-build:\n    run: echo safe\n  release-assets:\n    run: exit 1', 'android-build')?.includes('exit 1'), false);
});

test('source manifest rejects path traversal, duplicates and digest drift without writing', () => {
  const relativeFile = 'apps/desktop/src-tauri/tauri.conf.json';
  const originalBytes = readFileSync(resolve(root, relativeFile));
  const base = { Path: relativeFile, Size: originalBytes.length, Hash: createHash('sha256').update(originalBytes).digest('hex') };
  assert.deepEqual(validateSourceManifest([base], root, 1), []);
  assert.match(validateSourceManifest([{ ...base, Path: '../outside' }], root, 1).join(' '), /unsafe/);
  assert.match(validateSourceManifest([base, base], root, 2).join(' '), /unsafe/);
  assert.match(validateSourceManifest([{ ...base, Size: base.Size + 1 }], root, 1).join(' '), /drift/);
  assert.match(validateSourceManifest([{ ...base, Hash: '0'.repeat(64) }], root, 1).join(' '), /drift/);
  assert.match(validateSourceManifest([{ ...base, Path: 'nothing.txt' }], root, 1).join(' '), /Missing/);
});

test('actual local preflight remains unauthorized even when artifacts exist', () => {
  const result = inspectPreflight(root);
  const ids = new Set(result.issues.map(issue => issue.id));
  assert.equal(result.ready, false);
  assert.equal(result.release_authorized, false);
  assert.ok(ids.has('HOSTED_RELEASE_ACCEPTANCE_REQUIRED'));
  assert.ok(ids.has('ANDROID_SIGNATURE_DEVICE_UNVERIFIED'));
  assert.ok(ids.has('MACOS_NATIVE_RUNNER_UNVERIFIED'));
  assert.ok(ids.has('LINUX_CURRENT_SOURCE_BUILD_UNVERIFIED'));
  assert.ok(ids.has('WINDOWS_INSTALL_LIFECYCLE_UNVERIFIED'));
  assert.ok(ids.has('ANDROID_CI_INTENTIONAL_FAILURE'));
  assert.ok(ids.has('RELEASE_CI_INTENTIONAL_FAILURE'));
  assert.ok(result.issues.length >= 8);
});

test('public CLI JSON is machine-readable and returns blocked code 2', () => {
  const run = spawnSync(process.execPath, [script, '--json'], { cwd: root, encoding: 'utf8', timeout: 25000 });
  assert.equal(run.status, 2, run.stderr);
  const response = JSON.parse(run.stdout);
  assert.equal(response.schema, 'constellation-platform-packaging-preflight-v1');
  assert.equal(response.ready, false);
  assert.equal(response.release_authorized, false);
  assert.ok(response.issues.some(issue => issue.id === 'HOSTED_RELEASE_ACCEPTANCE_REQUIRED'));
});

test('invalid CLI invocation fails without treating unknown options as evidence', () => {
  const run = spawnSync(process.execPath, [script, '--approve-release'], { cwd: root, encoding: 'utf8', timeout: 2000 });
  assert.equal(run.status, 64);
  assert.match(run.stderr, /Usage:/);
});
