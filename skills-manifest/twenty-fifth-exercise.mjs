#!/usr/bin/env node
// Bounded independent local operational-readiness checks. This runner must
// never call release-readiness.ps1: that script itself invokes the strict skill
// validator, creating unbounded recursion after the skill is promoted.
// Passing the AUDIT does not mean the application is release-ready.
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstream = resolve(root, '..', 'vibe-coding-skills');
const name = 'production-readiness-auditor';
const upstreamFile = 'skills/deployment/production-readiness-auditor/SKILL.md';
const watched = Object.freeze([
  'AGENTS.md',
  'tests/release-readiness.ps1',
  '.github/workflows/acceptance.yml',
  'docs/qa/ci-eleventh-wave-2026-10-31.md',
  'docs/quality/acceptance-ledger.md',
  'catalog/build-roles.mjs',
]);
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const normalize = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const check = (name, result, evidence) => ({
  name, result: result ? 'pass' : 'fail', evidence,
});

export function inspectReadiness(report, environment) {
  if (!report || report.schema !== 'constellation.local_readiness_probe.v1'
    || !Array.isArray(report.gates) || report.gates.length < 8) {
    throw new Error('Missing independently observed local readiness gates');
  }
  const gates = new Map(report.gates.map((item) => [item.name, item]));
  if (gates.size !== report.gates.length
    || report.gates.some((item) => typeof item.passed !== 'boolean')) {
    throw new Error('Duplicate or malformed preflight gate');
  }
  const blocked = report.gates.filter((item) => !item.passed).map((item) => item.name);
  const gate = (id) => gates.get(id)?.passed;
  const required = [
    'clean_git_worktree', 'full_source_commit', 'configured_git_remote',
    'versioned_rust_lockfiles', 'committed_source_completeness',
    'required_role_definitions', 'required_installed_exercised_integrations',
    'master_acceptance_ledger', 'hosted_ci_signatures_and_public_release',
  ];
  const coherent = report.read_only === true
    && report.release_ready === (blocked.length === 0)
    && report.failed_count === blocked.length
    && required.every((id) => gates.has(id));
  const independent = environment.statusDirty === !gate('clean_git_worktree')
    && environment.remotePresent === gate('configured_git_remote')
    && environment.lockfileGap === !gate('versioned_rust_lockfiles');
  const blockedRelease = report.release_ready === false && blocked.length > 0;
  return {
    coherent,
    independent,
    blockedRelease,
    blocked,
    checks: [
      check('Local readiness observations are read-only with mandatory operational categories',
        coherent, 'independent repository observations with named release prerequisites'),
      check('Uncommitted source is independently observed and blocks clean release',
        environment.statusDirty && gate('clean_git_worktree') === false,
        'git status --porcelain independently observed'),
      check('Git remote absence is detected independently',
        !environment.remotePresent && gate('configured_git_remote') === false,
        'git remote inventory independently observed'),
      check('Untracked Rust lockfiles are independently detected',
        environment.lockfileGap && gate('versioned_rust_lockfiles') === false,
        'tracked source index vs existing Cargo.lock paths'),
      check('Incomplete source and acceptance ledger remain explicit release blockers',
        gate('committed_source_completeness') === false
          && gate('master_acceptance_ledger') === false,
        'tracked Git sources and 188-row acceptance ledger'),
      check('Unverified 200 integrations and hosted release stay blocked',
        gate('required_installed_exercised_integrations') === false
          && gate('hosted_ci_signatures_and_public_release') === false,
        'no fabricated integration completion or remote publication evidence'),
      check('A correct role definition count cannot override operational gates',
        gate('required_role_definitions') === true && blockedRelease,
        'actual separately green catalogue structural check'),
      check('Auditor cross-checks source facts independently of preflight assertions',
        independent, 'actual Git index/status/remote/locks compared with local readiness observation'),
      check('Fake success with unchanged blocked gates fails closed',
        !inspectReadinessCandidate({...report, release_ready: true}, environment),
        'adverse false-green preflight envelope'),
      check('Forged clean source, remote, or lockfile status fails independent cross-check',
        !inspectReadinessCandidate({
          ...report, gates: report.gates.map((item) =>
            ['clean_git_worktree', 'configured_git_remote',
              'versioned_rust_lockfiles'].includes(item.name)
              ? {...item, passed: true} : item),
          failed_count: blocked.length - 3,
        }, environment), 'adverse inconsistent Git and lockfile evidence'),
    ],
  };
}

function inspectReadinessCandidate(report, environment) {
  try {
    const gates = new Map(report.gates.map((item) => [item.name, item.passed]));
    const failed = report.gates.filter((item) => item.passed === false).length;
    return report.read_only === true
      && report.release_ready === (failed === 0)
      && report.failed_count === failed
      && gates.get('clean_git_worktree') === !environment.statusDirty
      && gates.get('configured_git_remote') === environment.remotePresent
      && gates.get('versioned_rust_lockfiles') === !environment.lockfileGap;
  } catch { return false; }
}

function currentGitEvidence() {
  const exec = (...args) => execFileSync('git', ['-C', root, ...args],
    {encoding: 'utf8', windowsHide: true, timeout: 10000}).trim();
  const statusDirty = exec('status', '--porcelain').length > 0;
  const remotePresent = exec('remote').length > 0;
  const tracked = new Set(exec('ls-files').replaceAll('\\', '/').split(/\r?\n/));
  const lockfileGap = (exec('ls-files', 'crates/*/Cargo.toml')
    .split(/\r?\n/).filter(Boolean).some((path) =>
      !tracked.has(path.replace(/Cargo\.toml$/, 'Cargo.lock'))))
    || !tracked.has('crates/constellation-agent-loop/Cargo.lock');
  return {statusDirty, remotePresent, lockfileGap};
}

function runGate(environment) {
  const head = execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const status = execFileSync('git', ['-C', root, 'status', '--porcelain', '-uall'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const acceptance = readFileSync(join(root, 'docs', 'quality', 'acceptance-ledger.md'), 'utf8');
  const inventory = JSON.parse(readFileSync(join(here, 'inventory.json')));
  const creditedDeclarations = inventory.entries.filter((item) =>
    item.kind !== 'cli' && item.status === 'installed_vetted').length;
  if (creditedDeclarations >= 200) {
    throw new Error('Independent strict skill revalidation required before advancing release gate');
  }
  const roleProbe = execFileSync(process.execPath,
    [join(root, 'catalog', 'build-roles.mjs'), '--check'],
    {cwd: root, windowsHide: true, timeout: 12000, stdio: ['ignore', 'pipe', 'pipe']});
  const mandatory = new Map([
    ['clean_git_worktree', !environment.statusDirty],
    ['full_source_commit', /^[a-f0-9]{40}$/.test(head)],
    ['configured_git_remote', environment.remotePresent],
    ['versioned_rust_lockfiles', !environment.lockfileGap],
    ['committed_source_completeness',
      !status.split(/\r?\n/).some((line) =>
        /(?:crates|apps|catalog|skills-manifest|tests|\.github)[\\/]/.test(line))],
    ['required_role_definitions', roleProbe !== null],
    ['required_installed_exercised_integrations', false],
    ['master_acceptance_ledger',
      !acceptance.includes('All 188 platform rows below remain UNVERIFIED')],
    ['hosted_ci_signatures_and_public_release', false],
  ]);
  const gates = [...mandatory].map(([name, passed]) => ({name, passed}));
  const failed_count = gates.filter((item) => !item.passed).length;
  return {
    schema: 'constellation.local_readiness_probe.v1',
    read_only: true, release_ready: failed_count === 0, failed_count, gates,
  };
}

export function runTwentyFifth(candidate = name, overrides = {}) {
  if (candidate !== name) throw new Error('Not an independently approved skill');
  const manifest = JSON.parse(readFileSync(join(here, 'inventory.json')));
  const entry = manifest.entries.find((item) => item.id === `skill:${candidate}`);
  if (!entry || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
    throw new Error('Candidate lacks source-reviewed identity');
  }
  const revision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const dirty = execFileSync('git', ['-C', upstream, 'status', '--porcelain'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (dirty || !/^[a-f0-9]{40}$/.test(revision)) throw new Error('Dirty or unpinned upstream');
  const source = readFileSync(join(upstream, upstreamFile));
  const gitSource = execFileSync('git',
    ['-C', upstream, 'show', `${revision}:${upstreamFile}`], {windowsHide: true});
  const license = readFileSync(join(upstream, 'LICENSE'));
  const gitLicense = execFileSync('git',
    ['-C', upstream, 'show', `${revision}:LICENSE`], {windowsHide: true});
  if (sha(source) !== entry.source.sha256 || sha(normalize(source)) !== sha(gitSource)
    || sha(normalize(license)) !== sha(gitLicense)
    || !license.toString('utf8').includes('MIT License')
    || !source.toString('utf8').includes('# Safety restrictions')
    || !source.toString('utf8').includes('# Completion criteria')) {
    throw new Error('Pinned Git source or MIT license mismatch');
  }
  const managed = join(process.env.APPDATA ?? '', 'chat-on-steroids',
    'skills', 'constellation', candidate, 'SKILL.md');
  let installed = null;
  try { if (sha(readFileSync(managed)) === sha(source)) installed = sha(source); }
  catch { /* Source presence alone never qualifies. */ }
  const environment = overrides.environment ?? currentGitEvidence();
  const gate = overrides.report ?? runGate(environment);
  const readiness = inspectReadiness(gate, environment);
  const inputs = Object.fromEntries(watched.map((path) =>
    [path, {path, sha256: sha(readFileSync(join(root, path)))}]));
  const runnerSHA = sha(readFileSync(fileURLToPath(import.meta.url)));
  return {
    id: `skill:${candidate}`,
    status: installed ? 'installed_exercised_pending_manifest_verification'
      : 'source_reviewed_uninstalled_not_qualified',
    project: 'Constellation', qualification_credit: 0,
    command: `node skills-manifest/twenty-fifth-exercise.mjs ${candidate} --write-receipt`,
    scope: 'real_independent_local_git_lock_and_operational_readiness_audit',
    purpose: 'Independently probe current Constellation release prerequisites, classify verified blockers and reject forged readiness conclusions',
    checks: readiness.checks,
    outcome: readiness.checks.every((item) => item.result === 'pass') ? 'passed' : 'failed',
    observations: {release_ready: gate.release_ready, failed_gates: readiness.blocked,
      source_state: environment, executable_preflight_status:
        'separately_executed_in_qa_not_nested_in_strict_validator',
      clean_install_startup: 'not_executed_current_source_unfrozen',
      health_and_observability: 'unverified_in_running_production_mode',
      backup_restore_smoke: 'not_executed_in_this_exercise',
      hosted_ci_and_remote_publication: 'not_executed_no_remote_attestation'},
    limitations: [
      'The audit correctly refuses release; it does not demonstrate a clean installed application start, production health endpoint, backup restore, or live external service',
      'Hosted CI, signatures, public installers and actual provider/MCP interactions remain unverified',
      'Current Git index/status and independently inspected source facts can change while workers edit; refresh only at a stable snapshot',
      'Real PowerShell release preflight is independently recorded in QA; the trusted runner intentionally avoids calling the preflight because that calls this validator recursively',
      'Publisher identity is unverified beyond the clean pinned local Git checkout',
    ],
    upstream: {revision, path: `../vibe-coding-skills/${upstreamFile}`,
      sha256: sha(source), pinned_blob_sha256: sha(gitSource),
      license_sha256: sha(license), license_pinned_sha256: sha(gitLicense),
      publisher_identity: 'unverified'},
    inputs, installed_source_sha256: installed, exercise_script_sha256: runnerSHA,
    runner: {path: 'skills-manifest/twenty-fifth-exercise.mjs', sha256: runnerSHA},
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [candidate, action] = process.argv.slice(2);
  if (candidate !== name || !['--check', '--write-receipt'].includes(action)
    || process.argv.length !== 4) {
    process.stderr.write('Usage: twenty-fifth-exercise.mjs production-readiness-auditor --check|--write-receipt\n');
    process.exitCode = 2;
  } else {
    const report = runTwentyFifth(candidate);
    if (report.outcome !== 'passed') {
      process.stderr.write(JSON.stringify(report.checks.filter((item) => item.result === 'fail')) + '\n');
      process.exitCode = 1;
    } else if (action === '--write-receipt') {
      if (!report.installed_source_sha256) throw new Error('No verified installed skill');
      const path = join(here, 'evidence', 'twenty-fifth', candidate + '.json');
      mkdirSync(dirname(path), {recursive: true});
      writeFileSync(path, JSON.stringify({...report,
        observed_on: new Date().toISOString().slice(0, 10)}, null, 2) + '\n');
      process.stdout.write(`${relative(root, path)} sha256=${sha(readFileSync(path))}\n`);
    } else {
      process.stdout.write(`${report.id}: ${report.checks.length} real checks pass; release remains blocked\n`);
    }
  }
}
