import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { runCandidate } from './reviewed-exercise.mjs';
import { validateManifest } from './validate.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');

for (const name of [
  'injection-risk-auditor', 'migration-safety-reviewer',
  'requirement-traceability-auditor', 'scope-drift-guard',
  'dependency-grounding-auditor',
  'accessibility-auditor', 'responsive-design-auditor', 'test-quality-auditor',
]) {
  test(`candidate ${name} passes source-pin/licensing and project-specific checks`, () => {
    const receipt = runCandidate(name);
    assert.equal(receipt.outcome, 'passed');
    assert.ok(receipt.checks.length >= 5);
    assert.equal(receipt.qualification_credit, 0);
    assert.equal(receipt.upstream.publisher_identity, 'unverified');
    assert.match(receipt.upstream.revision, /^[a-f0-9]{40}$/);
    assert.equal(receipt.input.sha256.length, 64);
    assert.ok(receipt.checks.every((check) => check.result === 'pass'));
  });
}

test('injection audit fails for removed SQL placeholder and unsafe lookup binding', () => {
  const source = read('crates/constellation-core/src/store.rs');
  assert.equal(runCandidate('injection-risk-auditor',
    source.replace('SELECT 1 FROM rooms WHERE id = ?1', 'SELECT 1 FROM rooms WHERE id = 1')).outcome,
  'failed');
  assert.equal(runCandidate('injection-risk-auditor',
    source.replace('.query_row(sql, [id],', '.query_row(sql, [],')).outcome, 'failed');
});

test('migration audit detects vanished version guard and revised zero-revision constraint', () => {
  const source = read('crates/constellation-workflows/src/repository.rs');
  assert.equal(runCandidate('migration-safety-reviewer',
    source.replace('if version != 0 && version != DB_VERSION', 'if false')).outcome, 'failed');
  assert.equal(runCandidate('migration-safety-reviewer',
    source.replace('CHECK(revision > 0)', 'CHECK(revision >= 0)')).outcome, 'failed');
});

test('requirement coverage catches dropped scope and invented PASS without evidence', () => {
  const ledger = read('docs/quality/acceptance-ledger.md');
  const dropped = ledger.replace(/^\| CORE-01 \|[^\n]*\r?\n/m, '');
  assert.notEqual(dropped, ledger);
  assert.equal(runCandidate('requirement-traceability-auditor', dropped).outcome, 'failed');
  const fraudulent = ledger.replace(/\*\*UNVERIFIED\*\*/, '**PASS**');
  assert.notEqual(fraudulent, ledger);
  assert.equal(runCandidate('requirement-traceability-auditor', fraudulent).outcome, 'failed');
});

test('scope audit catches deleted repository preservation instruction', () => {
  const source = read('AGENTS.md');
  assert.equal(runCandidate('scope-drift-guard',
    source.replace('Do not print secrets', 'Allow logging secrets')).outcome, 'failed');
});

test('dependency audit detects invented npm, stale lock and incompatible Cargo dependencies', () => {
  const manifest = read('apps/desktop/package.json');
  const pkg = JSON.parse(manifest);
  pkg.dependencies['@tauri-apps/api'] = '^9';
  assert.equal(runCandidate('dependency-grounding-auditor', JSON.stringify(pkg)).outcome, 'failed');
  const lock = read('apps/desktop/pnpm-lock.yaml');
  const brokenLock = lock.replace("specifier: ^2\n        version: 2.12.4", 'specifier: ^2\n        version: 99.0.0');
  assert.notEqual(brokenLock, lock);
  assert.equal(runCandidate('dependency-grounding-auditor', undefined,
    {pnpmLock:brokenLock}).outcome, 'failed');
  const cargo = read('apps/desktop/src-tauri/Cargo.toml');
  const brokenCargo = cargo.replace('tauri = { version = "2"', 'tauri = { version = "9"');
  assert.notEqual(brokenCargo, cargo);
  assert.equal(runCandidate('dependency-grounding-auditor', undefined,
    {cargoManifest:brokenCargo}).outcome, 'failed');
  const packageMetadata = read('apps/desktop/node_modules/svelte/package.json');
  assert.equal(runCandidate('dependency-grounding-auditor', undefined,
    {svelte:packageMetadata.replace('"version": "5.57.3"', '"version": "8.0.0"')}).outcome, 'failed');
});

test('accessibility audit fails for vanished names, alert semantics and focus style', () => {
  const ui = read('apps/desktop/src/routes/+page.svelte');
  const css = read('apps/desktop/src/routes/studio.css');
  const missingName = ui.replace('aria-label="Main navigation"','aria-label=""');
  assert.notEqual(missingName,ui);
  assert.equal(runCandidate('accessibility-auditor',missingName).outcome,'failed');
  const silent = ui.replace('class="error-toast" role="alert"','class="error-toast"');
  assert.notEqual(silent,ui);
  assert.equal(runCandidate('accessibility-auditor',silent).outcome,'failed');
  const noFocus = css.replace('button:focus-visible, input:focus-visible','button:hover, input:hover');
  assert.notEqual(noFocus,css);
  assert.equal(runCandidate('accessibility-auditor',undefined,{css:noFocus}).outcome,'failed');
});

test('responsive audit rejects removed narrow-grid reflow and hidden-nav keyboard guard', () => {
  const css = read('apps/desktop/src/routes/studio.css');
  const noMainShrink = css.replace('.main { display:flex; flex-direction:column; background:var(--color-surface); flex:1; min-width:0;',
    '.main { display:flex; flex-direction:column; background:var(--color-surface); flex:1; min-width:auto;');
  assert.notEqual(noMainShrink, css);
  assert.equal(runCandidate('responsive-design-auditor', noMainShrink).outcome, 'failed');
  const noSingleColumn = css.replace('.action-grid,.roles-grid{grid-template-columns:1fr;}','.action-grid,.roles-grid{grid-template-columns:3fr 1fr;}');
  assert.notEqual(noSingleColumn,css);
  assert.equal(runCandidate('responsive-design-auditor',noSingleColumn).outcome,'failed');
  const tabbableHidden = css.replace('.sidebar:not(.open){visibility:hidden;pointer-events:none;}','.sidebar:not(.open){opacity:0;}');
  assert.notEqual(tabbableHidden,css);
  assert.equal(runCandidate('responsive-design-auditor',tabbableHidden).outcome,'failed');
  const overflow = css.replace('max-height:90dvh; overflow-y:auto;','max-height:90dvh; overflow-y:hidden;');
  assert.notEqual(overflow,css);
  assert.equal(runCandidate('responsive-design-auditor',overflow).outcome,'failed');
});

test('test-quality audit catches missing anti-replay, confidentiality and native-proof assertions', () => {
  const host = read('apps/desktop/src-tauri/src/tool_host.rs');
  const replay = host.replace('assert_ne!(ticket1, ticket2);','assert_eq!(ticket1.len(), 64);');
  assert.notEqual(replay,host);
  assert.equal(runCandidate('test-quality-auditor',replay).outcome,'failed');
  const leak = host.replace('assert!(preview.get("sha256").is_none());','assert!(preview.is_object());');
  assert.notEqual(leak,host);
  assert.equal(runCandidate('test-quality-auditor',leak).outcome,'failed');
  const proof = host.replace('!native_proof.verify(&operation_id.to_string(), pending.prepared.digest())','false');
  assert.notEqual(proof,host);
  assert.equal(runCandidate('test-quality-auditor',proof).outcome,'failed');
});

test('unsupported reviewed skill cannot borrow an approved exercise', () => {
  assert.throws(() => runCandidate('mcp-reliability-auditor'),
    /Unapproved source-reviewed skill/);
});

test('forged reviewed exercise does not earn credit after attacker recomputes receipt SHA', () => {
  const manifest = JSON.parse(read('skills-manifest/inventory.json'));
  const entry = manifest.entries.find((item) => item.id === 'skill:migration-safety-reviewer');
  assert.equal(entry.status, 'installed_vetted');
  // Other independently exercised skills may lose credit when live project
  // sources change. Compare to the actual local baseline instead of assuming
  // all 22 receipts remain current at the time this negative test runs.
  const baseline = validateManifest(manifest, { verifyLocal: true });
  const index = manifest.entries.indexOf(entry);
  assert.equal(
    baseline.errors.some((error) => error.startsWith(`entries[${index}]`)),
    false,
    'the target skill must qualify before testing its forged receipt',
  );
  const receipt = JSON.parse(read('skills-manifest/evidence/reviewed/migration-safety-reviewer.json'));
  receipt.checks[0].name = 'Invented live production migration passed';
  const bytes = Buffer.from(JSON.stringify(receipt, null, 2) + '\n');
  const filename = `.forged-reviewed-${process.pid}.json`;
  const path = join(root, 'skills-manifest', 'evidence', 'reviewed', filename);
  let created = false;
  try {
    writeFileSync(path, bytes, { flag: 'wx' });
    created = true;
    entry.exercise.receipt_path = `skills-manifest/evidence/reviewed/${filename}`;
    entry.exercise.receipt_sha256 = createHash('sha256').update(bytes).digest('hex');
    const result = validateManifest(manifest, { verifyLocal: true });
    assert.match(result.errors.join('\n'), /reviewed exercise receipt differs from independently replayed checks/);
    assert.equal(result.counts.vetted_installed_exercised, baseline.counts.vetted_installed_exercised - 1);
  } finally {
    if (created) unlinkSync(path);
  }
});
