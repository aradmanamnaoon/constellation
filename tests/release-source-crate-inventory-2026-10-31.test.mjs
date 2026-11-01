import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { EXPECTED_CRATES } from '../.github/workflows/source-gate.mjs';

const root = resolve(import.meta.dirname, '..');
const release = readFileSync(resolve(root, 'tests/release-readiness.ps1'), 'utf8');
const workflow = readFileSync(resolve(root, '.github/workflows/validate.yml'), 'utf8');

function declaredCrates(source) {
  const declaration = source.match(/\$expectedCrates\s*=\s*@\(([\s\S]*?)\n\s*\)/);
  if (!declaration) throw new Error('Expected crates inventory missing');
  return [...declaration[1].matchAll(/'([^']+)'/g)].map((match) => match[1]);
}

function assertEqualInventory(source) {
  const declared = declaredCrates(source);
  assert.deepEqual(new Set(declared), new Set(EXPECTED_CRATES));
  assert.equal(declared.length, new Set(declared).size, 'Duplicated crate would mask omission');
}

test('release preflight requires the same canonical 14 Rust crates as the signed source gate', () => {
  assert.equal(EXPECTED_CRATES.length, 14);
  assert.ok(EXPECTED_CRATES.includes('constellation-runner-service'));
  assertEqualInventory(release);
  assert.match(workflow, /constellation-runner-service/);
});

test('omitted remote-runner service and duplicate entries fail release inventory audit', () => {
  assert.throws(() => assertEqualInventory(release.replace("'constellation-runner-service',", '')));
  assert.throws(() => assertEqualInventory(release.replace(
    "'constellation-runner-service',",
    "'constellation-runner-service', 'constellation-runner-service',",
  )));
});
