#!/usr/bin/env node
// Independently reviewed, read-only source-contract exercises for four distinct
// pinned skill definitions. Tests concrete Constellation contracts and adverse
// mutations; never starts a server, executes upstream instructions or grants OS
// permissions. Receipts alone never create qualification credit.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstream = resolve(root, '..', 'vibe-coding-skills');
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const normalized = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const specs = Object.freeze({
  'multi-agent-integration-reviewer': {
    category: 'agent-reliability',
    inputs: [
      'crates/constellation-core/src/permissions.rs',
      'apps/desktop/src-tauri/src/tool_host.rs',
      'crates/constellation-core/tests/security.rs',
    ],
  },
  'environment-parity-auditor': {
    category: 'deployment',
    inputs: [
      'apps/desktop/package.json', 'apps/desktop/pnpm-lock.yaml',
      '.github/workflows/validate.yml', '.github/workflows/acceptance.yml',
    ],
  },
  'authentication-security-auditor': {
    category: 'security',
    inputs: [
      'apps/desktop/src-tauri/src/native_consent.rs',
      'apps/desktop/src-tauri/src/tool_host.rs',
      'crates/constellation-core/src/permissions.rs',
    ],
  },
  'regression-detector': {
    category: 'testing',
    inputs: [
      'crates/constellation-core/src/permissions.rs',
      'crates/constellation-core/tests/security.rs',
      'crates/constellation-tools/tests/authenticated_journal.rs',
    ],
  },
});
export const eligibleEighteenth = Object.freeze(Object.keys(specs));
const check = (name, ok, evidence) => ({name, result: ok ? 'pass' : 'fail', evidence});

function permissionFields(source) {
  const block = source.match(/pub struct PermissionScope\s*\{([^}]+)\}/)?.[1] ?? '';
  return [...block.matchAll(/pub\s+(\w+)\s*:/g)].map((match) => match[1]);
}

/** Checks a concrete shared native/core contract, not just file overlap. */
export function integrationReview(corePermissions, toolHost, coreTests) {
  const fields = permissionFields(corePermissions);
  const required = ['agent_id', 'room_id', 'task_id', 'action', 'resource'];
  const crossCrate = /use constellation_core::\{[^}]*OperationId[^}]*PermissionGate/.test(toolHost)
    && /use constellation_tools::\{/.test(toolHost);
  const binding = /native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/.test(toolHost);
  const matrix = (fileOwners) => {
    const rows = new Map();
    for (const [owner, files] of fileOwners) for (const path of files) {
      rows.set(path, [...(rows.get(path) ?? []), owner]);
    }
    return [...rows].filter(([, owners]) => new Set(owners).size > 1).map(([path]) => path);
  };
  const collision = matrix([
    ['core-worker', ['crates/constellation-core/src/permissions.rs']],
    ['native-worker', ['crates/constellation-core/src/permissions.rs', 'apps/desktop/src-tauri/src/lib.rs']],
  ]);
  const distinct = matrix([
    ['core-worker', ['crates/constellation-core/src/permissions.rs']],
    ['native-worker', ['apps/desktop/src-tauri/src/tool_host.rs']],
  ]);
  return {
    purpose: 'Audit the real native/Core tool-permission integration contract and detect overlapping ownership plus semantic signature drift',
    checks: [
      check('Core permission schema has complete five-part identity', required.every((item) => fields.includes(item)),
        'PermissionScope public field names'),
      check('Native tool host actually links the Core and Tools interfaces', crossCrate, 'real Rust imports'),
      check('Native consent compares operation ID and private prepared digest', binding, 'trusted dispatcher call'),
      check('Core test exercises exact-task denial on grant reuse',
        /other\.task_id\s*=\s*TaskId::new\(\)/.test(coreTests)
        && /no_actions_execute_without_scoped_explicit_permission/.test(coreTests), 'core security.rs tests'),
      check('Overlapping worker ownership is detected', collision.length === 1
        && collision[0] === 'crates/constellation-core/src/permissions.rs', 'adverse two-worker ownership fixture'),
      check('Independent worker scopes remain separate', distinct.length === 0, 'non-overlapping fixture'),
      check('Removed native digest bind invalidates semantic integration',
        binding && !/native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/
          .test(toolHost.replace(
            /native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/,
            'native_proof.verify(&operation_id.to_string(), \"unbound\")',
          )), 'independent mutation fixture'),
    ],
    limitations: ['Performed read-only module-contract and ownership exercises, not actual branch integration or merging',
      'The native/Core combined build and runtime remain separately validated by project CI'],
  };
}

function workflowVersions(text) {
  return [...text.matchAll(/node-version:\s*['"]?([0-9]+(?:\.[0-9]+){0,2})/g)].map((m) => m[1]);
}
/** Validates versions and frozen dependency use across two real CI workflows. */
export function environmentParity(pkgText, lock, validate, acceptance) {
  const pkg = JSON.parse(pkgText);
  const variants = [validate, acceptance];
  const nodeVersions = variants.flatMap(workflowVersions);
  const major = (value) => Number(value.split('.')[0]);
  const importer = lock.split('\npackages:')[0];
  const declared = Object.keys({...pkg.dependencies, ...pkg.devDependencies});
  const resolutionMissing = declared.filter((name) => !importer.includes(
    `      ${name.startsWith('@') ? `'${name}'` : name}:`,
  ));
  const expectedNode = '24.17.0';
  const pin = (value) => workflowVersions(value).every((v) => v === expectedNode);
  const pinnedInstall = (value) => /pnpm install --frozen-lockfile/.test(value);
  const lockMutation = importer.replace('      svelte:', '      unrelated:');
  return {
    purpose: 'Independently compare actual desktop dependency/importer and Node/pnpm CI environments with mutation checks',
    checks: [
      check('Both real CI workflows pin an expected Node 24 runtime',
        nodeVersions.length >= 3 && nodeVersions.every((v) => major(v) === 24 && v === expectedNode),
        `${nodeVersions.length} pinned Node references`),
      check('Both workflows use frozen pnpm installs', variants.every(pinnedInstall),
        'validate.yml and acceptance.yml'),
      check('Every declared desktop dependency has a lockfile importer', declared.length >= 8
        && resolutionMissing.length === 0, `${declared.length} declared dependencies`),
      check('Production build and type checks remain in both workflows',
        variants.every((v) => /pnpm run check/.test(v) && /pnpm run build/.test(v)), 'CI build/check commands'),
      check('Node version mismatch is rejected', !pin(validate.replace(expectedNode, '23.0.0'))
        && pin(validate), 'synthetic stale-node workflow'),
      check('Unfrozen dependency install is rejected',
        !pinnedInstall(acceptance.replaceAll('pnpm install --frozen-lockfile', 'pnpm install')), 'negative lock mode'),
      check('Dropped lock resolution produces dependency parity failure',
        importer.includes('      svelte:') && !lockMutation.includes('      svelte:'),
        'synthetic missing Svelte importer resolution'),
    ],
    limitations: ['Analyzes actual workflow pins and lockfile imports; no clean container, Linux/macOS launch, installer startup or hosted CI was run',
      'The presence of lock resolutions does not guarantee dependency security or a reproducible binary'],
  };
}

/** Audits actual native consent proof and default-deny dispatch boundaries. */
export function authenticationReview(consent, host, corePermissions) {
  const proof = consent.match(/pub\(crate\) struct NativeConsentProof\s*\{([^}]+)\}/)?.[1] ?? '';
  const verify = consent.match(/pub\(crate\) fn verify\([^)]*\) -> bool\s*\{([^}]+)\}/)?.[1] ?? '';
  const both = /self\.operation_id\s*==\s*operation_id/.test(verify)
    && /self\.operation_digest\s*==\s*operation_digest/.test(verify)
    && /&&/.test(verify);
  const noPublicProductionConstructor = !/pub\s+fn\s+new\s*\(/.test(consent.slice(
    consent.indexOf('pub(crate) struct NativeConsentProof'),
    consent.indexOf('pub(crate) enum NativeConsentError'),
  ));
  const journalFail = /Stored audit key is invalid; action denied/.test(host)
    && /Authenticated audit key is unavailable; action denied/.test(host);
  const denyByDefault = /grants:\s*HashSet<PermissionScope>/.test(corePermissions)
    && /Err\(CoreError::PermissionDenied\)/.test(corePermissions);
  return {
    purpose: 'Audit Constellation native consent identity/digest proof and OS-vault fail-closed action gate, with privilege-bypass mutations',
    checks: [
      check('Native proof fields are both private and exact', /operation_id:\s*String/.test(proof)
        && /operation_digest:\s*String/.test(proof) && !/pub\s+operation_/.test(proof), 'native Rust proof struct'),
      check('Native proof verifies immutable ID and digest jointly', both, 'native_consent::verify'),
      check('Native proof has no public runtime constructor', noPublicProductionConstructor
        && /#\[cfg\(test\)\]/.test(consent), 'non-constructible production proof'),
      check('Dispatcher verifies proof against current private pending operation',
        /native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/.test(host),
        'tool_host dispatch proof binding'),
      check('Bad/missing OS vault key fails closed', journalFail, 'native tool audit key errors'),
      check('Core grant semantics remain exact and default deny', denyByDefault,
        'PermissionGate HashSet and denied error'),
      check('Removing digest from proof check is a detected regression',
        both && !/self\.operation_digest\s*==\s*operation_digest/
          .test(verify.replace(/self\.operation_digest\s*==\s*operation_digest/, 'true')),
        'synthetic weak-proof mutation'),
    ],
    limitations: ['This is native consent proof auditing; login, OAuth, issuer, cookie and online identity-provider behavior are not present in this scope',
      'No Windows physical consent interaction or OS credential-vault execution was performed in this audit'],
  };
}

/** Baselines exact core policy behavior and tests controlled adverse diffs. */
export function regressionReview(permissions, coreTests, journalTests) {
  const sourceGuard = /self\.grants\.contains\(request\)/.test(permissions)
    && /Err\(CoreError::PermissionDenied\)/.test(permissions);
  const mutated = permissions.replace('self.grants.contains(request)', 'true');
  const scopeDimensions = ['task_id', 'agent_id', 'room_id', 'action', 'resource'];
  const covered = scopeDimensions.filter((dimension) =>
    new RegExp(`other\\.${dimension}\\s*=`).test(coreTests));
  const tamperChecks = [
    'valid_json_tampering_and_missing_middle_record_fail_closed',
    'copied_authenticated_log_cannot_be_transplanted_to_another_journal_path',
    'denial_survives_restart_and_cannot_be_forged_into_approval',
  ];
  const genuineTests = tamperChecks.every((item) => journalTests.includes(`fn ${item}()`));
  const reduced = journalTests.replace(
    'fn denial_survives_restart_and_cannot_be_forged_into_approval()',
    'fn removed_replay_test()',
  );
  return {
    purpose: 'Independently compare live permission gate behavior contract with five existing cross-scope tests and signed-journal regression cases',
    checks: [
      check('PermissionGate uses an exact membership check and explicit denial', sourceGuard,
        'real core permissions implementation'),
      check('All five permission dimensions have adverse unit-test assignments',
        covered.length === scopeDimensions.length, `${covered.length}/5 independent dimensions`),
      check('Core denies permission on the fresh ungranted request',
        /gate\.authorize\(&request\)/.test(coreTests)
        && /Err\(CoreError::PermissionDenied\)/.test(coreTests), 'test no_actions_execute_without_scoped_explicit_permission'),
      check('Authenticated journal has replay/tamper/transplant regression tests', genuineTests,
        'real journal integration test cases'),
      check('A broadened always-grant implementation is flagged',
        sourceGuard && !/self\.grants\.contains\(request\)/.test(mutated),
        'synthetic permission bypass mutation'),
      check('Removal of the durable denial test is flagged',
        genuineTests && !tamperChecks.every((item) => reduced.includes(`fn ${item}()`)),
        'synthetic missing anti-replay test'),
    ],
    limitations: ['This checks real source/test contracts and controlled adverse mutations; it does not run cargo or compare a baseline binary',
      'After any production patch, run the exact relevant Rust tests and cross-platform CI before declaring zero regressions'],
  };
}

const reviewers = Object.freeze({
  'multi-agent-integration-reviewer': integrationReview,
  'environment-parity-auditor': environmentParity,
  'authentication-security-auditor': authenticationReview,
  'regression-detector': regressionReview,
});

export function runEighteenth(name, overrides = {}) {
  const spec = specs[name];
  if (!spec) throw new Error('Unsupported/unreviewed eighteenth-wave skill');
  const inventory = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
  const entry = inventory.entries.find((item) => item.id === `skill:${name}`);
  if (!entry || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
    throw new Error('Skill lacks a reviewed source identity');
  }
  const revision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const status = execFileSync('git', ['-C', upstream, 'status', '--porcelain'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (status || !/^[a-f0-9]{40}$/.test(revision)) throw new Error('Unpinned or dirty upstream');
  const sourcePath = `skills/${spec.category}/${name}/SKILL.md`;
  const source = readFileSync(join(upstream, sourcePath));
  const pinned = execFileSync('git', ['-C', upstream, 'show', `${revision}:${sourcePath}`],
    {windowsHide: true});
  const license = readFileSync(join(upstream, 'LICENSE'));
  const licensePinned = execFileSync('git', ['-C', upstream, 'show', `${revision}:LICENSE`],
    {windowsHide: true});
  if (hash(source) !== entry.source.sha256
    || hash(normalized(source)) !== hash(pinned)
    || hash(normalized(license)) !== hash(licensePinned)
    || entry.license.spdx !== 'MIT'
    || !license.toString('utf8').includes('MIT License')
    || !normalized(source).toString('utf8').startsWith('---\nname: ' + name + '\n')
    || !source.toString('utf8').includes('# Safety restrictions')
    || !source.toString('utf8').includes('# Completion criteria')) {
    throw new Error('Pinned source/license/frontmatter/safety validation failed');
  }
  const managed = join(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation',
    name, 'SKILL.md');
  let installed = null;
  try {
    if (hash(readFileSync(managed)) === hash(source)) installed = hash(source);
  } catch { /* An absent install has zero qualification credit. */ }
  const inputs = Object.fromEntries(spec.inputs.map((path) => [path, {
    path, sha256: hash(readFileSync(resolve(root, path))),
  }]));
  const texts = spec.inputs.map((path) => overrides[path]
    ?? readFileSync(resolve(root, path), 'utf8'));
  const audit = reviewers[name](...texts);
  const runnerSha = hash(readFileSync(fileURLToPath(import.meta.url)));
  return {
    id: `skill:${name}`,
    status: installed ? 'installed_exercised_pending_manifest_verification'
      : 'source_reviewed_uninstalled_not_qualified',
    project: 'Constellation',
    qualification_credit: 0,
    command: `node skills-manifest/eighteenth-exercise.mjs ${name} --write-receipt`,
    scope: 'bounded_independent_source_contract_and_adverse_mutations',
    purpose: audit.purpose,
    checks: audit.checks,
    outcome: audit.checks.every((item) => item.result === 'pass') ? 'passed' : 'failed',
    limitations: [...audit.limitations,
      'No runtime MCP integration is provided or claimed by this local development skill',
      'Publisher GitHub identity not authenticated beyond observed clean pinned local Git repository'],
    upstream: {
      revision,
      path: `../vibe-coding-skills/${sourcePath}`,
      sha256: hash(source),
      pinned_blob_sha256: hash(pinned),
      license_sha256: hash(license),
      license_pinned_sha256: hash(licensePinned),
      publisher_identity: 'unverified',
    },
    inputs,
    installed_source_sha256: installed,
    exercise_script_sha256: runnerSha,
    runner: {path: 'skills-manifest/eighteenth-exercise.mjs', sha256: runnerSha},
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [name, flag] = process.argv.slice(2);
  if (!eligibleEighteenth.includes(name)
    || !['--check', '--write-receipt'].includes(flag) || process.argv.length !== 4) {
    process.stderr.write('Usage: eighteenth-exercise.mjs <reviewed-name> --check|--write-receipt\n');
    process.exitCode = 2;
  } else {
    const audit = runEighteenth(name);
    if (audit.outcome !== 'passed') {
      process.stderr.write(JSON.stringify(audit.checks.filter((item) => item.result === 'fail'), null, 2) + '\n');
      process.exitCode = 1;
    } else if (flag === '--write-receipt') {
      if (!audit.installed_source_sha256) throw new Error('No exact managed installation');
      const destination = join(here, 'evidence', 'eighteenth', name + '.json');
      mkdirSync(dirname(destination), {recursive: true});
      writeFileSync(destination, JSON.stringify({...audit,
        observed_on: new Date().toISOString().slice(0, 10)}, null, 2) + '\n');
      process.stdout.write(`${relative(root, destination)} SHA256=${hash(readFileSync(destination))} checks=${audit.checks.length}\n`);
    } else {
      process.stdout.write(`${name}: ${audit.checks.length} checks passing, qualification pending\n`);
    }
  }
}
