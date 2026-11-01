import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import test from 'node:test';
import {
  eligibleEighteenth, runEighteenth, integrationReview,
  environmentParity, authenticationReview, regressionReview,
} from './eighteenth-exercise.mjs';

const root = resolve(import.meta.dirname, '..');
const source = (path) => readFileSync(resolve(root, path), 'utf8');
const inputs = {
  perms: source('crates/constellation-core/src/permissions.rs'),
  toolHost: source('apps/desktop/src-tauri/src/tool_host.rs'),
  coreTests: source('crates/constellation-core/tests/security.rs'),
  consent: source('apps/desktop/src-tauri/src/native_consent.rs'),
  package: source('apps/desktop/package.json'),
  lock: source('apps/desktop/pnpm-lock.yaml'),
  validate: source('.github/workflows/validate.yml'),
  acceptance: source('.github/workflows/acceptance.yml'),
  journal: source('crates/constellation-tools/tests/authenticated_journal.rs'),
};
const outcome = (result) => result.checks.every((c) => c.result === 'pass') ? 'passed' : 'failed';

test('each distinct pinned skill has a real, separately replayable Constellation exercise', () => {
  assert.equal(eligibleEighteenth.length, 4);
  for (const name of eligibleEighteenth) {
    const report = runEighteenth(name);
    assert.equal(report.outcome, 'passed', name);
    assert.ok(report.checks.length >= 6, name);
    assert.equal(report.qualification_credit, 0);
    assert.equal(report.upstream.publisher_identity, 'unverified');
    assert.ok(Object.values(report.inputs).every((entry) => entry.sha256.length === 64));
    assert.ok(report.installed_source_sha256 === null
      || report.installed_source_sha256 === report.upstream.sha256,
    'A managed install may earn review status only with an exact pinned SHA match');
  }
});

test('a missing Core permission dimension and weakened native digest binding fail the integration review', () => {
  const good = integrationReview(inputs.perms, inputs.toolHost, inputs.coreTests);
  assert.equal(outcome(good), 'passed');
  const missingTask = integrationReview(
    inputs.perms.replace('pub task_id: TaskId,', ''),
    inputs.toolHost,
    inputs.coreTests,
  );
  assert.equal(missingTask.checks[0].result, 'fail');
  const digestBypass = integrationReview(inputs.perms,
    inputs.toolHost.replace(
      /native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/,
      'native_proof.verify(&operation_id.to_string(), \"unbound\")',
    ),
    inputs.coreTests);
  assert.equal(digestBypass.checks[2].result, 'fail');
});

test('the environment parity exercise catches altered Node pin, missing lock and changed install mode', () => {
  assert.equal(outcome(environmentParity(inputs.package, inputs.lock, inputs.validate,
    inputs.acceptance)), 'passed');
  const wrongNode = environmentParity(inputs.package, inputs.lock,
    inputs.validate.replaceAll('24.17.0', '23.0.0'), inputs.acceptance);
  assert.equal(wrongNode.checks[0].result, 'fail');
  const missingLock = environmentParity(inputs.package,
    inputs.lock.replace('      svelte:', '      NOT_SVELTE:'),
    inputs.validate, inputs.acceptance);
  assert.equal(missingLock.checks[2].result, 'fail');
  const noFreeze = environmentParity(inputs.package, inputs.lock,
    inputs.validate.replaceAll('--frozen-lockfile', ''), inputs.acceptance);
  assert.equal(noFreeze.checks[1].result, 'fail');
});

test('native proof audit rejects digest bypass and unavailable vault fail-open changes', () => {
  assert.equal(outcome(authenticationReview(inputs.consent, inputs.toolHost, inputs.perms)), 'passed');
  const weak = inputs.consent.replace(
    'self.operation_id == operation_id && self.operation_digest == operation_digest',
    'self.operation_id == operation_id',
  );
  assert.equal(authenticationReview(weak, inputs.toolHost, inputs.perms).checks[1].result, 'fail');
  assert.equal(authenticationReview(inputs.consent,
    inputs.toolHost.replace('Authenticated audit key is unavailable; action denied', 'Continue anyway'),
    inputs.perms).checks[4].result, 'fail');
});

test('permission regression review detects broadened grants and removed replay test', () => {
  assert.equal(outcome(regressionReview(inputs.perms, inputs.coreTests, inputs.journal)), 'passed');
  const permissionBypass = regressionReview(
    inputs.perms.replace('self.grants.contains(request)', 'true'),
    inputs.coreTests, inputs.journal);
  assert.equal(permissionBypass.checks[0].result, 'fail');
  const removedTest = regressionReview(inputs.perms, inputs.coreTests,
    inputs.journal.replace(
      'fn denial_survives_restart_and_cannot_be_forged_into_approval()',
      'fn removed_test()',
    ));
  assert.equal(removedTest.checks[3].result, 'fail');
});

test('unreviewed and uninstalled candidates cannot reuse this runner', () => {
  assert.throws(() => runEighteenth('mcp-reliability-auditor'), /Unsupported\/unreviewed/);
  assert.throws(() => runEighteenth('dead-feature-detector'), /Unsupported\/unreviewed/);
});
