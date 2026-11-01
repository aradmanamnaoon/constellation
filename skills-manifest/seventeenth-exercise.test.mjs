import assert from 'node:assert/strict';
import test from 'node:test';
import {
  approvedSkills, runSeventeenth,
  preservationAudit, architectureAudit, featureAudit, missingTestsAudit,
} from './seventeenth-exercise.mjs';

test('all four reviewed candidate exercises replay on the current actual Constellation source', () => {
  assert.equal(approvedSkills.length, 4);
  for (const name of approvedSkills) {
    const result = runSeventeenth(name);
    assert.equal(result.outcome, 'passed', name);
    assert.ok(result.installed_source_sha256, name);
    assert.ok(result.checks.length >= 5, name);
    assert.equal(result.qualification_credit, 0);
    assert.equal(result.upstream.publisher_identity, 'unverified');
  }
});

test('preservation exercise rejects missing policy and detects path conflicts', () => {
  const result = preservationAudit('', '');
  assert.equal(result.outcome, undefined);
  assert.equal(result.checks[0].result, 'fail');
  assert.equal(result.checks[1].result, 'fail');
  assert.equal(result.checks.find((check) => check.name.includes('overlapping')).result, 'pass');
});

test('architecture exercise rejects missing real module responsibilities', () => {
  const result = architectureAudit('fn create_room() {}', 'pub mod store;', '');
  assert.equal(result.checks[0].result, 'fail');
  assert.equal(result.checks.find((check) => check.name.includes('Native')).result, 'fail');
});

test('dead feature detector rejects orphan and unregistered UI commands', () => {
  const result = featureAudit('invoke("studio_dead_button");', 'fn studio_other() {}');
  assert.equal(result.checks.find((check) => check.name.includes('matching handler')).result, 'fail');
  assert.equal(result.checks.find((check) => check.name.includes('registered')).result, 'fail');
});

test('split native Tauri commands require a real declared module and qualified registration', () => {
  const name = 'studio_native_multi_step_next';
  const ui = 'await invoke("' + name + '", {})';
  const module = '#[tauri::command]\npub(crate) async fn ' + name + '() {}';
  const root = 'mod native_multi_step_runtime;\n'
    + 'tauri::generate_handler![native_multi_step_runtime::' + name + ']';
  const audit = (r, m) => featureAudit(ui, r, m).checks;
  const handler = (checks) => checks.find((row) =>
    row.name.includes('matching handler')).result;
  const linked = (checks) => checks.find((row) =>
    row.name.startsWith('Separate-module commands')).result;
  assert.equal(handler(audit(root, module)), 'pass');
  assert.equal(linked(audit(root, module)), 'pass');
  for (const [source, implementation] of [
    [root.replace('mod native_multi_step_runtime;', ''), module],
    [root.replace('native_multi_step_runtime::', ''), module],
    [root, module.replace('#[tauri::command]', '')],
    [root, module.replace(name, 'studio_wrong_name')],
    [root, ''],
  ]) {
    assert.equal(handler(audit(source, implementation)), 'fail');
    assert.equal(linked(audit(source, implementation)), 'fail');
  }
});

test('missing tests detection fails when tests and adverse assertions are absent', () => {
  const result = missingTestsAudit('pub fn journal_state() {}', '', '');
  assert.equal(result.checks[0].result, 'fail');
  assert.equal(result.checks[1].result, 'fail');
  assert.equal(result.checks.find((check) => check.name.includes('fail-closed')).result, 'fail');
});

test('an unapproved skill has no evaluator and earns no fabricated credit', () => {
  assert.throws(() => runSeventeenth('ui-flow-verifier'), /Unreviewed/);
});
