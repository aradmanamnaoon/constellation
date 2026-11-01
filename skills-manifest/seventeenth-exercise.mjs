#!/usr/bin/env node
// Four distinct, bounded local exercises for individually reviewed skills.
// No execution of upstream SKILL.md instructions; no network, app writes or MCP.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const upstream = resolve(repo, '..', 'vibe-coding-skills');
const pathOf = (part) => resolve(repo, part);
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const norm = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const skillSources = Object.freeze({
  'existing-work-preservation': ['agent-reliability', ['AGENTS.md', 'docs/mission/WORKER_ASSIGNMENTS.md']],
  'architecture-complexity-auditor': ['architecture', [
    'crates/constellation-core/src/store.rs', 'crates/constellation-core/src/lib.rs',
    'apps/desktop/src-tauri/src/lib.rs']],
  'dead-feature-detector': ['frontend', [
    'apps/desktop/src/routes/+page.svelte',
    'apps/desktop/src-tauri/src/lib.rs',
    'apps/desktop/src-tauri/src/native_multi_step_runtime.rs']],
  'missing-test-detector': ['testing', [
    'crates/constellation-tools/src/execution.rs',
    'crates/constellation-tools/tests/journal_operation_state.rs',
    'crates/constellation-tools/tests/authenticated_journal.rs']],
});
export const approvedSkills = Object.freeze(Object.keys(skillSources));
const pass = (name, condition, evidence) => ({name, result: condition ? 'pass' : 'fail', evidence});

// These read-only tests use the actual app source and independent adverse fixtures.
export function preservationAudit(instructions, assignments) {
  const forbidden = /(?:\bgit\s+(?:reset\s+--hard|clean\s+-fdx)|\bforce.push\b|\bmass.reformat\b)/i;
  const ownership = /own|scope|worker/i.test(assignments);
  const policies = /Preserve repo\/history without force-push or destructive/.test(instructions)
    && /Do not change sibling repositories/.test(instructions);
  const dirtySample = [' M apps/desktop/src/routes/+page.svelte', '?? crates/constellation-core/src/new.rs'];
  const inventory = (lines) => lines.map((line) => ({
    state: line.slice(0, 2),
    path: line.slice(3),
  }));
  const parsed = inventory(dirtySample);
  const proposedPaths = ['skills-manifest/seventeenth-exercise.mjs'];
  const conflicts = (paths, existing) => paths.some((file) =>
    existing.some((entry) => entry.path === file));
  return {
    purpose: 'Verify actual Constellation preservation policy, worker ownership, and a bounded dirty-tree conflict decision',
    checks: [
      pass('Project explicitly protects existing working changes', policies, 'AGENTS.md preservation rules'),
      pass('Live worker assignments specify ownership', ownership, 'Worker assignment guidance'),
      pass('Dirty-tree parser preserves modified and untracked distinctions', parsed.length === 2
        && parsed[0].state === ' M' && parsed[1].state === '??', 'two synthetic git porcelain records'),
      pass('An owned path does not collide with unrelated dirty files', !conflicts(proposedPaths, parsed), 'owned path isolation'),
      pass('An overlapping change is caught before writing', conflicts([parsed[0].path], parsed), 'adverse overlap fixture'),
      pass('Destructive command signals are detected', forbidden.test('git reset --hard')
        && forbidden.test('git clean -fdx'), 'adverse command fixture'),
    ],
    limitations: ['Policy and deterministic conflict exercise; did not modify or restore the actual working tree',
      'Git baseline change history and behavior regression verification require each real edit task'],
  };
}

export function architectureAudit(store, coreLib, nativeLib) {
  const functions = [...store.matchAll(/^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+([\w]+)\s*\(/gm)]
    .map((m) => m[1]);
  const unique = new Set(functions);
  const storeIsUsed = /\bmod\s+store\s*;/.test(coreLib)
    || /\bpub\s+mod\s+store\s*;/.test(coreLib);
  const tests = /(?:#\[cfg\(test\)\]|mod\s+tests)/.test(store)
    || /(?:pub\s+mod\s+store\s*;|mod\s+store\s*;)/.test(coreLib);
  const bounded = (file) => ({
    lines: file.split(/\r?\n/).length,
    distinctFunctions: new Set([...file.matchAll(/^\s*(?:pub\s+)?fn\s+(\w+)\s*\(/gm)]
      .map((m) => m[1])).size,
  });
  const signal = bounded(store);
  const artificial = bounded('fn save() {}\nfn restore() {}\nfn save() {}');
  return {
    purpose: 'Measure real Rust core store responsibilities and dependency evidence without asserting file length alone mandates refactoring',
    checks: [
      pass('Actual production store has multiple independent operation families',
        ['create_room', 'append_message', 'create_task', 'transition_task']
          .every((name) => unique.has(name)), 'room/message/task methods'),
      pass('Core public module graph includes the store', storeIsUsed, 'core lib.rs module declaration'),
      pass('Function names are deduplicated for complexity evidence',
        artificial.distinctFunctions === 2, 'duplicate-name negative fixture'),
      pass('Removal of a required responsibility invalidates the evidence',
        !['create_room', 'append_message', 'create_task', 'transition_task']
          .every((name) => new Set([...store.replace(/\bpub fn create_task\b/, 'pub fn removed_task')
            .matchAll(/^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+([\w]+)\s*\(/gm)]
            .map((m) => m[1])).has(name)), 'production source mutation fixture'),
      pass('Line count and function diversity are separately measured',
        signal.lines > 100 && unique.size >= 10, 'real store source metrics'),
      pass('Native application consumes the core API',
        /constellation_core::/.test(nativeLib), 'native-to-core dependency edge'),
      pass('Refactor evaluation can locate test ownership before change', tests, 'core module/test contract'),
    ],
    limitations: ['Static fan-out and responsibility signals only; no claim that a refactor was performed',
      'Dynamic dispatch, hidden dependencies, and actual change-history coupling are outside this bounded scan'],
  };
}

export function featureAudit(ui, native, nativeMultiStep = '') {
  const invoked = [...new Set([...ui.matchAll(/\binvoke(?:<[^>]*>)?\(\s*['"](studio_[\w]+)['"]/g)]
    .map((m) => m[1]))];
  const registrations = native.match(/(?:tauri::)?generate_handler!\s*\[([\s\S]*?)\]/)?.[1] ?? '';
  const match = (name, source = native) => new RegExp(
    '(?:async\\s+)?fn\\s+' + name + '\\s*\\(',
  ).test(source);
  // Tauri 2 permits command handlers in a separately declared native Rust
  // module. Count only a real native module handler with a Tauri attribute
  // and exact qualified registration, not arbitrary or orphan functions.
  const linkedModule = /\bmod\s+native_multi_step_runtime\s*;/.test(native);
  const moduleHandler = (name) => linkedModule
    && new RegExp('#\\[tauri::command\\]\\s+pub\\(crate\\)\\s+(?:async\\s+)?fn\\s+'
      + name + '\\s*\\(').test(nativeMultiStep)
    && new RegExp('\\bnative_multi_step_runtime::' + name + '\\b').test(registrations);
  const missing = invoked.filter((name) => !match(name) && !moduleHandler(name));
  const unregistered = invoked.filter((name) =>
    !new RegExp('\\b' + name + '\\b').test(registrations));
  const externalCommands = invoked.filter((name) => !match(name));
  const correctlyLinked = externalCommands.every((name) => moduleHandler(name));
  const small = 'async function click(){ await invoke("studio_exercise_missing", {}); }';
  const handlerOnly = 'fn studio_other_handler() {}';
  const fakeOnly = 'fn studio_exercise_missing() {}';
  return {
    purpose: 'Trace actual Svelte invoke names to real native command implementations and reject missing-handler fixtures',
    checks: [
      pass('Production UI invokes multiple distinct native actions', invoked.length >= 8,
        `${invoked.length} unique invoke names`),
      pass('Every scanned UI native action has a matching handler definition', missing.length === 0,
        `${missing.length} missing handler names`),
      pass('Scanned UI actions are registered in native invoke handler', unregistered.length === 0,
        `${unregistered.length} unregistered command names`),
      pass('Separate-module commands require declaration, Tauri attribute and qualified registration',
        correctlyLinked, `${externalCommands.length} external native actions checked`),
      pass('Missing handler produces a negative finding', !match('studio_exercise_missing', handlerOnly)
        && /invoke\("studio_exercise_missing"/.test(small), 'orphan UI action fixture'),
      pass('Handler presence alone is not asserted as a real effect',
        match('studio_exercise_missing', fakeOnly) && !/\.execute\(|\.save\(/.test(fakeOnly),
        'no-op handler requires additional runtime verification'),
      pass('UI exposes observable failure paths', /catch\s*\(/.test(ui)
        && /error|failed|alert/i.test(ui), 'async error reporting paths'),
    ],
    limitations: ['Static UI-to-native name trace; no browser click or OS effect verified',
      'A matched native command may still be inert; end-to-end feature completion remains unproven'],
  };
}

export function missingTestsAudit(execution, recoveryTests, authTests) {
  const signatures = ['journal_state', 'journal_state_bound', 'lookup_authenticated_journal'];
  const src = signatures.every((name) =>
    new RegExp('pub\\s+fn\\s+' + name + '\\s*\\(').test(execution));
  const both = recoveryTests + '\n' + authTests;
  const cases = [
    /a_trusted_chain_head_detects_complete_suffix_removal/,
    /forged_mac_wrong_key_partial_tail_and_duplicate_events_fail_closed/,
    /lookup_live_writer_and_reopened_journal_identify_all_states_without_content/,
    /denial_survives_restart_and_cannot_be_forged_into_approval/,
  ];
  const fixtureWithGap = both.replace(
    'fn forged_mac_wrong_key_partial_tail_and_duplicate_events_fail_closed',
    'fn renamed_test_with_no_security_coverage',
  );
  return {
    purpose: 'Map actual authenticated tool recovery APIs to independent negative security regression tests, including missing-test detection',
    checks: [
      pass('New read-only recovery APIs exist', src, 'three public LocalExecutor method declarations'),
      pass('Rollback, forgery, confidentiality and one-use cases have independent tests',
        cases.every((pattern) => pattern.test(both)), 'four specifically named adverse tests'),
      pass('Removing a key adversarial test leaves an uncovered mapping',
        !cases[1].test(fixtureWithGap), 'synthetic missing-security-test mutation'),
      pass('Security tests assert fail-closed error results',
        /Err\(ToolError::InvalidJournal\)/.test(both), 'actual denial assertions'),
      pass('Successful known-key read is distinguished from wrong key',
        /\[0xa6; 32\]/.test(recoveryTests)
          && /lookup\(workspace\.path\(\)/.test(recoveryTests), 'key-rotation and success fixtures'),
    ],
    limitations: ['Checks security test presence and selected assertions, not their current execution outcome',
      'Full coverage, integration with native ToolHost and fault injection remain separately required'],
  };
}

const assessments = Object.freeze({
  'existing-work-preservation': preservationAudit,
  'architecture-complexity-auditor': architectureAudit,
  'dead-feature-detector': featureAudit,
  'missing-test-detector': missingTestsAudit,
});

export function runSeventeenth(name, overrides = {}) {
  const definition = skillSources[name];
  if (!definition) throw new Error('Unreviewed seventeenth-wave candidate');
  const [category, files] = definition;
  const manifest = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
  const entry = manifest.entries.find((item) => item.id === `skill:${name}`);
  if (!entry || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
    throw new Error('Candidate is not an approved, reviewed skill');
  }
  const relativeSource = `skills/${category}/${name}/SKILL.md`;
  const revision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (execFileSync('git', ['-C', upstream, 'status', '--porcelain'],
    {encoding: 'utf8', windowsHide: true}).trim()) throw new Error('Dirty upstream');
  if (!/^[a-f0-9]{40}$/.test(revision)) throw new Error('Invalid source revision');
  const source = readFileSync(join(upstream, relativeSource));
  const pinned = execFileSync('git', ['-C', upstream, 'show', `${revision}:${relativeSource}`],
    {windowsHide: true});
  const license = readFileSync(join(upstream, 'LICENSE'));
  const pinnedLicense = execFileSync('git', ['-C', upstream, 'show', `${revision}:LICENSE`],
    {windowsHide: true});
  if (sha(source) !== entry.source.sha256
    || sha(norm(source)) !== sha(pinned)
    || sha(norm(license)) !== sha(pinnedLicense)
    || !license.toString('utf8').includes('MIT License')
    || entry.license.status !== 'verified' || entry.license.spdx !== 'MIT') {
    throw new Error('Unverified pinned source/license');
  }
  const installed = join(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation', name, 'SKILL.md');
  let installedSha = null;
  try {
    if (sha(readFileSync(installed)) === sha(source)) installedSha = sha(source);
  } catch { /* No installation credit until exact managed copy exists. */ }
  const inputs = Object.fromEntries(files.map((path) =>
    [path, {path, sha256: sha(readFileSync(pathOf(path)))}]));
  const content = files.map((path) => overrides[path] ?? readFileSync(pathOf(path), 'utf8'));
  const assessed = assessments[name](...content);
  const runnerSha = sha(readFileSync(fileURLToPath(import.meta.url)));
  return {
    id: `skill:${name}`,
    status: installedSha ? 'installed_exercised_pending_manifest_verification' : 'source_reviewed_uninstalled_not_qualified',
    project: 'Constellation',
    qualification_credit: 0,
    command: `node skills-manifest/seventeenth-exercise.mjs ${name} --write-receipt`,
    scope: 'bounded_offline_source_and_negative_contract',
    purpose: assessed.purpose,
    checks: assessed.checks,
    outcome: assessed.checks.every((item) => item.result === 'pass') ? 'passed' : 'failed',
    limitations: [...assessed.limitations,
      'Development-only skill; no real MCP runtime or app tool authorization was installed',
      'Upstream publisher identity not independently authenticated; pinned local Git source and MIT license verified'],
    upstream: {
      revision,
      path: `../vibe-coding-skills/${relativeSource}`,
      sha256: sha(source),
      pinned_blob_sha256: sha(pinned),
      license_sha256: sha(license),
      license_pinned_sha256: sha(pinnedLicense),
      publisher_identity: 'unverified',
    },
    inputs,
    installed_source_sha256: installedSha,
    exercise_script_sha256: runnerSha,
    runner: {path: 'skills-manifest/seventeenth-exercise.mjs', sha256: runnerSha},
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [name, mode] = process.argv.slice(2);
  if (!approvedSkills.includes(name) || !['--check', '--write-receipt'].includes(mode)
    || process.argv.length !== 4) {
    process.stderr.write('Usage: seventeenth-exercise.mjs <approved-skill> --check|--write-receipt\n');
    process.exitCode = 2;
  } else {
    const result = runSeventeenth(name);
    if (result.outcome !== 'passed') {
      process.stderr.write(JSON.stringify(result.checks.filter((item) => item.result === 'fail'), null, 2));
      process.exitCode = 1;
    } else if (mode === '--write-receipt') {
      if (!result.installed_source_sha256) throw new Error('Missing exact managed skill installation');
      const destination = join(here, 'evidence', 'seventeenth', name + '.json');
      mkdirSync(dirname(destination), {recursive: true});
      writeFileSync(destination, JSON.stringify({...result, observed_on: new Date().toISOString().slice(0, 10)}, null, 2) + '\n');
      process.stdout.write(`${relative(repo, destination)} sha256=${sha(readFileSync(destination))} checks=${result.checks.length}\n`);
    } else {
      process.stdout.write(`${result.id}: ${result.checks.length} checks passed; qualification pending\n`);
    }
  }
}
