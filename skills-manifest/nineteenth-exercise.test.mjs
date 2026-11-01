import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import test from 'node:test';
import {nineteenthNames, runNineteenth, recoverContext,
  reproduceManifestFailure} from './nineteenth-exercise.mjs';

const root = resolve(import.meta.dirname, '..');
const file = (path) => readFileSync(resolve(root, path), 'utf8');
const instructions = file('AGENTS.md');
const ledger = file('docs/quality/acceptance-ledger.md');
const core = file('crates/constellation-core/src/lib.rs');
const validator = file('skills-manifest/validate.mjs');
const validatorTests = file('skills-manifest/validate.test.mjs');
const inventory = JSON.parse(file('skills-manifest/inventory.json'));

test('both originally source-reviewed named skills produce distinct actual local evidence', () => {
  assert.deepEqual(nineteenthNames, ['project-context-recovery', 'reproduction-builder']);
  for (const name of nineteenthNames) {
    const result = runNineteenth(name);
    assert.equal(result.outcome, 'passed', name);
    assert.equal(result.qualification_credit, 0);
    assert.equal(result.upstream.publisher_identity, 'unverified');
    assert.ok(result.checks.length >= 7);
    assert.ok(Object.values(result.inputs).every((item) => /^[a-f0-9]{64}$/.test(item.sha256)));
    assert.ok(result.installed_source_sha256 === null
      || result.installed_source_sha256 === result.upstream.sha256);
  }
});

test('read-only recovery rejects forged acceptance and missing authority', () => {
  const metadata = {head: 'a'.repeat(40), status: ' M AGENTS.md\n?? example.txt'};
  const valid = recoverContext(instructions, ledger, core, metadata);
  assert.ok(valid.checks.every((item) => item.result === 'pass'));
  assert.match(valid.state.skill_milestone, /unverified/);
  assert.match(valid.state.workspace, /preserve/);
  const falseVerified = recoverContext(instructions,
    ledger.replace(/(\| INTEG-01\s*\|[^\n]*)\*\*UNVERIFIED\*\*/,
      '$1**VERIFIED**'), core, metadata);
  assert.equal(falseVerified.checks[4].result, 'fail');
  const removedAuthority = recoverContext(instructions.replace(
    '200 real role definitions and 200 vetted, installed/exercised skills/MCP',
    'all requirements have been implemented'), ledger, core, metadata);
  assert.equal(removedAuthority.checks[2].result, 'fail');
  const badGit = recoverContext(instructions, ledger, core,
    {head: 'not-a-commit', status: 'broken status record'});
  assert.equal(badGit.checks[0].result, 'fail');
  assert.equal(badGit.checks[1].result, 'fail');
});

test('real production validator reproduces exact minimal red/green path twice', () => {
  const result = reproduceManifestFailure(validator, validatorTests, instructions, inventory);
  assert.ok(result.checks.every((item) => item.result === 'pass'));
  assert.equal(result.reproduction.repeat_count, 2);
  assert.equal(result.reproduction.minimal_records, 2);
  assert.match(result.reproduction.project_bug_claim, /NOT evidence/);
});

test('reproducer refuses duplicate baseline inputs and removed duplicate-detection guard', () => {
  const corrupted = structuredClone(inventory);
  const ids = corrupted.entries.filter((item) => item.kind === 'cli');
  ids[1].id = ids[0].id;
  const invalid = reproduceManifestFailure(validator, validatorTests, instructions, corrupted);
  assert.equal(invalid.checks[0].result, 'fail');
  assert.equal(invalid.checks[1].result, 'fail');
  const noSourceGuard = reproduceManifestFailure(
    validator.replace('seen.has(normalized)', 'false'), validatorTests, instructions, inventory);
  assert.equal(noSourceGuard.checks[6].result, 'fail');
  const noTest = reproduceManifestFailure(
    validator, validatorTests.replace('does not accept duplicate entry IDs',
      'never covers duplicate identities'), instructions, inventory);
  assert.equal(noTest.checks[6].result, 'fail');
});

test('unknown names and unrelated candidates cannot borrow this runner', () => {
  assert.throws(() => runNineteenth('mcp-reliability-auditor'), /No approved/);
  assert.throws(() => runNineteenth('ui-flow-verifier'), /No approved/);
});
