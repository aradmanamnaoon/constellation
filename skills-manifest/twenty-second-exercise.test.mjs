import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import test from 'node:test';
import {
  twentySecondNames, runTwentySecond, rootCauseAudit,
  loopAudit, classifyAttempts,
} from './twenty-second-exercise.mjs';

const root = resolve(import.meta.dirname, '..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const incident = read('docs/qa/skills-nineteenth-wave-2026-10-31.md');
const recovery = read('docs/qa/skills-source-freeze-refresh-2026-10-31.md');
const activePlan = read('docs/mission/active-task-plan.md');
const receipt = read('skills-manifest/evidence/completion-claims-auditor.json');
const runner = read('skills-manifest/exercise-local.mjs');
const requalifier = read('skills-manifest/requalify-stable.mjs');
const outcome = (result) => result.checks.every((check) => check.result === 'pass');

test('two genuinely separate pinned MIT development skills have real executable exercises', () => {
  assert.deepEqual(twentySecondNames, ['root-cause-debugger', 'loop-breaker']);
  for (const name of twentySecondNames) {
    const report = runTwentySecond(name);
    assert.equal(report.outcome, 'passed', name);
    assert.equal(report.qualification_credit, 0);
    assert.equal(report.upstream.publisher_identity, 'unverified');
    assert.equal(report.checks.length, 10);
    assert.ok(Object.values(report.inputs).every((item) => /^[a-f0-9]{64}$/.test(item.sha256)));
    assert.ok(report.installed_source_sha256 === null
      || report.installed_source_sha256 === report.upstream.sha256);
    assert.ok(report.limitations.some((item) => /No real MCP process/.test(item)));
  }
});

test('root cause diagnosis independently distinguishes changed receipt input from runner tamper', () => {
  const genuine = rootCauseAudit(incident, recovery, activePlan, receipt, runner);
  assert.ok(outcome(genuine));
  assert.deepEqual(genuine.findings.map((h) => h.supported), [true, false, false]);
  const fakeHistory = rootCauseAudit(incident.replaceAll(
    '7133aa6c12334c7927048e86dfcdb2ddda474a08d5318d24a7a7fa34e796675d', '0'.repeat(64),
  ), recovery, activePlan, receipt, runner);
  assert.equal(fakeHistory.checks[0].result, 'fail');
  const adulteratedRunner = rootCauseAudit(incident, recovery, activePlan, receipt,
    runner + '\n/* changed runner: must not attest to historic runner */');
  assert.equal(adulteratedRunner.checks[4].result, 'fail');
  assert.equal(adulteratedRunner.checks[9].result, 'fail');
  const adulteratedPlan = rootCauseAudit(incident, recovery,
    activePlan + '\nUNAPPROVED SENTENCE', receipt, runner);
  assert.equal(adulteratedPlan.checks[2].result, 'fail');
});

test('no fake repaired incident or false passing receipt can earn root cause credit', () => {
  const fakeRefresh = rootCauseAudit(incident, recovery.replace('renewed: 32', 'renewed: 0'),
    activePlan, receipt, runner);
  assert.equal(fakeRefresh.checks[8].result, 'fail');
  const corrupted = JSON.parse(receipt);
  corrupted.inputs.activePlan.sha256 = '0'.repeat(64);
  assert.equal(rootCauseAudit(incident, recovery, activePlan,
    JSON.stringify(corrupted), runner).checks[2].result, 'fail');
});

test('loop policy preserves progress and cuts stalled/permanent or excessive transient retries', () => {
  const observed = loopAudit(incident, recovery, requalifier);
  assert.ok(outcome(observed));
  assert.deepEqual(observed.observations.actualHistory.map((item) => item.credited), [31, 24, 32]);
  const changedSource = loopAudit(incident,
    recovery.replace('renewed: 32', 'renewed: 0'), requalifier);
  assert.equal(changedSource.checks[1].result, 'fail');
  const disabledReplay = loopAudit(incident, recovery,
    requalifier.replace('const second = verify(state)', 'const second = first'));
  assert.equal(disabledReplay.checks[9].result, 'fail');
  const permissionError = classifyAttempts([{credited: 32, state: 'permanent'}]);
  assert.equal(permissionError.action, 'stop_and_escalate');
  const batch = classifyAttempts([
    {credited: 31, state: 'ok'}, {credited: 31, state: 'ok'}, {credited: 32, state: 'ok'},
  ]);
  assert.equal(batch.classification, 'measurable_progress');
  assert.equal(classifyAttempts([{credited: -1, state: 'ok'}]).classification,
    'invalid_evidence');
});

test('unreviewed candidates cannot borrow either evaluator or claim a fake MCP', () => {
  assert.throws(() => runTwentySecond('mcp-reliability-auditor'), /no approved/i);
  assert.throws(() => runTwentySecond('ci-failure-investigator'), /no approved/i);
  assert.throws(() => runTwentySecond('production-readiness-auditor'), /no approved/i);
});
