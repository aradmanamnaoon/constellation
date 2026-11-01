import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const get = (path) => readFileSync(join(root, path), 'utf8');
const plan = get('docs/mission/COMPLETE_EXECUTION_PLAN.md');
const specs = get('Codex_Master_Prompt_Local_Agent_Collaboration_Platform.md');
const roles = JSON.parse(get('catalog/roles.json')).roles;
const integrations = JSON.parse(get('skills-manifest/inventory.json')).entries.filter((entry) => entry.kind !== 'cli');

function taskRows() {
  return [...plan.matchAll(/^- \[[ x]\] \*\*([\w-]+)\*\* — /gm)].map((match) => match[1]);
}

test('every original acceptance inventory has an independent execution row', () => {
  const ids = taskRows();
  assert.equal(ids.length, new Set(ids).size, 'duplicate task ID');
  const count = (prefix) => ids.filter((id) => id.startsWith(prefix)).length;
  assert.equal(count('TRACK-'), 23);
  assert.equal(count('ACC-'), 188);
  assert.equal(count('FEATURE-'), 40);
  assert.equal(count('CI-'), 17);
  assert.equal(count('ROLE-'), roles.length);
  assert.equal(count('INTEGRATION-'), integrations.length);
  assert.equal(count('DISCOVERY-'), 23);
  assert.equal(count('SPEC-'), 520);
  for (const id of roles.map((role) => `ROLE-${role.id}`)) assert.ok(ids.includes(id), id);
  for (const id of integrations.map((entry) => `INTEGRATION-${entry.id.replace(':', '-')}`)) assert.ok(ids.includes(id), id);
});

test('every task has acceptance, dependency, actual owner and blocker metadata', () => {
  const rows = [...plan.matchAll(/^- \[[ x]\] \*\*([\w-]+)\*\* — (.+)\n  - \*\*Status:\*\* (.+)\n  - \*\*Files\/components:\*\* (.+)\n  - \*\*Evidence:\*\* (.+)/gm)];
  assert.equal(rows.length, taskRows().length);
  for (const [, id, title, statusLine, acceptance, evidence] of rows) {
    assert.ok(title.trim(), `missing title ${id}`);
    assert.match(statusLine, /(?:TODO|IN PROGRESS|BLOCKED|VERIFIED DONE) · \*\*Priority:\*\* P[0-2]/, id);
    assert.match(statusLine, /\*\*Worker\/chat:\*\* .* · \*\*Dependencies:\*\* .+/, id);
    assert.match(acceptance, /\*\*Acceptance\/tests:\*\* .+/, id);
    assert.match(evidence, /\*\*Remaining blocker:\*\* .+/, id);
  }
});

test('master source present, wave/workstream structure and high-risk release checks remain open', () => {
  assert.match(specs, /## 33\. The instruction that matters most/);
  assert.equal((plan.match(/^### Wave \d{2} — /gm) ?? []).length, 9);
  assert.ok((plan.match(/^#### Workstream — /gm) ?? []).length >= 16);
  for (const release of ['RELEASE-006', 'RELEASE-007', 'RELEASE-008', 'RELEASE-016', 'RELEASE-017', 'RELEASE-020']) {
    assert.match(plan, new RegExp(`^- \\[ \\] \\*\\*${release}\\*\\* — `, 'm'));
  }
  // The generator deliberately no longer freezes yesterday's remote or
  // worker state into its header. Check the release *requirement* rather
  // than an obsolete observation that could remain after setup changes.
  assert.match(plan, /verified intended remote/);
  assert.match(plan, /no verified remote push or public release exists/);
  assert.match(plan, /The 188-row platform ledger/);
});
