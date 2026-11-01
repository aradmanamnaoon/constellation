import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {inspectReadiness, runTwentyFifth} from './twenty-fifth-exercise.mjs';

const baseline = {
  schema: 'constellation.local_readiness_probe.v1', read_only: true,
  release_ready: false, failed_count: 7,
  gates: [
    ['clean_git_worktree', false], ['full_source_commit', true],
    ['configured_git_remote', false], ['versioned_rust_lockfiles', false],
    ['committed_source_completeness', false],
    ['required_role_definitions', true],
    ['required_installed_exercised_integrations', false],
    ['master_acceptance_ledger', false],
    ['hosted_ci_signatures_and_public_release', false],
  ].map(([name, passed]) => ({name, passed})),
};
const environment = {
  statusDirty: true, remotePresent: false, lockfileGap: true,
};
const audit = (report = baseline, state = environment) => inspectReadiness(report, state);

test('blocked readiness audit corroborates independent Git/lockfile state and rejects false success', () => {
  const result = audit();
  assert.equal(result.blockedRelease, true);
  assert.equal(result.checks.length, 10);
  assert.ok(result.checks.every((check) => check.result === 'pass'));
  assert.ok(result.blocked.includes('hosted_ci_signatures_and_public_release'));
});

test('a forged pass, missing gate, duplicate gate, or false read-only claim fails closed', () => {
  const falseSuccess = audit({...baseline, release_ready: true});
  assert.equal(falseSuccess.coherent, false);
  const duplicate = {...baseline, gates: [...baseline.gates, baseline.gates[0]]};
  assert.throws(() => audit(duplicate), /Duplicate/);
  const missing = {...baseline, gates: baseline.gates.slice(1)};
  assert.equal(audit(missing).coherent, false);
  assert.equal(audit({...baseline, read_only: false}).coherent, false);
});

test('Git status, remote, and lockfile observations must not be forged in the receipt', () => {
  const falseGit = audit(baseline, {...environment, statusDirty: false});
  assert.equal(falseGit.independent, false);
  assert.equal(falseGit.checks[1].result, 'fail');
  const falseRemote = audit(baseline, {...environment, remotePresent: true});
  assert.equal(falseRemote.independent, false);
  assert.equal(falseRemote.checks[2].result, 'fail');
  const falseLock = audit(baseline, {...environment, lockfileGap: false});
  assert.equal(falseLock.independent, false);
  assert.equal(falseLock.checks[3].result, 'fail');
});

test('only the approved pinned candidate can execute this operational exercise', () => {
  assert.throws(() => runTwentyFifth('release-verification-agent'), /Not an independently approved/);
  const entry = JSON.parse(readFileSync(new URL('./inventory.json', import.meta.url)))
    .entries.find((item) => item.id === 'skill:production-readiness-auditor');
  assert.ok(entry && ['source_reviewed', 'installed_vetted'].includes(entry.status));
});

test('actual local Git index, lockfile and role probes execute without nested validator calls', () => {
  const run = runTwentyFifth();
  assert.equal(run.outcome, 'passed');
  assert.equal(run.observations.release_ready, false);
  assert.equal(run.observations.clean_install_startup, 'not_executed_current_source_unfrozen');
  assert.equal(run.observations.hosted_ci_and_remote_publication,
    'not_executed_no_remote_attestation');
  assert.ok(run.inputs['tests/release-readiness.ps1']?.sha256?.length === 64);
  assert.ok(run.checks.every((check) => check.result === 'pass'));
});
