import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { auditSourceCandidates } from './audit-source-candidates.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
const candidate = (inventory, name) => inventory.entries.find(
  (entry) => entry.id === 'skill:' + name,
);

test('local source candidates have pinned distinct hashes and zero qualification credit', () => {
  const result = auditSourceCandidates(manifest);
  assert.deepEqual(result.errors, []);
  assert.equal(result.ok, true);
  assert.equal(result.summary.source_reviewed + result.summary.promoted_vetted, 20);
  assert.equal(result.summary.source_checked, 20);
  assert.equal(result.summary.uninstalled + result.summary.promoted_vetted, 20);
  assert.equal(result.summary.installed_without_exercise, 0);
  assert.equal(result.summary.installed_unvetted_user_level, 158);
  assert.equal(result.summary.source_review_credit, 0);
  assert.equal(new Set(result.reports.map((r) => r.source_sha256)).size, 20);
  assert.ok(result.reports.every((r) => r.git_revision.length === 40
    && r.license.startsWith('MIT')
    && r.publisher_identity === 'unverified'
    && r.qualification_credit === 0));
});

test('source-review rejects fabricated SHA, duplicate artifacts and license claims', () => {
  const altered = structuredClone(manifest);
  candidate(altered, 'ui-flow-verifier').source.sha256 = 'f'.repeat(64);
  candidate(altered, 'missing-test-detector').source.upstream_path =
    candidate(altered, 'ui-flow-verifier').source.path;
  candidate(altered, 'dead-feature-detector').license.spdx = 'Apache-2.0';
  const result = auditSourceCandidates(altered);
  assert.equal(result.ok, false);
  assert.match(result.errors.join('\n'), /source differs from pinned Git blob or inventory SHA/);
  assert.match(result.errors.join('\n'), /invalid upstream source identity/);
  assert.match(result.errors.join('\n'), /license or distinct source identity unverified/);
  assert.equal(result.summary.source_review_credit, 0);
});

test('upstream path traversal cannot be used to claim another source file', () => {
  const altered = structuredClone(manifest);
  candidate(altered, 'mcp-reliability-auditor').source.path =
    '../vibe-coding-skills/skills/../../constellation/skills-manifest/evidence/ghost/mcp-reliability-auditor/SKILL.md';
  const result = auditSourceCandidates(altered);
  assert.equal(result.ok, false);
  assert.match(result.errors.join('\n'), /escapes checkout or is missing/);
  assert.equal(result.summary.source_review_credit, 0);
});
