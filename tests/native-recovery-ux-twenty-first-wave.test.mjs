import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import { runInNewContext } from 'node:vm';

// Exercise the ACTUAL TypeScript validator embedded in the Svelte route, not
// an independently rewritten fixture or a mocked native command.
const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
const styles = readFileSync(new URL('../apps/desktop/src/routes/studio.css', import.meta.url), 'utf8');
const requireDesktop = createRequire(new URL('../apps/desktop/package.json', import.meta.url));
const typescript = requireDesktop('typescript');
const start = page.indexOf('  function validateModelRecovery(');
const end = page.indexOf('  async function prepareFileTool(', start);
assert.ok(start > 0 && end > start, 'real recovery validator must be present');
const source = page.slice(start, end);
const transpiled = typescript.transpileModule(source, {
  compilerOptions: { target: typescript.ScriptTarget.ES2022, module: typescript.ModuleKind.None }
}).outputText;
const validate = runInNewContext(transpiled + '\nvalidateModelRecovery;', {}, { timeout: 1000 });
const id = '0dcbebdd-ec9f-4cad-bdf7-22eb87963aa4';
const taskId = 'c6a34ff2-835b-469b-a8d2-b158a3b8e14a';
const operation = { operation_id: id, status: 'completed', checkpoint_revision: 2, outcome: 'succeeded' };
const valid = () => ({ task_id: taskId, original_provider_id: null, automatic_resume_allowed: false, native_approval_restored: false, operations: [{ ...operation }] });

test('recovery only accepts explicit signed-provider identity metadata, never a forged route', () => {
  const providerId = '05ea83e1-e6cd-409f-a548-0eb43b5a7881';
  assert.equal(validate({ ...valid(), original_provider_id: providerId }, taskId).original_provider_id, providerId);
  assert.equal(validate(valid(), taskId).original_provider_id, null);
  for (const original_provider_id of [undefined, '', 'changed-provider', '<script>alert(1)</script>', {}, 123, false]) {
    assert.throws(() => validate({ ...valid(), original_provider_id }, taskId));
  }
  assert.match(page, /const providerId = recoveryInspections\[task\.id\]\?\.original_provider_id/);
  assert.match(page, /!recoveryInspections\[task\.id\]\.original_provider_id/);
});

test('authenticated read-only recovery response preserves five explicit journal statuses and the empty case', () => {
  for (const status of ['not_in_journal', 'approved_not_started', 'denied', 'uncertain', 'completed']) {
    const value = valid();
    value.operations[0].status = status;
    value.operations[0].outcome = status === 'completed' ? 'failed' : null;
    const parsed = validate(value, taskId);
    assert.equal(parsed.operations[0].status, status);
    assert.equal(parsed.automatic_resume_allowed, false);
    assert.equal(parsed.native_approval_restored, false);
  }
  assert.equal(validate({ ...valid(), operations: [] }, taskId).operations.length, 0);
});

test('a forged task identity or attempted permission restoration is rejected', () => {
  for (const change of [
    { task_id: 'different-task' }, { automatic_resume_allowed: true },
    { native_approval_restored: true }, { operations: {} },
    { operations: Array.from({ length: 9 }, (_, n) => ({ ...operation, operation_id: String(n) })) },
  ]) {
    assert.throws(() => validate({ ...valid(), ...change }, taskId));
  }
});

test('reject duplicate operations, malformed IDs, missing/oversized revision and unexpected outcome', () => {
  for (const change of [
    { operations: [operation, operation] },
    { operations: [{ ...operation, operation_id: '<script>alert(1)</script>' }] },
    { operations: [{ ...operation, checkpoint_revision: 0 }] },
    { operations: [{ ...operation, checkpoint_revision: 65 }] },
    { operations: [{ ...operation, checkpoint_revision: '2' }] },
    { operations: [{ ...operation, outcome: 'executed_and_approved' }] },
    { operations: [{ ...operation, status: 'permission_granted' }] },
    { operations: [{ ...operation, status: 'uncertain', outcome: 'succeeded' }] },
    { operations: [{ ...operation, status: 'completed', outcome: null }] },
  ]) assert.throws(() => validate({ ...valid(), ...change }, taskId));
});

test('stale UI responses are ignored and no status is displayed while a new audit is pending', () => {
  assert.match(page, /const requestSequence = \+\+recoveryInspectionSequence/);
  assert.match(page, /delete previous\[task\.id\];\s*recoveryInspections = previous/);
  assert.match(page, /requestSequence !== recoveryInspectionSequence \|\|\s*!snapshot\.tasks\.some\(\(item\) => item\.id === task\.id && item\.state === 'pending'\)/);
  assert.match(page, /recoveryInspectionTaskId === task\.id/);
  assert.match(page, /aria-busy=\{recoveryInspectionTaskId === task\.id\}/);
  assert.match(page, /role="alert">\{recoveryInspectionErrors\[task\.id\]\}/);
});

test('recovery is visibly read-only, keyboard reachable, safe for narrow screens, and never an execution control', () => {
  assert.match(page, /No former native approval is restored\. No automatic resume or replay is allowed/);
  assert.match(page, /Do not retry it automatically/);
  assert.match(page, /operation may have changed files before interruption/);
  assert.match(page, /aria-controls=\{'recovery-evidence-' \+ task\.id\}/);
  assert.match(styles, /\.recovery-history code \{[^}]*overflow-wrap:anywhere/);
  assert.match(styles, /\.recovery-heading \{[^}]*flex-wrap:wrap/);
  assert.doesNotMatch(source, /invoke\s*\(\s*['"]studio_(approve|execute|retry)_/);
});
