import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { resolveEvidencePath, validateManifest } from './validate.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const inventory = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
const copy = () => structuredClone(inventory);
const invalid = (data, pattern, options) => {
  const result = validateManifest(data, options);
  assert.equal(result.ok, false);
  assert.match(result.errors.join('\n'), pattern);
};

test('real manifest validates without pretending skills are installed and exercised', () => {
  const result = validateManifest(inventory);
  assert.deepEqual(result.errors, []);
  assert.equal(result.ok, true);
  assert.equal(result.counts.total_entries, 208);
  assert.equal(result.counts.cli_observed, 8);
  assert.equal(result.counts.skill_source_reviewed, 42);
  assert.equal(result.counts.skill_installed_unvetted, 158);
  assert.equal(result.counts.vetted_installed_exercised, 0);
  assert.equal(result.counts.remaining_to_target, 200);
});

test('local read-only SHA256, frontmatter, license and executable paths match observed evidence', () => {
  const result = validateManifest(inventory, { verifyLocal: true });
  assert.deepEqual(result.errors, []);
});

test('does not accept duplicate entry IDs', () => {
  const manifest = copy();
  manifest.entries.push(structuredClone(manifest.entries[0]));
  invalid(manifest, /duplicate ID/);
});

test('does not accept duplicate skill under another integration type', () => {
  const manifest = copy();
  manifest.entries.push({
    ...structuredClone(manifest.entries[8]),
    id: 'mcp:completion-claims-auditor',
    kind: 'mcp',
    status: 'configured_unverified',
  });
  invalid(manifest, /duplicates a development capability name/);
});

test('CLI and skill state categories may not be interchanged', () => {
  const manifest = copy();
  manifest.entries[0].status = 'installed_vetted';
  invalid(manifest, /status invalid for cli/);
});

test('source-reviewed skills require observed license evidence', () => {
  const manifest = copy();
  manifest.entries[8].license = {
    status: 'unverified',
    spdx: null,
    evidence: null,
  };
  invalid(manifest, /source_reviewed requires verified license/);
});

test('installed-vetted status cannot be claimed without passing exercise evidence', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.id === 'skill:ai-debt-detector').status = 'installed_vetted';
  invalid(manifest, /requires license and real passing exercise evidence/);
});

test('even a passing exercise cannot qualify without a verified license', () => {
  const manifest = copy();
  const entry = manifest.entries.find((item) => item.id === 'skill:ai-debt-detector');
  entry.status = 'installed_vetted';
  entry.exercise = {
    command: 'test command',
    result: 'observed pass',
    outcome: 'passed',
    observed_on: '2026-10-30',
  };
  invalid(manifest, /requires license and real passing exercise evidence/);
});

test('fabricated exercise receipt cannot pass local verification or inflate totals', () => {
  const manifest = copy();
  const entry = manifest.entries.find((item) => item.id === 'skill:ai-debt-detector');
  entry.status = 'installed_vetted';
  entry.license = {
    status: 'verified',
    spdx: 'MIT',
    evidence: '../vibe-coding-skills/LICENSE',
  };
  entry.exercise = {
    command: 'claimed verification command',
    result: 'claimed pass',
    outcome: 'passed',
    observed_on: '2026-10-30',
    receipt_path: 'docs/dev-tools/evidence/not-a-real-test.json',
    receipt_sha256: '0'.repeat(64),
  };
  const portable = validateManifest(manifest);
  assert.equal(portable.ok, true);
  assert.equal(portable.counts.vetted_installed_exercised, 0);
  const local = validateManifest(manifest, { verifyLocal: true });
  assert.equal(local.counts.vetted_installed_exercised, 0);
  assert.match(local.errors.join('\n'), /exercise receipt does not exist/);
});

test('pretended 200 count is rejected and strict milestone still fails honestly', () => {
  const manifest = copy();
  manifest.claimed_vetted_count = 200;
  invalid(manifest, /claimed_vetted_count does not match/);
  invalid(copy(), /target is unmet/, { requireTarget: true });
});

test('missing source fingerprint is rejected', () => {
  const manifest = copy();
  manifest.entries[8].source.sha256 = null;
  invalid(manifest, /source.sha256/);
});

test('tampered hash is detected by the read-only local verification', () => {
  const manifest = copy();
  manifest.entries[8].source.sha256 = '0'.repeat(64);
  invalid(manifest, /SHA256 mismatch/, { verifyLocal: true });
});

test('missing license path is detected when checking filesystem', () => {
  const manifest = copy();
  manifest.entries[8].license.evidence = '../vibe-coding-skills/LICENSE-NOT-THERE';
  invalid(manifest, /license evidence missing/, { verifyLocal: true });
});

test('missing environment variable never silently substitutes a guessed path', () => {
  assert.throws(
    () => resolveEvidencePath('%NOT_SET_BY_TEST%/bad', {}),
    /Missing environment variable/,
  );
});

test('200 distinct observed skill filenames are never counted as 200 exercised integrations', () => {
  const skills = inventory.entries.filter((entry) => entry.kind === 'skill');
  assert.equal(skills.length, 200);
  assert.equal(new Set(skills.map((entry) => entry.id)).size, 200);
  assert.equal(new Set(skills.map((entry) => entry.source.path.toLowerCase())).size, 200);
  assert.equal(skills.filter((entry) => entry.status === 'installed_vetted').length, 0);
  assert.ok(skills.every((entry) => /^[a-f0-9]{64}$/.test(entry.source.sha256)));
  const local = validateManifest(inventory, { verifyLocal: true, requireTarget: true });
  assert.equal(local.counts.vetted_installed_exercised, 0);
  assert.match(local.errors.join('\n'), /target is unmet/);
});

test('tampered user-level skill content hash fails verification', () => {
  const manifest = copy();
  const entry = manifest.entries.find((item) => item.id === 'skill:ai-debt-detector');
  assert.ok(entry);
  entry.source.sha256 = 'f'.repeat(64);
  invalid(manifest, /SHA256 mismatch/, { verifyLocal: true });
});

test('MCP file fingerprints must be checked locally, including unverified configurations', () => {
  const manifest = copy();
  manifest.entries.push({
    id: 'mcp:missing-fingerprint-fixture',
    kind: 'mcp',
    status: 'configured_unverified',
    purpose: 'Test whether a fabricated MCP source digest is rejected',
    source: {
      path: '../vibe-coding-skills/skills/agent-reliability/completion-claims-auditor/SKILL.md',
      provenance: 'Read-only test fixture pointing to an existing non-MCP skill file',
      sha256: 'f'.repeat(64),
    },
    evidence: { command: 'fixture only', output: 'No real MCP integration exercised' },
    license: { status: 'unverified', spdx: null, evidence: null },
    limitations: ['Fixture only; no MCP was configured or contacted'],
  });
  invalid(manifest, /SHA256 mismatch/, { verifyLocal: true });
});

test('rejects absent limitations, missing version probes and invalid target', () => {
  const manifest = copy();
  manifest.entries[0].limitations = [];
  manifest.entries[1].evidence.version = '';
  manifest.target_vetted_skill_mcp_count = 42;
  invalid(manifest, /limitations.*|version.*|target_vetted_skill_mcp_count/);
  const errors = validateManifest(manifest).errors.join('\n');
  assert.match(errors, /target_vetted_skill_mcp_count/);
  assert.match(errors, /evidence.version/);
  assert.match(errors, /limitations/);
});
