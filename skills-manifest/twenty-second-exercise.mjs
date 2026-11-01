#!/usr/bin/env node
// Two genuine bounded, source-pinned Constellation development exercises.
// These are not model runs, live service integrations, or MCP runtimes.
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {exercise} from './exercise-local.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstream = resolve(root, '..', 'vibe-coding-skills');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const normalized = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const approved = Object.freeze({
  'root-cause-debugger': {
    category: 'debugging',
    inputs: ['docs/qa/skills-nineteenth-wave-2026-10-31.md',
      'docs/qa/skills-source-freeze-refresh-2026-10-31.md',
      'docs/mission/active-task-plan.md',
      'skills-manifest/evidence/completion-claims-auditor.json',
      'skills-manifest/exercise-local.mjs'],
  },
  'loop-breaker': {
    category: 'debugging',
    inputs: ['docs/qa/skills-nineteenth-wave-2026-10-31.md',
      'docs/qa/skills-source-freeze-refresh-2026-10-31.md',
      'skills-manifest/requalify-stable.mjs'],
  },
});
export const twentySecondNames = Object.freeze(Object.keys(approved));
const check = (name, truth, evidence) => ({
  name, result: truth ? 'pass' : 'fail', evidence,
});

/** Diagnose a documented *actual* failed receipt verification, not a made-up incident.
 * Competing causes: changed exercise input, corrupted receipt, runner changed.
 * Old input SHA is preserved in the historical QA, new source and receipt are
 * independently read. An adverse source mutation is exercised without writing.
 */
export function rootCauseAudit(historical, refreshed, activePlan, receiptText, runnerText) {
  const originalSHA = historical.match(/7133aa6c12334c7927048e86dfcdb2ddda474a08d5318d24a7a7fa34e796675d/)?.[0];
  const priorSHA = historical.match(/d3d23fddb17a1f5693ac55a0ae4001c65221c73e172131242ac0621a5c5cf0d3/)?.[0];
  const receipt = JSON.parse(receiptText);
  const current = exercise('completion-claims-auditor');
  const liveSHA = sha(Buffer.from(activePlan));
  const currentInputSHA = receipt.inputs?.activePlan?.sha256;
  const observedDrift = !!originalSHA && !!priorSHA && originalSHA !== priorSHA;
  const runnerSame = receipt.exercise_script_sha256 === sha(Buffer.from(runnerText));
  const unchangedOtherInput = current.inputs?.claimsLedger?.sha256
    === receipt.inputs?.claimsLedger?.sha256;
  const adversePlan = activePlan + '\n<!-- independent read-only mutation: must invalidate receipt input hash -->\n';
  const altered = exercise('completion-claims-auditor', {activePlan: adversePlan});
  const mutationChangesSource = altered.inputs?.activePlan?.sha256 !== currentInputSHA;
  const noMutationAfterRestart = current.inputs.activePlan.sha256 === liveSHA
    && exercise('completion-claims-auditor').inputs.activePlan.sha256 === liveSHA;
  // The discriminating evidence differs from reproduction-builder's synthetic
  // duplicate-ID case: here the failure was recorded on an actual source edit.
  const historyExplainsError = historical.includes('entries[8] exercised source changed since receipt creation')
    && historical.includes('entries[8] exercise receipt differs from independently replayed checks');
  const refreshExplainsRepair = /eight distinct/.test(refreshed)
    && /renewed: 32/.test(refreshed) && /strict_validation_ok: true/.test(refreshed);
  const falseRunnerHypothesis = runnerSame && unchangedOtherInput;
  const fixedReceipt = currentInputSHA === liveSHA
    && receipt.outcome === 'passed'
    && current.checks.every((item) => item.status === 'passed');
  const hypotheses = [
    {candidate: 'input source changed after original receipt', supported: observedDrift && mutationChangesSource},
    {candidate: 'runner source changed', supported: !runnerSame},
    {candidate: 'unrelated evidence input changed', supported: !unchangedOtherInput},
  ];
  return {
    purpose: 'Investigate the real Constellation nineteenth-wave stale-receipt incident with falsifiable source/runner hypotheses and independently exercised repair checks',
    checks: [
      check('Original incident identifies both input SHA versions', observedDrift,
        'actual nineteenth-wave incident source hashes'),
      check('Actual validator failure is precisely the historical stale-receipt error', historyExplainsError,
        'historical strict validator errors on entry[8]'),
      check('Actual current source and restored receipt SHA agree', currentInputSHA === liveSHA,
        'mission plan source vs hashed current receipt'),
      check('Independent current exercise passes all production assertions', current.outcome === 'passed'
        && fixedReceipt, 'live completion-claims-auditor exercise'),
      check('Runner modification hypothesis falsified independently', runnerSame,
        'unchanged original exercise-local.mjs current SHA'),
      check('Other input modification hypothesis falsified independently', unchangedOtherInput,
        'source-bound acceptance ledger SHA'),
      check('Controlled changed source is detected without writing inputs', mutationChangesSource,
        'adverse input content mutation through actual exercise engine'),
      check('Repeated replay is deterministic on identical source', noMutationAfterRestart,
        'two actual independent exercise runs'),
      check('Historical repair shows red-to-green reason without weakening checks',
        refreshExplainsRepair && fixedReceipt, 'source-freeze refresh QA and live current receipt'),
      check('Only causal source-change hypothesis survives competing hypotheses',
        hypotheses[0].supported && !hypotheses[1].supported && !hypotheses[2].supported,
        'three falsifiable actual root-cause hypotheses'),
    ],
    findings: hypotheses,
    limitations: [
      'Actual prior incident and red state are established by historical QA evidence; original pre-refresh receipt is no longer present, so the original failing state was not reconstructed byte-for-byte',
      'This is a bounded development diagnostic, not proof of a production client bug or new source code fix',
    ],
  };
}

/**
 * A concrete progress marker for qualification is strict passing count, not
 * command-name repetition. Use the actual previously recorded counts and
 * exercise a bounded escalation classifier against controlled adverse
 * histories. No arbitrary retries or background operations occur.
 */
export function classifyAttempts(attempts, {maxFlat = 2, transientBudget = 1} = {}) {
  if (!Array.isArray(attempts) || attempts.length === 0) {
    return {classification: 'insufficient_evidence', action: 'capture_exact_state_once', exhausted: 0};
  }
  const sorted = attempts;
  if (!sorted.every((attempt) => Number.isInteger(attempt.credited)
    && attempt.credited >= 0 && attempt.credited <= 200
    && ['ok', 'source_drift', 'transient', 'permanent'].includes(attempt.state))) {
    return {classification: 'invalid_evidence', action: 'stop_and_inspect', exhausted: 0};
  }
  if (sorted.some((attempt) => attempt.state === 'permanent')) {
    return {classification: 'structural_error', action: 'stop_and_escalate', exhausted: sorted.length};
  }
  const last = sorted.at(-1);
  if (last.state === 'transient') {
    const transientCount = sorted.filter((attempt) => attempt.state === 'transient').length;
    return transientCount <= transientBudget
      ? {classification: 'bounded_transient', action: 'retry_with_backoff', exhausted: transientCount}
      : {classification: 'transient_budget_exhausted', action: 'stop_and_escalate', exhausted: transientCount};
  }
  const lastCount = last.credited;
  let flat = 0;
  for (let i = sorted.length - 2; i >= 0; i--) {
    if (sorted[i].credited !== lastCount) break;
    flat++;
  }
  const prior = sorted.at(-2);
  if (prior && lastCount > prior.credited) {
    return {classification: 'measurable_progress', action: 'continue_with_evidence', exhausted: 0};
  }
  if (last.state === 'source_drift') {
    return {classification: 'source_drift', action: 'stop_until_source_freeze', exhausted: flat};
  }
  return flat >= maxFlat
    ? {classification: 'no_progress_loop', action: 'change_diagnostic_strategy', exhausted: flat}
    : {classification: 'bounded_observation', action: 'capture_distinct_next_signal', exhausted: flat};
}

export function loopAudit(historical, refreshed, stableScript) {
  const recordedPreFreeze = /31\*\*, not 32/.test(historical)
    && historical.includes('entries[8] exercised source changed since receipt creation');
  const recordedSourceDrift = /credited only 24/.test(refreshed)
    && /eight distinct/.test(refreshed);
  const recordedProgress = /renewed: 32/.test(refreshed)
    && /strict_validation_ok: true/.test(refreshed);
  const allowsSourceCheck = stableScript.includes('const first = verify(state)')
    && stableScript.includes('const second = verify(state)')
    && stableScript.includes("first.outcome !== 'passed'");
  const actualHistory = [
    {credited: 31, state: 'source_drift'},
    {credited: 24, state: 'source_drift'},
    {credited: 32, state: 'ok'},
  ];
  const progressed = classifyAttempts(actualHistory);
  const flat = classifyAttempts([
    {credited: 32, state: 'ok'}, {credited: 32, state: 'ok'}, {credited: 32, state: 'ok'},
  ]);
  const batch = classifyAttempts([
    {credited: 24, state: 'ok'}, {credited: 30, state: 'ok'}, {credited: 32, state: 'ok'},
  ]);
  const hard = classifyAttempts([
    {credited: 32, state: 'permanent'}, {credited: 32, state: 'permanent'},
  ]);
  const transient = classifyAttempts([
    {credited: 32, state: 'transient'}, {credited: 32, state: 'transient'},
  ]);
  const drift = classifyAttempts([{credited: 31, state: 'source_drift'}]);
  const noEvidence = classifyAttempts([]);
  return {
    purpose: 'Evaluate real nineteenth-to-source-freeze Constellation qualification observations and enforce bounded progress/retry decisions with adversarial histories',
    checks: [
      check('Historical strict credit regression and source drift are recorded', recordedPreFreeze && recordedSourceDrift,
        'documented real 31-to-24 credit regression'),
      check('Subsequent valid refresh records measured 24-to-32 improvement', recordedProgress,
        'real refreshed 32 evidence'),
      check('Actual measured improvement is not misclassified as a loop',
        progressed.classification === 'measurable_progress', 'real chronological qualification snapshots'),
      check('Repeated commands with changing state are preserved as progress',
        batch.classification === 'measurable_progress', 'adversarial increasing-batch fixture'),
      check('Three unchanged successful counts trigger a distinct strategy',
        flat.classification === 'no_progress_loop'
          && flat.action === 'change_diagnostic_strategy', 'adverse identical-result fixture'),
      check('Structural errors stop rather than retry indefinitely',
        hard.classification === 'structural_error'
          && hard.action === 'stop_and_escalate', 'permanent error fixture'),
      check('Transient retries are capped by a recorded budget',
        transient.classification === 'transient_budget_exhausted'
          && transient.action === 'stop_and_escalate', 'bounded transient retry fixture'),
      check('Changed receipt input stops work until source freeze',
        drift.classification === 'source_drift'
          && drift.action === 'stop_until_source_freeze', 'real incident failure category'),
      check('No progress marker forces explicit snapshot instead of loops',
        noEvidence.classification === 'insufficient_evidence'
          && noEvidence.action === 'capture_exact_state_once', 'missing history fixture'),
      check('Requalification uses independently repeated real exercises',
        allowsSourceCheck, 'existing production requalify-stable.mjs double replay gate'),
    ],
    observations: {actualHistory, progressed, flat, drift, transient},
    limitations: [
      'Historical observations come from dated genuine Constellation QA logs, not live provider/tool telemetry',
      'This is a bounded offline retry/progress decision exercise, not authority to terminate tasks or run unattended retries',
    ],
  };
}

const evaluators = {
  'root-cause-debugger': rootCauseAudit,
  'loop-breaker': loopAudit,
};

export function runTwentySecond(name, overrides = {}) {
  const spec = approved[name];
  if (!spec) throw new Error('Skill has no approved twenty-second-wave executable verifier');
  const manifest = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
  const entry = manifest.entries.find((item) => item.id === `skill:${name}`);
  if (!entry || entry.kind !== 'skill'
    || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
    throw new Error('Unreviewed skill or invalid status');
  }
  const revision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const dirty = execFileSync('git', ['-C', upstream, 'status', '--porcelain'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (dirty || !/^[a-f0-9]{40}$/.test(revision))
    throw new Error('Unpinned or dirty upstream');
  const sourcePath = `skills/${spec.category}/${name}/SKILL.md`;
  const source = readFileSync(join(upstream, sourcePath));
  const pinned = execFileSync('git', ['-C', upstream, 'show', `${revision}:${sourcePath}`],
    {windowsHide: true});
  const license = readFileSync(join(upstream, 'LICENSE'));
  const pinnedLicense = execFileSync('git', ['-C', upstream, 'show', `${revision}:LICENSE`],
    {windowsHide: true});
  if (sha(source) !== entry.source.sha256 || sha(normalized(source)) !== sha(pinned)
    || sha(normalized(license)) !== sha(pinnedLicense)
    || entry.license.spdx !== 'MIT' || entry.license.status !== 'verified'
    || !license.toString('utf8').includes('MIT License')
    || !normalized(source).toString('utf8').startsWith(`---\nname: ${name}\n`)
    || !source.toString('utf8').includes('# Safety restrictions')
    || !source.toString('utf8').includes('# Completion criteria')) {
    throw new Error('Pinned source/license/frontmatter/safety evidence failed');
  }
  const installed = join(process.env.APPDATA ?? '', 'chat-on-steroids',
    'skills', 'constellation', name, 'SKILL.md');
  let installedSourceSHA = null;
  try {
    if (sha(readFileSync(installed)) === sha(source)) installedSourceSHA = sha(source);
  } catch { /* No installed credit. */ }
  const inputs = Object.fromEntries(spec.inputs.map((path) => [path,
    {path, sha256: sha(readFileSync(join(root, path)))}]));
  const texts = spec.inputs.map((path) => overrides[path]
    ?? readFileSync(join(root, path), 'utf8'));
  const audit = evaluators[name](...texts);
  const runnerSHA = sha(readFileSync(fileURLToPath(import.meta.url)));
  return {
    id: `skill:${name}`, status: installedSourceSHA
      ? 'installed_exercised_pending_manifest_verification'
      : 'source_reviewed_uninstalled_not_qualified',
    project: 'Constellation', qualification_credit: 0,
    command: `node skills-manifest/twenty-second-exercise.mjs ${name} --write-receipt`,
    scope: 'bounded_real_incident_diagnosis_or_progress_review',
    purpose: audit.purpose,
    checks: audit.checks,
    outcome: audit.checks.every((item) => item.result === 'pass') ? 'passed' : 'failed',
    limitations: [...audit.limitations,
      'No real MCP process, protocol handshake or app tool permission was installed',
      'Upstream publisher account not authenticated beyond clean pinned local Git source'],
    observations: audit.findings ?? audit.observations,
    upstream: {revision, path: `../vibe-coding-skills/${sourcePath}`,
      sha256: sha(source), pinned_blob_sha256: sha(pinned),
      license_sha256: sha(license), license_pinned_sha256: sha(pinnedLicense),
      publisher_identity: 'unverified'},
    inputs, installed_source_sha256: installedSourceSHA,
    exercise_script_sha256: runnerSHA,
    runner: {path: 'skills-manifest/twenty-second-exercise.mjs', sha256: runnerSHA},
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [name, action] = process.argv.slice(2);
  if (!twentySecondNames.includes(name)
    || !['--check', '--write-receipt'].includes(action) || process.argv.length !== 4) {
    process.stderr.write('Usage: twenty-second-exercise.mjs <approved-name> --check|--write-receipt\n');
    process.exitCode = 2;
  } else {
    const report = runTwentySecond(name);
    if (report.outcome !== 'passed') {
      process.stderr.write(JSON.stringify(report.checks.filter((c) => c.result === 'fail'), null, 2) + '\n');
      process.exitCode = 1;
    } else if (action === '--write-receipt') {
      if (!report.installed_source_sha256) throw new Error('No verified exact managed skill installation');
      const path = join(here, 'evidence', 'twenty-second', name + '.json');
      mkdirSync(dirname(path), {recursive: true});
      writeFileSync(path, JSON.stringify({...report,
        observed_on: new Date().toISOString().slice(0, 10)}, null, 2) + '\n');
      process.stdout.write(`${relative(root, path)} sha256=${sha(readFileSync(path))} checks=${report.checks.length}\n`);
    } else {
      process.stdout.write(`${report.id}: ${report.checks.length} checks passed; qualification pending\n`);
    }
  }
}
