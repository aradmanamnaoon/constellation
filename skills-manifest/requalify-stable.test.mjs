import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { inspectInstalled, refresh } from './requalify-stable.mjs';
import { auditSourceCandidates } from './audit-source-candidates.mjs';
import { exercise } from './exercise-local.mjs';
import { runCandidate } from './reviewed-exercise.mjs';

const manifest = JSON.parse(readFileSync(new URL('./inventory.json', import.meta.url)));
const installed = manifest.entries.filter((entry) => entry.status === 'installed_vetted');
const find = (name) => structuredClone(manifest.entries.find((entry) => entry.id === 'skill:' + name));

test('each refresh candidate has a real pinned installed skill, license, and unique receipt path', () => {
  const used = new Set();
  for (const entry of installed) {
    const result = inspectInstalled(entry, used);
    assert.equal(result.name, entry.id.slice(6));
    assert.ok(result.receipt.endsWith(result.name + '.json'));
  }
  assert.equal(used.size, installed.length);
});

test('forged or aliased installation, repository and receipt paths reject qualification', () => {
  const original = find('api-contract-auditor');
  const forged = structuredClone(original);
  forged.source.path = forged.source.path.replace('/api-contract-auditor/', '/input-validation-auditor/');
  assert.throws(() => inspectInstalled(forged), /installation identity/);

  const alias = structuredClone(original);
  alias.exercise.receipt_path =
    'skills-manifest/evidence/../evidence/input-validation-auditor.json';
  assert.throws(() => inspectInstalled(alias), /forged\/aliased receipt/);

  const switchedSource = structuredClone(original);
  switchedSource.source.upstream_path =
    '../vibe-coding-skills/skills/security/input-validation-auditor/SKILL.md';
  assert.throws(() => inspectInstalled(switchedSource), /pinned Git source mismatch/);

  const duplicate = new Set();
  inspectInstalled(original, duplicate);
  assert.throws(() => inspectInstalled(original, duplicate), /duplicate receipt/);
});

test('source-reviewed candidates without managed installation and live MCP exercise earn zero credit', () => {
  const review = auditSourceCandidates(manifest);
  assert.equal(review.ok, true);
  assert.equal(review.summary.source_reviewed + review.summary.promoted_vetted, 20);
  assert.equal(review.summary.uninstalled + review.summary.promoted_vetted, 20);
  assert.equal(review.summary.source_review_credit, 0);
  assert.equal(manifest.entries.filter((entry) => entry.kind === 'mcp').length, 0);
  assert.throws(() => inspectInstalled(find('mcp-reliability-auditor')), /pre-qualified/);
});

test('independent real Constellation exercises are not replaced with receipt booleans', () => {
  const liveLocal = exercise('api-contract-auditor');
  const liveReviewed = runCandidate('injection-risk-auditor');
  assert.equal(liveLocal.outcome, 'passed');
  assert.equal(liveReviewed.outcome, 'passed');
  assert.ok(liveLocal.checks.length >= 3);
  assert.ok(liveReviewed.checks.length >= 3);
  assert.ok(Object.values(liveLocal.inputs).every((input) => input.sha256.length === 64));
  assert.ok(Object.values(liveReviewed.inputs).every((input) => input.sha256.length === 64));
  assert.equal(liveReviewed.qualification_credit, 0);
});

test('an actual SQLite schema mutation changes source evidence and fails the live audit', () => {
  const corePath = new URL('../crates/constellation-core/src/store.rs', import.meta.url);
  const source = readFileSync(corePath, 'utf8');
  const weakened = source.replace('FOREIGN KEY(room_id) REFERENCES rooms(id)',
    'CHECK(length(room_id)>0)');
  assert.notEqual(weakened, source);
  const original = exercise('database-schema-reviewer');
  const mutated = exercise('database-schema-reviewer', { coreStore: weakened });
  assert.equal(original.outcome, 'passed');
  assert.equal(mutated.outcome, 'failed');
  assert.equal(original.inputs.coreStore.path, 'crates/constellation-core/src/store.rs');
  assert.notEqual(original.inputs.coreStore.sha256, mutated.inputs.coreStore.sha256);
  assert.ok(mutated.checks.some((check) =>
    check.check === 'Core rejects message linked to missing room' && check.status === 'failed'));
});

test('read-only requalification audits all installed runners and reports current source drift', () => {
  const audit = refresh();
  assert.equal(audit.renewed, 0);
  assert.equal(audit.counts.total_entries, 208);
  assert.equal(audit.counts.skill_source_reviewed,
    manifest.entries.filter((entry) => entry.status === 'source_reviewed').length);
  assert.equal(audit.counts.skill_installed_unvetted, 158);
  assert.ok(audit.counts.vetted_installed_exercised < 200);
  assert.equal(audit.counts.remaining_to_target,
    200 - audit.counts.vetted_installed_exercised);
  const failed = audit.outcomes.filter((result) => result.status === 'failing_checks');
  assert.ok(failed.every((result) => result.failed_checks?.length > 0));
  assert.ok(audit.outcomes.every((result) =>
    ['eligible_stable', 'failing_checks', 'unstable_source', 'blocked'].includes(result.status)));
  assert.equal(audit.outcomes.filter((result) => result.status === 'eligible_stable').length
    + audit.outcomes.filter((result) => result.status !== 'eligible_stable').length,
  installed.length);
  assert.equal(audit.outcomes.length, installed.length);
});
