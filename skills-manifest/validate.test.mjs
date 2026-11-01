import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { resolveEvidencePath, validateManifest } from './validate.mjs';
import { exercise } from './exercise-local.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const inventory = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
const copy = () => structuredClone(inventory);
// Local source inputs can change between independently qualified snapshots.
// Compare negative fixtures to this invocation's independently verified baseline
// rather than assuming a historical number of healthy receipt hashes.
const localBaseline = validateManifest(inventory, { verifyLocal: true });
const locallyClaimedSkills = inventory.entries.filter(
  (entry) => entry.kind === 'skill' && entry.status === 'installed_vetted',
).length;
const creditedAtBaseline = (id) => {
  const index = inventory.entries.findIndex((entry) => entry.id === id);
  assert.notEqual(index, -1, `missing required fixture skill: ${id}`);
  return !localBaseline.errors.some((error) => error.startsWith(`entries[${index}]`));
};
const remainingAfterInvalidating = (...ids) => localBaseline.counts.vetted_installed_exercised
  - ids.filter(creditedAtBaseline).length;
const invalid = (data, pattern, options) => {
  const result = validateManifest(data, options);
  assert.equal(result.ok, false);
  assert.match(result.errors.join('\n'), pattern);
};

test('portable schema validation never grants local exercise credit', () => {
  const result = validateManifest(inventory);
  assert.deepEqual(result.errors, []);
  assert.equal(result.ok, true);
  assert.equal(result.counts.total_entries, inventory.entries.length);
  assert.equal(result.counts.cli_observed, 8);
  assert.equal(result.counts.skill_source_reviewed, inventory.entries.filter(
    (entry) => entry.kind === 'skill' && entry.status === 'source_reviewed',
  ).length);
  assert.equal(result.counts.skill_installed_unvetted, inventory.entries.filter(
    (entry) => entry.kind === 'skill' && entry.status === 'installed_unvetted',
  ).length);
  assert.equal(result.counts.vetted_installed_exercised, 0);
  assert.equal(result.counts.remaining_to_target, inventory.target_vetted_skill_mcp_count);
});

test('local read-only SHA256, frontmatter, license and executable paths match observed evidence', () => {
  assert.deepEqual(localBaseline.errors, []);
  assert.equal(localBaseline.counts.vetted_installed_exercised, locallyClaimedSkills);
  assert.equal(localBaseline.counts.remaining_to_target,
    inventory.target_vetted_skill_mcp_count - locallyClaimedSkills);
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

test('distinct skill names cannot reuse the same underlying source artifact', () => {
  const manifest = copy();
  const duplicated = structuredClone(manifest.entries.find(
    (item) => item.id === 'skill:api-contract-auditor',
  ));
  duplicated.id = 'skill:contract-auditor-alias';
  duplicated.source.path = duplicated.source.path
    .replace('/api-contract-auditor/SKILL.md', '/api-contract-auditor/./SKILL.md')
    .replaceAll('/', '\\');
  manifest.entries.push(duplicated);
  invalid(manifest, /reuses a source artifact/);
});

test('independently named vetted entries cannot reuse a pinned upstream skill', () => {
  const manifest = copy();
  const first = manifest.entries.find((item) => item.id === 'skill:api-contract-auditor');
  const second = manifest.entries.find((item) => item.id === 'skill:completion-claims-auditor');
  second.source.upstream_repo = first.source.upstream_repo;
  second.source.upstream_path = first.source.upstream_path;
  invalid(manifest, /reuses another vetted entry's pinned upstream source/);
});

test('one hashed exercise receipt cannot count toward two distinct skills', () => {
  const manifest = copy();
  const first = manifest.entries.find((item) => item.id === 'skill:api-contract-auditor');
  const second = manifest.entries.find((item) => item.id === 'skill:completion-claims-auditor');
  second.exercise.receipt_path = first.exercise.receipt_path.replaceAll('/', '\\');
  invalid(manifest, /reuses another vetted entry's exercise receipt/);
});

test('CLI and skill state categories may not be interchanged', () => {
  const manifest = copy();
  manifest.entries[0].status = 'installed_vetted';
  invalid(manifest, /status invalid for cli/);
});

test('source-reviewed skills require observed license evidence', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.status === 'source_reviewed').license = {
    status: 'unverified',
    spdx: null,
    evidence: null,
  };
  invalid(manifest, /source_reviewed requires verified license/);
});

test('installed-vetted status cannot be claimed without passing exercise evidence', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.id === 'skill:ai-debt-detector').status = 'installed_vetted';
  invalid(manifest, /installed_vetted requires pinned upstream provenance, verified license, and passing exercise/);
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
  invalid(manifest, /installed_vetted requires pinned upstream provenance, verified license, and passing exercise/);
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
  // A claim with no pinned upstream source/license or hashed exercise runner
  // is invalid even when the caller does not request local verification.
  assert.equal(portable.ok, false);
  assert.match(portable.errors.join('\n'), /installed_vetted requires pinned upstream provenance/);
  assert.equal(portable.counts.vetted_installed_exercised, 0);
  const local = validateManifest(manifest, { verifyLocal: true });
  assert.equal(local.counts.vetted_installed_exercised,
    localBaseline.counts.vetted_installed_exercised);
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

test('200 distinct observed skill filenames never become 200 verified integrations', () => {
  const skills = inventory.entries.filter((entry) => entry.kind === 'skill');
  assert.equal(skills.length, 200);
  assert.equal(new Set(skills.map((entry) => entry.id)).size, 200);
  assert.equal(new Set(skills.map((entry) => entry.source.path.toLowerCase())).size, 200);
  assert.equal(skills.filter((entry) => entry.status === 'installed_vetted').length,
    locallyClaimedSkills);
  assert.ok(skills.every((entry) => /^[a-f0-9]{64}$/.test(entry.source.sha256)));
  const local = validateManifest(inventory, { verifyLocal: true, requireTarget: true });
  assert.equal(local.counts.vetted_installed_exercised,
    localBaseline.counts.vetted_installed_exercised);
  assert.ok(local.counts.vetted_installed_exercised < inventory.target_vetted_skill_mcp_count);
  assert.match(local.errors.join('\n'), /target is unmet/);
});

test('a matching upstream file is not proof of a managed local installation', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.id === 'skill:api-contract-auditor')
    .source.path = '../vibe-coding-skills/skills/testing/api-contract-auditor/SKILL.md';
  invalid(manifest, /actual managed installation path/, { verifyLocal: true });
});

test('pinned upstream revision rejects substituted commit IDs', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.id === 'skill:dependency-cycle-auditor')
    .source.upstream_revision = '0'.repeat(40);
  invalid(manifest, /could not verify the pinned installed source/, { verifyLocal: true });
});

test('altered receipt digest removes qualification for that row', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.id === 'skill:api-contract-auditor')
    .exercise.receipt_sha256 = 'f'.repeat(64);
  const result = validateManifest(manifest, { verifyLocal: true });
  assert.equal(result.counts.vetted_installed_exercised,
    remainingAfterInvalidating('skill:api-contract-auditor'));
  assert.match(result.errors.join('\n'), /exercise receipt SHA256 mismatch/);
});

test('a self-asserted passing receipt is rejected even if its digest is updated', () => {
  const manifest = copy();
  const entry = manifest.entries.find((item) => item.id === 'skill:api-contract-auditor');
  const receipt = JSON.parse(readFileSync(
    join(here, 'evidence', 'api-contract-auditor.json'), 'utf8',
  ));
  // Preserve the legitimate project source hashes and all-passed statuses.
  // Someone could otherwise replace the recorded checks with invented claims,
  // recompute the digest and still satisfy a hash-only verifier.
  receipt.checks[0].check = 'Fabricated external MCP execution passed';
  const forgedBytes = Buffer.from(JSON.stringify(receipt, null, 2) + '\n');
  const fixtureName = `.forged-receipt-test-${process.pid}.json`;
  const fixturePath = join(here, 'evidence', fixtureName);
  let createdFixture = false;
  try {
    writeFileSync(fixturePath, forgedBytes, { flag: 'wx' });
    createdFixture = true;
    entry.exercise.receipt_path = `skills-manifest/evidence/${fixtureName}`;
    entry.exercise.receipt_sha256 = createHash('sha256').update(forgedBytes).digest('hex');
    const result = validateManifest(manifest, { verifyLocal: true });
    assert.equal(result.counts.vetted_installed_exercised,
      remainingAfterInvalidating('skill:api-contract-auditor'));
    assert.match(result.errors.join('\n'), /differs from independently replayed checks/);
  } finally {
    if (createdFixture) unlinkSync(fixturePath);
  }
});

test('receipt paths outside the approved evidence directories never earn credit', () => {
  const manifest = copy();
  const entry = manifest.entries.find((item) => item.id === 'skill:completion-claims-auditor');
  entry.exercise.receipt_path = '../vibe-coding-skills/LICENSE';
  const result = validateManifest(manifest, { verifyLocal: true });
  assert.equal(result.counts.vetted_installed_exercised,
    remainingAfterInvalidating('skill:completion-claims-auditor'));
  assert.match(result.errors.join('\n'), /receipt must be under approved local evidence directories/);
});

test('a copied development skill receipt cannot masquerade as a live MCP integration', () => {
  const manifest = copy();
  const skill = manifest.entries.find((item) => item.id === 'skill:api-contract-auditor');
  const forged = structuredClone(skill);
  forged.id = 'mcp:fake-live-server';
  forged.kind = 'mcp';
  manifest.entries.push(forged);
  const result = validateManifest(manifest, { verifyLocal: true });
  assert.equal(result.counts.vetted_installed_exercised,
    localBaseline.counts.vetted_installed_exercised);
  assert.match(result.errors.join('\n'), /no approved executable verifier for this skill\/MCP|exercise receipt assertions or source identity invalid/);
});

test('changing the exercise objective does not earn verified credit', () => {
  const manifest = copy();
  const entry = manifest.entries.find((item) => item.id === 'skill:api-contract-auditor');
  entry.exercise.result = 'External provider was exercised successfully';
  const result = validateManifest(manifest, { verifyLocal: true });
  assert.equal(result.counts.vetted_installed_exercised,
    remainingAfterInvalidating('skill:api-contract-auditor'));
  assert.match(result.errors.join('\n'), /exercise receipt assertions or source identity invalid|differs from independently replayed checks/);
});

test('tampered installed license and exercise runner fingerprints invalidate credits', () => {
  const manifest = copy();
  manifest.entries.find((entry) => entry.id === 'skill:input-validation-auditor')
    .license.sha256 = '0'.repeat(64);
  manifest.entries.find((entry) => entry.id === 'skill:dependency-cycle-auditor')
    .exercise.runner_sha256 = '0'.repeat(64);
  const result = validateManifest(manifest, { verifyLocal: true });
  assert.equal(result.counts.vetted_installed_exercised,
    remainingAfterInvalidating('skill:input-validation-auditor',
      'skill:dependency-cycle-auditor'));
  assert.match(result.errors.join('\n'), /installed license fingerprint mismatch/);
  assert.match(result.errors.join('\n'), /pinned exercise runner missing or modified/);
});

test('skill exercises reject real contract, size-limit, and documentation regressions', () => {
  const ui = readFileSync(join(here, '..', 'apps/desktop/src/routes/+page.svelte'), 'utf8');
  const native = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/lib.rs'), 'utf8');
  const guide = readFileSync(join(here, '..', 'docs/getting-started.md'), 'utf8');
  const contract = exercise('api-contract-auditor', {
    ui: ui.replace("'studio_search_evidence', { sourceText, query }", "'studio_search_evidence', { badField, query }"),
  });
  assert.equal(contract.outcome, 'failed');
  assert.ok(contract.checks.some((item) => item.status === 'failed'));
  const limits = exercise('input-validation-auditor', {
    native: native.replaceAll('max_query_bytes: 2048', 'max_query_bytes: 16384'),
  });
  assert.equal(limits.outcome, 'failed');
  const docs = exercise('documentation-accuracy-auditor', {
    guide: guide.replace('128 KiB (131,072 UTF-8 bytes)', '128 MiB (134,217,728 UTF-8 bytes)'),
  });
  assert.equal(docs.outcome, 'failed');
});

test('completion claim exercise rejects unsupported PASS, duplicate scoped IDs, and unjustified release claims', () => {
  const ledger = readFileSync(join(here, '..', 'docs/quality/acceptance-ledger.md'), 'utf8');
  const activePlan = readFileSync(join(here, '..', 'docs/mission/active-task-plan.md'), 'utf8');
  const passWithoutEvidence = exercise('completion-claims-auditor', {
    claimsLedger: ledger.replace('| **UNVERIFIED** |', '| **PASS** |'),
  });
  assert.equal(passWithoutEvidence.outcome, 'failed');
  assert.ok(passWithoutEvidence.checks.some((check) => check.status === 'failed'));
  const duplicateScopedId = exercise('completion-claims-auditor', {
    claimsLedger: ledger.replace(/^(\| GUI-01 \|[^\n]*)/m, '$1\n$1'),
  });
  assert.equal(duplicateScopedId.outcome, 'failed');
  const fakeRelease = exercise('completion-claims-auditor', {
    activePlan: activePlan.replace('No public release exists yet', 'Public release verified'),
  });
  assert.equal(fakeRelease.outcome, 'failed');
  const falseP10Complete = activePlan.replace(
    /^(\| P10 \|[^|\n]*\| )\*\*IN PROGRESS[^*|\n]*\*\*( \|)/m,
    '$1**COMPLETE**$2',
  );
  assert.notEqual(falseP10Complete, activePlan);
  assert.equal(exercise('completion-claims-auditor', { activePlan: falseP10Complete }).outcome, 'failed');
  const falseCompletedGate = activePlan.replace(
    /^(\| P22 \|[^|\n]*\| )\*\*BLOCKED(?: \([^|\n]*\))?\*\*( \|)/m,
    '$1**COMPLETE**$2',
  );
  assert.notEqual(falseCompletedGate, activePlan);
  assert.equal(exercise('completion-claims-auditor', { activePlan: falseCompletedGate }).outcome, 'failed');
});

test('SQLite schema reviewer executes real SQL and detects removed FK and relaxed revision constraints', () => {
  const coreStore = readFileSync(join(here, '..', 'crates/constellation-core/src/store.rs'), 'utf8');
  const workflowStore = readFileSync(join(here, '..', 'crates/constellation-workflows/src/repository.rs'), 'utf8');
  const normal = exercise('database-schema-reviewer');
  assert.equal(normal.outcome, 'passed');
  assert.equal(normal.checks.length, 16);
  // Core now names the canonical DDL, consumed by both migration and reference
  // validation. A string with no production callsite or FK activation is unsafe.
  assert.match(coreStore, /const STORE_SCHEMA_DDL:\s*&str/);
  const noSchemaCall = coreStore.replaceAll('execute_batch(STORE_SCHEMA_DDL)', 'execute_batch(WRONG_SCHEMA)');
  assert.notEqual(noSchemaCall, coreStore);
  assert.equal(exercise('database-schema-reviewer', { coreStore: noSchemaCall }).outcome, 'failed');
  const noFkEnforcement = coreStore.replace('conn.pragma_update(None, "foreign_keys", "ON")?', 'conn.pragma_update(None, "foreign_keys", "OFF")?');
  assert.notEqual(noFkEnforcement, coreStore);
  assert.equal(exercise('database-schema-reviewer', { coreStore: noFkEnforcement }).outcome, 'failed');
  const brokenForeignKey = exercise('database-schema-reviewer', {
    coreStore: coreStore.replace('FOREIGN KEY(room_id) REFERENCES rooms(id)', 'CHECK(length(room_id)>0)'),
  });
  assert.equal(brokenForeignKey.outcome, 'failed');
  assert.ok(brokenForeignKey.checks.some((check) => check.check === 'Core rejects message linked to missing room' && check.status === 'failed'));
  const relaxedRevision = exercise('database-schema-reviewer', {
    workflowStore: workflowStore.replace('CHECK(revision > 0)', 'CHECK(revision >= 0)'),
  });
  assert.equal(relaxedRevision.outcome, 'failed');
  assert.ok(relaxedRevision.checks.some((check) => check.check === 'Workflow rejects zero revision' && check.status === 'failed'));
});

test('desktop security configuration audit rejects unsafe CSP and added shell privileges', () => {
  const tauriConfig = readFileSync(join(here, '..', 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8');
  const tauriCapability = readFileSync(join(here, '..', 'apps/desktop/src-tauri/capabilities/default.json'), 'utf8');
  assert.equal(exercise('security-misconfiguration-auditor').outcome, 'passed');
  const unsafeCsp = exercise('security-misconfiguration-auditor', {
    tauriConfig: tauriConfig.replace('"default-src": "\'self\'"', '"default-src": "\'self\' \'unsafe-eval\'"'),
  });
  assert.equal(unsafeCsp.outcome, 'failed');
  const addedShell = exercise('security-misconfiguration-auditor', {
    tauriCapability: tauriCapability.replace('"core:default"', '"core:default", "shell:allow-execute"'),
  });
  assert.equal(addedShell.outcome, 'failed');
});

test('agent tool trust exercise rejects missing native consent, missing proof, and spoofed YES origin', () => {
  const native = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/lib.rs'), 'utf8');
  const host = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/tool_host.rs'), 'utf8');
  const consent = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/native_consent.rs'), 'utf8');
  assert.equal(exercise('agent-tool-trust-auditor').outcome, 'passed');
  const skippedConsent = exercise('agent-tool-trust-auditor', {
    native: native.replace('confirm_native_consent(owner_hwnd, &native_request)', '/* skipped */'),
  });
  assert.equal(skippedConsent.outcome, 'failed');
  assert.ok(skippedConsent.checks.some((item) => item.check === 'Native confirmation is required before dispatch' && item.status === 'failed'));
  const missingProof = exercise('agent-tool-trust-auditor', {
    toolHost: host.replace('!native_proof.verify(&operation_id.to_string(), pending.prepared.digest())', 'false'),
  });
  assert.equal(missingProof.outcome, 'failed');
  const forgedButton = exercise('agent-tool-trust-auditor', {
    nativeConsent: consent.replace('expected_button != 0 && lparam == expected_button', 'true'),
  });
  assert.equal(forgedButton.outcome, 'failed');
});

test('authorization auditor rejects missing dispatch task gate and oversized permission grants', () => {
  const native = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/lib.rs'), 'utf8');
  const host = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/tool_host.rs'), 'utf8');
  assert.equal(exercise('authorization-boundary-auditor').outcome, 'passed');
  const missingPendingCheck = exercise('authorization-boundary-auditor', {
    native: native.replace(/(fn execute_prepared_file_if_task_pending\([\s\S]*?)task\.state == TaskState::Pending/, '$1task.state == TaskState::Running'),
  });
  assert.equal(missingPendingCheck.outcome, 'failed');
  assert.ok(missingPendingCheck.checks.some((item) => item.check.includes('Final task state') && item.status === 'failed'));
  const broaderGrant = exercise('authorization-boundary-auditor', {
    toolHost: host.replace('gate.grant(pending.prepared.scope().clone())', 'gate.grant(unscoped_scope)'),
  });
  assert.equal(broaderGrant.outcome, 'failed');
});

test('secret leakage audit spots synthetic credentials while avoiding sample placeholders', () => {
  const config = readFileSync(join(here, '..', 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8');
  assert.equal(exercise('secret-leakage-detector').outcome, 'passed');
  const embedded = exercise('secret-leakage-detector', {
    tauriConfig: config + '\n"api_key": "' + 'A'.repeat(36) + '"',
  });
  assert.equal(embedded.outcome, 'failed');
  assert.ok(embedded.checks.some((item) => item.check.includes('no high-confidence embedded') && item.status === 'failed'));
  assert.ok(!JSON.stringify(embedded.checks).includes('A'.repeat(36)), 'redacted scan must not include credentials');
});

test('file-action UX audit detects unsafe duplicate submit and silent errors', () => {
  const ui = readFileSync(join(here, '..', 'apps/desktop/src/routes/+page.svelte'), 'utf8');
  assert.equal(exercise('ux-state-completeness-auditor').outcome, 'passed');
  const duplicate = exercise('ux-state-completeness-auditor', {
    ui: ui.replace('disabled={fileToolBusy} onclick={() => void approveFileTool()}', 'onclick={() => void approveFileTool()}'),
  });
  assert.equal(duplicate.outcome, 'failed');
  const silentError = exercise('ux-state-completeness-auditor', {
    ui: ui.replace('role="alert">{fileToolError}', '>{fileToolError}'),
  });
  assert.equal(silentError.outcome, 'failed');
});

test('data consistency exercise executes SQLite rollback and detects stale CAS updates', () => {
  const coreStore = readFileSync(join(here, '..', 'crates/constellation-core/src/store.rs'), 'utf8');
  const workflowStore = readFileSync(join(here, '..', 'crates/constellation-workflows/src/repository.rs'), 'utf8');
  assert.equal(exercise('data-consistency-auditor').outcome, 'passed');
  const noUniqueEvent = exercise('data-consistency-auditor', {
    coreStore: coreStore.replace('operation_id TEXT UNIQUE NOT NULL', 'operation_id TEXT NOT NULL'),
  });
  assert.equal(noUniqueEvent.outcome, 'failed');
  assert.ok(noUniqueEvent.checks.some((check) =>
    check.check.includes('rollback') && check.status === 'failed'));
  const lostCas = exercise('data-consistency-auditor', {
    workflowStore: workflowStore.replace('WHERE id = ?3 AND revision = ?4', 'WHERE id = ?3'),
  });
  assert.equal(lostCas.outcome, 'failed');
  assert.ok(lostCas.checks.some((check) => check.check.includes('stale concurrent') && check.status === 'failed'));
});

test('environment config auditor rejects all-interface defaults, HMR mismatch and credential fallback', () => {
  const viteConfig = readFileSync(join(here, '..', 'apps/desktop/vite.config.js'), 'utf8');
  const native = readFileSync(join(here, '..', 'apps/desktop/src-tauri/src/lib.rs'), 'utf8');
  assert.equal(exercise('environment-config-auditor').outcome, 'passed');
  const exposed = exercise('environment-config-auditor', {
    viteConfig: viteConfig.replace('host: host || "127.0.0.1"', 'host: host || "0.0.0.0"'),
  });
  assert.equal(exposed.outcome, 'failed');
  const wrongHmr = exercise('environment-config-auditor', {
    viteConfig: viteConfig.replace('port: 1421,', 'port: 9000,'),
  });
  assert.equal(wrongHmr.outcome, 'failed');
  const absentSecret = exercise('environment-config-auditor', {
    native: native.replace(
      'std::env::var(variable).map_err(|_| "Configured provider credential is unavailable")',
      'std::env::var(variable).unwrap_or_default()',
    ),
  });
  assert.equal(absentSecret.outcome, 'failed');
});

test('cross-platform readiness detects missing macOS CI, Unix process group and Windows null sink', () => {
  const ci = readFileSync(join(here, '..', '.github/workflows/validate.yml'), 'utf8');
  const toolsExecutor = readFileSync(join(here, '..', 'crates/constellation-tools/src/execution.rs'), 'utf8');
  assert.equal(exercise('cross-platform-readiness-auditor').outcome, 'passed');
  const noMac = exercise('cross-platform-readiness-auditor', {
    ci: ci.replace(
      /(name: Tool library and catalogue validation[\s\S]*?os: \[ubuntu-24\.04, windows-2025), macos-15\]/,
      '$1]',
    ),
  });
  assert.equal(noMac.outcome, 'failed');
  const noUnix = exercise('cross-platform-readiness-auditor', {
    toolsExecutor: toolsExecutor.replaceAll('unix_process::', 'missing_posix_group::'),
  });
  assert.equal(noUnix.outcome, 'failed');
  const noWinSink = exercise('cross-platform-readiness-auditor', {
    toolsExecutor: toolsExecutor.replace(
      '"GIT_CONFIG_GLOBAL",\n                if cfg!(windows) { "NUL" } else { "/dev/null" },',
      '"GIT_CONFIG_GLOBAL",\n                "/dev/null",',
    ),
  });
  assert.equal(noWinSink.outcome, 'failed');
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
