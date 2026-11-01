import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const get = (path) => readFileSync(join(root, path), 'utf8');
const plan = get('docs/mission/COMPLETE_EXECUTION_PLAN.md');
const generator = get('docs/mission/generate-complete-execution-plan.mjs');
const ledger = get('docs/quality/acceptance-ledger.md');
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

test('all 520 public specification IDs and 188 platform ledger identities match exactly', () => {
  const match = generator.match(
    /const PUBLIC_SPEC_ROWS = (\[[\s\S]+?\]);\s*const PUBLIC_TRACK_ROWS = /,
  );
  assert.ok(match, 'the generator must version its public specification input');
  const sourceRows = JSON.parse(match[1]);
  const publicIds = sourceRows.map(([id, title, section, originalLine]) => {
    assert.match(id, /^SPEC-\d{2}-\d{3}$/);
    assert.ok(title.length > 10, id);
    assert.ok(section.length > 0, id);
    assert.ok(Number.isInteger(originalLine) && originalLine > 0, id);
    return id;
  });
  assert.equal(publicIds.length, 520);
  assert.equal(new Set(publicIds).size, 520);
  assert.deepEqual(publicIds.slice().sort(),
    taskRows().filter((id) => id.startsWith('SPEC-')).sort());

  const ledgerIds = ledger.split(/\r?\n/).map((line) => line.split('|')[1]?.trim())
    .filter((id) => /^[A-Z]+-\d{2}$/.test(id));
  assert.equal(ledgerIds.length, 188);
  const expected = ledgerIds.map((id, index) =>
    'ACC-' + id + '-' + String(index + 1).padStart(3, '0'));
  assert.deepEqual(expected.sort(),
    taskRows().filter((id) => id.startsWith('ACC-')).sort());
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

test('published public requirement synopsis, waves and high-risk release gates remain open', () => {
  assert.match(generator, /const PUBLIC_SPEC_ROWS = \[/);
  assert.match(generator, /const PUBLIC_TRACK_ROWS = \[/);
  assert.doesNotMatch(generator, /read\(['"](?:Codex_Master_Prompt|docs\/mission\/active-task-plan\.md)/);
  assert.equal((plan.match(/^### Wave \d{2} — /gm) ?? []).length, 9);
  assert.ok((plan.match(/^#### Workstream — /gm) ?? []).length >= 16);
  for (const release of ['RELEASE-006', 'RELEASE-007', 'RELEASE-008', 'RELEASE-016', 'RELEASE-017', 'RELEASE-020']) {
    assert.match(plan, new RegExp(`^- \\[ \\] \\*\\*${release}\\*\\* — `, 'm'));
  }
  // The generator deliberately no longer freezes yesterday's remote or
  // worker state into its header. Check the release *requirement* rather
  // than an obsolete observation that could remain after setup changes.
  assert.match(plan, /verified intended remote/);
  assert.match(plan, /does not establish a public release/);
  assert.match(plan, /The 188-row platform ledger/);
  assert.match(plan, /\*\*PLAN-001\*\* — /);
  assert.match(plan, /\*\*Status:\*\* BLOCKED/);
  assert.doesNotMatch(plan, /\]\(\.\.\/\.\.\/Codex_Master_Prompt_Local_Agent_Collaboration_Platform\.md\)/);
  assert.doesNotMatch(plan, /\]\(active-task-plan\.md\)/);
  assert.doesNotMatch(plan, /\]\(\.\.\/\.\.\/skills-manifest\/evidence\/\)/);
});

test('generator deterministically verifies a clean, public-input-only source snapshot', () => {
  const temp = mkdtempSync(join(tmpdir(), 'cstl-public-traceability-'));
  const paths = [
    'docs/mission/generate-complete-execution-plan.mjs',
    'docs/mission/COMPLETE_EXECUTION_PLAN.md',
    'docs/mission/master-feature-acceptance.md',
    'docs/quality/acceptance-ledger.md',
    'catalog/roles.json',
    'skills-manifest/inventory.json',
  ];
  try {
    for (const relative of paths) {
      const output = join(temp, relative);
      mkdirSync(dirname(output), { recursive: true });
      writeFileSync(output, get(relative));
    }
    // Neither the private original master nor the private coordination
    // board, local skill receipts or hidden screenshots exist in the snapshot.
    const run = () => spawnSync(
      process.execPath,
      [join(temp, 'docs/mission/generate-complete-execution-plan.mjs'), '--check'],
      { cwd: temp, encoding: 'utf8', timeout: 10_000, windowsHide: true },
    );
    const result = run();
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Verified deterministic plan content/);
    // Deliberately alter the *output* without updating public source inputs.
    // The check MUST fail rather than silently trusting the plan it verifies.
    const planPath = join(temp, 'docs/mission/COMPLETE_EXECUTION_PLAN.md');
    writeFileSync(planPath, get('docs/mission/COMPLETE_EXECUTION_PLAN.md').replace(
      '**Coverage:**', '**Coverage: deliberately altered**',
    ));
    const tampered = run();
    assert.equal(tampered.status, 1);
    assert.match(tampered.stderr, /stale/);
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});
