#!/usr/bin/env node
/**
 * Execute bounded, offline Constellation development-skill exercises.
 * The output is a factual receipt of static checks, not a claim of end-to-end
 * runtime or security verification. No source files are modified.
 */
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { win32, posix } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const hash = (data) => createHash('sha256').update(data).digest('hex');
const names = new Set([
  'api-contract-auditor',
  'input-validation-auditor',
  'documentation-accuracy-auditor',
  'dependency-cycle-auditor',
  'completion-claims-auditor',
  'database-schema-reviewer',
  'security-misconfiguration-auditor',
  'agent-tool-trust-auditor',
  'authorization-boundary-auditor',
  'secret-leakage-detector',
  'ux-state-completeness-auditor',
  'data-consistency-auditor',
  'environment-config-auditor',
  'cross-platform-readiness-auditor',
]);
const inputs = {
  ui: 'apps/desktop/src/routes/+page.svelte',
  native: 'apps/desktop/src-tauri/src/lib.rs',
  codec: 'crates/constellation-backup/src/lib.rs',
  cli: 'crates/constellation-backup-cli/src/lib.rs',
  guide: 'docs/getting-started.md',
  qa: 'docs/qa/new-native-feature-verification.md',
  windowsQa: 'docs/qa/windows-native-smoke.md',
  claimsLedger: 'docs/quality/acceptance-ledger.md',
  activePlan: 'docs/mission/active-task-plan.md',
  coreStore: 'crates/constellation-core/src/store.rs',
  workflowStore: 'crates/constellation-workflows/src/repository.rs',
  tauriConfig: 'apps/desktop/src-tauri/tauri.conf.json',
  tauriCapability: 'apps/desktop/src-tauri/capabilities/default.json',
  viteConfig: 'apps/desktop/vite.config.js',
  probeSource: 'crates/constellation-provider-probe/src/lib.rs',
  toolHost: 'apps/desktop/src-tauri/src/tool_host.rs',
  nativeConsent: 'apps/desktop/src-tauri/src/native_consent.rs',
  vault: 'crates/constellation-credential-vault/src/lib.rs',
  toolsExecutor: 'crates/constellation-tools/src/execution.rs',
  ci: '.github/workflows/validate.yml',
};
const read = (path) => readFileSync(resolve(root, path), 'utf8');

function assertObserved(condition, label, path, needle, evidence) {
  const location = needle ? evidence.indexOf(needle) : -1;
  return {
    check: label,
    status: condition ? 'passed' : 'failed',
    source: path,
    line: location < 0 ? null : evidence.slice(0, location).split(/\r?\n/).length,
    detail: condition ? 'Matched source condition' : 'Missing or conflicting source condition',
  };
}

function auditContract(data) {
  const cases = [
    ['studio_search_evidence', ['sourceText', 'query'], ['source_text', 'query']],
    ['studio_create_encrypted_backup', ['passphrase'], ['passphrase']],
    ['studio_restore_encrypted_backup', ['packagePath', 'passphrase'], ['package_path', 'passphrase']],
  ];
  const checks = [];
  const handler = data.native.match(/tauri::generate_handler!\[([\s\S]*?)\]/)?.[1] ?? '';
  for (const [command, clientKeys, nativeKeys] of cases) {
    const invocation = data.ui.match(new RegExp(
      "['\\\"]" + command + "['\\\"]\\s*,\\s*\\{([^}]*)\\}",
    ))?.[1] ?? '';
    const signature = data.native.match(new RegExp(
      'fn\\s+' + command + '\\s*\\(([\\s\\S]*?)\\)\\s*->',
    ))?.[1] ?? '';
    checks.push(assertObserved(
      new RegExp('\\b' + command + '\\b').test(handler),
      `Tauri command registered: ${command}`, inputs.native, command, data.native,
    ));
    checks.push(assertObserved(
      clientKeys.every((key) => new RegExp('\\b' + key + '\\b').test(invocation)),
      `Frontend sends declared arguments: ${command}`, inputs.ui, command, data.ui,
    ));
    checks.push(assertObserved(
      nativeKeys.every((key) => new RegExp('\\b' + key + '\\s*:').test(signature)),
      `Native receives the same arguments: ${command}`, inputs.native, command, data.native,
    ));
  }
  checks.push(assertObserved(
    data.ui.includes('response.model_called !== false || response.stored !== false')
      && data.native.includes('"model_called": false, "stored": false'),
    'Evidence response validates local-only contract on both sides',
    inputs.ui, 'response.model_called !== false', data.ui,
  ));
  checks.push(assertObserved(
    data.ui.includes('result.active_workspace_replaced !== false')
      && data.native.includes('"active_workspace_replaced":false'),
    'Recovery response requires active workspace preservation',
    inputs.ui, 'result.active_workspace_replaced !== false', data.ui,
  ));
  return {
    objective: 'Compare the three new Evidence/backup Tauri IPC producer and consumer contracts, including result assertions',
    checks, limitations: [
      'Static source contract checks do not exercise WebView2, serialization runtime, or native IPC',
      'Additional historic provider and task commands are outside this focused exercise',
    ],
  };
}

function auditValidation(data) {
  const checks = [
    assertObserved(
      data.ui.includes('const maxEvidenceDocumentBytes = 128 * 1024;')
        && data.native.includes('max_document_bytes: 128 * 1024')
        && data.native.includes('max_total_bytes: 128 * 1024'),
      'Document size 128 KiB enforced in UI and Rust index',
      inputs.ui, 'const maxEvidenceDocumentBytes', data.ui,
    ),
    assertObserved(
      data.ui.includes('const maxEvidenceQueryBytes = 2048;')
        && data.native.includes('max_query_bytes: 2048'),
      'Evidence query 2048 UTF-8-byte limit aligned in UI and native',
      inputs.native, 'max_query_bytes: 2048', data.native,
    ),
    assertObserved(
      data.native.includes('max_top_k: 5') && data.native.includes('.search(&query, 5)')
        && data.ui.includes('response.results.length > 5'),
      'Result count limited to five on both sides',
      inputs.native, 'max_top_k: 5', data.native,
    ),
    assertObserved(
      data.ui.includes('const maxBackupPassphraseBytes = 1024;')
        && data.ui.includes('utf8Bytes(backupPassphrase) < 8')
        && data.ui.includes('utf8Bytes(restorePassphrase) < 8')
        && /\(8\.\.=1024\)\.contains\(&passphrase\.len\(\)\)/.test(data.codec),
      '8–1024-byte backup passphrase checked in UI and Rust codec',
      inputs.codec, '(8..=1024).contains', data.codec,
    ),
    assertObserved(
      data.ui.includes('const maxRecoveryPathBytes = 4096;')
        && data.ui.includes('utf8Bytes(restorePackagePath.trim()) > maxRecoveryPathBytes')
        && data.native.includes('checked_local_backup_path(&package_path)?')
        && data.native.includes('path_text.len() > 4096'),
      'Recovery input path bytes bounded by UI and native',
      inputs.native, 'path_text.len() > 4096', data.native,
    ),
    assertObserved(
      data.native.includes('BackupLimits::new(HARD_MAX_PLAINTEXT)')
        && data.codec.includes('pub const HARD_MAX_PLAINTEXT: usize = 64 * 1024 * 1024;'),
      'Native encrypted backup hard cap is 64 MiB',
      inputs.native, 'BackupLimits::new(HARD_MAX_PLAINTEXT)', data.native,
    ),
    assertObserved(
      data.cli.includes('persist_path_noclobber(staged, &output.target)?;')
        && data.cli.includes('check_restore_sidecars(&output.target)?;'),
      'SQLite restore checks stale sidecars then publishes exclusively',
      inputs.cli, 'check_restore_sidecars(&output.target)?;', data.cli,
    ),
  ];
  return {
    objective: 'Inspect user-input size and recovery safety boundaries across Svelte, Tauri, codec and SQLite CLI',
    checks, limitations: [
      'Static source checks cannot prove race freedom or prevent a malicious local filesystem actor',
      'No fuzzer, installed GUI, oversized production database or real secret was used',
    ],
  };
}

function auditDocs(data) {
  const checks = [
    assertObserved(
      data.guide.includes('128 KiB (131,072 UTF-8 bytes)')
        && data.ui.includes('const maxEvidenceDocumentBytes = 128 * 1024;'),
      'Document byte cap in guide agrees with UI',
      inputs.guide, '128 KiB (131,072 UTF-8 bytes)', data.guide,
    ),
    assertObserved(
      data.guide.includes('up to **five** matching passages')
        && data.native.includes('max_top_k: 5'),
      'Guide does not overpromise Evidence hit count',
      inputs.guide, 'up to **five** matching passages', data.guide,
    ),
    assertObserved(
      data.guide.includes('Recovery does not open, merge, or replace the active workspace.')
        && data.native.includes('"active_workspace_replaced":false'),
      'Guide correctly warns recovered file is separate',
      inputs.guide, 'Recovery does not open', data.guide,
    ),
    assertObserved(
      data.guide.includes('backups-and-recovery')
        && data.native.includes('app_data.join("backups-and-recovery")'),
      'Guide describes native app data recovery directory',
      inputs.native, 'app_data.join("backups-and-recovery")', data.native,
    ),
    assertObserved(
      data.guide.includes('**8–1,024 UTF-8 bytes**')
        && data.ui.includes('const maxBackupPassphraseBytes = 1024;'),
      'Guide documents UTF-8 passphrase length, consistent with UI',
      inputs.guide, '**8–1,024 UTF-8 bytes**', data.guide,
    ),
    assertObserved(
      data.guide.includes('**unsigned installed Windows QA build**')
        && data.guide.includes('has not been exercised through the installed GUI')
        && data.qa.includes('**PASS (narrow)** for exact-source query')
        && data.qa.includes('**PARTIAL**: actions and inputs exposed')
        && data.windowsQa.includes('No backup or restore performed on the pre-existing user workspace'),
      'Installed Evidence search is verified narrowly while installed backup/restore remain unexercised',
      inputs.qa, '**PASS (narrow)** for exact-source query', data.qa,
    ),
  ];
  for (const path of [inputs.guide, inputs.qa]) {
    const doc = data[path === inputs.guide ? 'guide' : 'qa'];
    const matches = [...doc.matchAll(/\]\(([^)#]+)(?:#[^)]*)?\)/g)];
    for (const link of matches) {
      if (/^(?:https?:|mailto:)/.test(link[1])) continue;
      const destination = resolve(root, dirname(path), link[1]);
      const exists = (() => {
        try { return readFileSync(destination).length >= 0; } catch { return false; }
      })();
      checks.push(assertObserved(exists, `Relative link resolves: ${link[1]}`,
        path, link[0], doc));
    }
  }
  return {
    objective: 'Validate the updated Constellation Evidence/backup onboarding claims and all its local documentation links',
    checks, limitations: [
      'Only two explicitly identified current docs reviewed; external URLs and screenshots not checked',
      'Claims about actual installed GUI behavior still require separate native Windows acceptance',
    ],
  };
}

/** Inspect claim evidence metadata without granting completion credit to UNVERIFIED rows. */
function inspectClaimsLedger(source) {
  const rows = [];
  const violations = [];
  const seen = new Set();
  for (const line of source.split(/\r?\n/)) {
    if (!/^\| [A-Z][A-Z0-9-]+ \|/.test(line)) continue;
    const cells = line.split('|').slice(1, -1).map((item) => item.trim());
    if (cells.length !== 12) {
      violations.push('Incorrect evidence column count');
      continue;
    }
    const id = cells[0];
    // GUI-XX IDs deliberately repeat across Windows/macOS/Linux/Android.
    const identity = `${id}:${cells[2]}`;
    if (seen.has(identity)) violations.push(`Duplicate evidence row: ${identity}`);
    seen.add(identity);
    const status = cells[7].replaceAll('*', '');
    if (!['UNVERIFIED', 'PASS', 'FAIL', 'PARTIAL', 'BLOCKED'].includes(status)) {
      violations.push(`Invalid evidence status: ${id}`);
    }
    if (status === 'PASS' && [3, 4, 8, 9, 10].some((index) =>
      !cells[index] || ['—', '-', '–', 'not executed'].includes(cells[index].toLowerCase()))) {
      violations.push(`Unsupported PASS without build/environment/log/tester/time: ${id}`);
    }
    rows.push({ id, status });
  }
  return { rows, violations };
}

function auditCompletionClaims(data) {
  const ledger = inspectClaimsLedger(data.claimsLedger);
  const statusCounts = Object.fromEntries(['UNVERIFIED', 'PASS', 'FAIL', 'PARTIAL', 'BLOCKED']
    .map((status) => [status, ledger.rows.filter((row) => row.status === status).length]));
  const planRows = [...data.activePlan.matchAll(/^\| (P\d{2}) \|[^\n]*$/gm)]
    .map((match) => match[1]);
  const allPlanIds = Array.from({ length: 23 }, (_, i) => `P${String(i + 1).padStart(2, '0')}`);
  const tamperedRow = data.claimsLedger.replace(
    /\| \*\*UNVERIFIED\*\* \|/, '| **PASS** |',
  );
  const duplicatedRow = data.claimsLedger.replace(
    /^(\| CORE-01 \|[^\n]*)/m, '$1\n$1',
  );
  const checks = [
    { check: 'Acceptance ledger has at least 180 structured requirement records', status: ledger.rows.length >= 180 ? 'passed' : 'failed', source: inputs.claimsLedger, detail: `${ledger.rows.length} rows parsed` },
    { check: 'All acceptance evidence statuses and required fields are valid', status: ledger.violations.length === 0 ? 'passed' : 'failed', source: inputs.claimsLedger, detail: ledger.violations.join('; ') || JSON.stringify(statusCounts) },
    { check: 'Every delivery milestone P01–P23 has exactly one task-board row', status: planRows.length === 23 && allPlanIds.every((id) => planRows.filter((row) => row === id).length === 1) ? 'passed' : 'failed', source: inputs.activePlan, detail: `${planRows.length} task rows` },
    { check: 'Release and hosted CI explicitly remain unverified', status: data.activePlan.includes('No public release exists yet') && /^\| P22 \|[^|\n]*\| \*\*BLOCKED(?: \([^|\n]*\))?\*\* \|/m.test(data.activePlan) ? 'passed' : 'failed', source: inputs.activePlan, detail: 'Read actual P22 qualified BLOCKED status and release-gate text; no remote check performed' },
    { check: 'Skill/MCP goal remains explicitly qualified as incomplete', status: /^\| P10 \|[^|\n]*\| \*\*IN PROGRESS(?: \/ [^*|\n]+)?\*\* \|/m.test(data.activePlan) ? 'passed' : 'failed', source: inputs.activePlan, detail: 'P10 milestone explicitly remains IN PROGRESS; runtime MCP claims not inspected' },
    { check: 'Auditor rejects fixture claiming PASS without evidence', status: tamperedRow !== data.claimsLedger && inspectClaimsLedger(tamperedRow).violations.some((item) => item.startsWith('Unsupported PASS')) ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Reclassified an unsupported UNVERIFIED record to PASS' },
    { check: 'Auditor rejects duplicate requirement fixture', status: duplicatedRow !== data.claimsLedger && inspectClaimsLedger(duplicatedRow).violations.some((item) => item.startsWith('Duplicate evidence row')) ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Inserted duplicate CORE-01 record' },
  ];
  return {
    objective: 'Audit Constellation requirement evidence rows and delivery completion claims; reject unsupported PASS and duplicate IDs',
    checks,
    limitations: [
      'Audit checks evidence metadata and published milestone wording, not actual installed GUI, CI or remote release status',
      'The acceptance ledger may still contain UNVERIFIED requirements; a passing audit does not fulfill those requirements',
    ],
  };
}

function trySql(test) {
  try { test(); return true; } catch { return false; }
}

function rejectSql(test) {
  try { test(); return false; } catch { return true; }
}

// Core now names the canonical schema rather than embedding it at each
// execute_batch call. Only execute a real referenced DDL literal and require
// that the production connection enables SQLite FK enforcement before BEGIN.
function coreSchemaSql(source) {
  if (!/\.execute_batch\(STORE_SCHEMA_DDL\)/.test(source)
    || !/\.pragma_update\(None,\s*"foreign_keys",\s*"ON"\)/.test(source)) {
    return null;
  }
  return source.match(/const STORE_SCHEMA_DDL:\s*&str\s*=\s*"([\s\S]*?)"\s*;/)?.[1] ?? null;
}

function inspectDatabaseSchemas(coreStore, workflowStore) {
  const coreSql = coreSchemaSql(coreStore);
  const workflowSql = workflowStore.match(/tx\.execute_batch\(\s*"([\s\S]*?)"\s*,?\s*\)/)?.[1];
  if (!coreSql || !workflowSql) return { available: false, assertions: [] };
  const core = new DatabaseSync(':memory:');
  const flow = new DatabaseSync(':memory:');
  try {
    core.exec('PRAGMA foreign_keys = ON;');
    core.exec(coreSql);
    flow.exec(workflowSql);
    const checks = [
      ['Foreign-key enforcement enabled at core connection', core.prepare('PRAGMA foreign_keys').get().foreign_keys === 1],
      ['Core allows valid room and agent identities', trySql(() => {
        core.prepare('INSERT INTO rooms (id,data) VALUES (?,?)').run('r1', '{}');
        core.prepare('INSERT INTO agents (id,data) VALUES (?,?)').run('a1', '{}');
      })],
      ['Core rejects message linked to missing room', rejectSql(() => core.prepare('INSERT INTO messages (id,room_id,data) VALUES (?,?,?)').run('m1', 'nonexistent', '{}'))],
      ['Core accepts message linked to existing room', trySql(() => core.prepare('INSERT INTO messages (id,room_id,data) VALUES (?,?,?)').run('m1', 'r1', '{}'))],
      ['Core rejects task linked to missing agent', rejectSql(() => core.prepare('INSERT INTO tasks (id,room_id,agent_id,data) VALUES (?,?,?,?)').run('t1', 'r1', 'missing', '{}'))],
      ['Core accepts task with valid room and agent', trySql(() => core.prepare('INSERT INTO tasks (id,room_id,agent_id,data) VALUES (?,?,?,?)').run('t1', 'r1', 'a1', '{}'))],
      ['Core rejects duplicate operation ID', trySql(() => core.prepare('INSERT INTO events (operation_id,fingerprint,occurred_at_ms,kind,result) VALUES (?,?,?,?,?)').run('op1', 'f', 1, '{}', '{}')) && rejectSql(() => core.prepare('INSERT INTO events (operation_id,fingerprint,occurred_at_ms,kind,result) VALUES (?,?,?,?,?)').run('op1', 'f', 2, '{}', '{}'))],
      ['Core provides room-indexed message retrieval index', core.prepare('PRAGMA index_list(messages)').all().some((row) => row.name === 'messages_room_index')],
      ['Workflow allows minimally valid bounded record', trySql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf1', 1, 1, Buffer.from('x'), 'a'.repeat(64)))],
      ['Workflow rejects zero revision', rejectSql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf-bad-revision', 0, 1, Buffer.from('x'), 'a'.repeat(64)))],
      ['Workflow rejects unsupported schema version', rejectSql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf-bad-version', 1, 2, Buffer.from('x'), 'a'.repeat(64)))],
      ['Workflow rejects empty and oversized payloads', rejectSql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf-empty', 1, 1, Buffer.alloc(0), 'a'.repeat(64))) && rejectSql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf-huge', 1, 1, Buffer.alloc(524289), 'a'.repeat(64)))],
      ['Workflow rejects malformed hash length and duplicate ID', rejectSql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf-short-hash', 1, 1, Buffer.from('x'), 'short')) && rejectSql(() => flow.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)').run('wf1', 1, 1, Buffer.from('x'), 'a'.repeat(64)))],
    ];
    return { available: true, assertions: checks };
  } catch {
    return { available: false, assertions: [] };
  } finally {
    core.close();
    flow.close();
  }
}

function auditDatabaseSchema(data) {
  const observed = inspectDatabaseSchemas(data.coreStore, data.workflowStore);
  const mutatedCore = data.coreStore.replace('FOREIGN KEY(room_id) REFERENCES rooms(id)', 'CHECK(length(room_id)>0)');
  const mutatedFlow = data.workflowStore.replace('CHECK(revision > 0)', 'CHECK(revision >= 0)');
  const coreFixture = inspectDatabaseSchemas(mutatedCore, data.workflowStore);
  const flowFixture = inspectDatabaseSchemas(data.coreStore, mutatedFlow);
  const checks = [
    { check: 'Real Rust SQLite schema SQL extracted and executed in isolated memory databases', status: observed.available ? 'passed' : 'failed', source: inputs.coreStore, detail: 'Both CREATE TABLE batches and indexes executed through node:sqlite' },
    ...observed.assertions.map(([check, good]) => ({ check, status: good ? 'passed' : 'failed', source: check.startsWith('Core') || check.startsWith('Foreign') ? inputs.coreStore : inputs.workflowStore, detail: good ? 'Observed SQLite operation succeeded/rejected as expected' : 'SQLite constraint/index behavior violated' })),
    { check: 'Removing message foreign-key constraint is detected', status: mutatedCore !== data.coreStore && coreFixture.available && coreFixture.assertions.some(([name, passed]) => name === 'Core rejects message linked to missing room' && !passed) ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Mutated SQLite schema drops one referenced-room constraint' },
    { check: 'Weakening workflow revision constraint is detected', status: mutatedFlow !== data.workflowStore && flowFixture.available && flowFixture.assertions.some(([name, passed]) => name === 'Workflow rejects zero revision' && !passed) ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Mutated SQLite revision CHECK allows zero' },
  ];
  return {
    objective: 'Execute Constellation core and workflow Rust-authored SQLite schemas in memory and exercise relational constraints with valid/invalid records',
    checks,
    limitations: [
      'Node SQLite engine exercises schema SQL only; full rusqlite migrations, transaction concurrency, WAL and end-to-end persistence remain separate tests',
      'Focused constraints and one index tested, not complete query-plan optimization or all domain invariants',
    ],
  };
}

function auditSecurityConfiguration(data) {
  let tauri;
  let capability;
  try {
    tauri = JSON.parse(data.tauriConfig);
    capability = JSON.parse(data.tauriCapability);
  } catch {
    return {
      objective: 'Check effective static Tauri, frontend development, and provider HTTP security defaults',
      checks: [{ check: 'Tauri config and capability JSON parse', status: 'failed', source: inputs.tauriConfig, detail: 'Invalid JSON' }],
      limitations: ['Invalid configuration prevents the rest of the static review'],
    };
  }
  const csp = tauri.app?.security?.csp;
  const rules = Array.isArray(capability.permissions) ? capability.permissions : [];
  const checks = [
    ['CSP constrains default script and resource origin to self', csp?.['default-src'] === "'self'", inputs.tauriConfig],
    ['CSP has no eval, wildcard or remote executable sources', Object.entries(csp ?? {}).every(([directive, value]) => !/\bunsafe-eval\b|\*|https?:\/\//.test(String(value)) || (directive === 'connect-src' && value === 'ipc: http://ipc.localhost')), inputs.tauriConfig],
    ['CSP connect policy allows only Tauri local IPC', csp?.['connect-src'] === 'ipc: http://ipc.localhost', inputs.tauriConfig],
    ['Remote web origins cannot opt into privileged IPC', !JSON.stringify(tauri).includes('dangerousRemoteDomainIpcAccess'), inputs.tauriConfig],
    ['Desktop capability is scoped to main window', JSON.stringify(capability.windows) === '["main"]', inputs.tauriCapability],
    ['Desktop capability does not grant shell, filesystem or broad native plugin access', rules.length > 0 && rules.every((permission) => permission === 'core:default'), inputs.tauriCapability],
    ['Vite development host defaults to loopback with strict port', data.viteConfig.includes('host: host || "127.0.0.1"') && data.viteConfig.includes('strictPort: true'), inputs.viteConfig],
    ['Provider HTTP client disables proxy inheritance and redirects', data.probeSource.includes('.no_proxy()') && data.probeSource.includes('.redirect(reqwest::redirect::Policy::none())'), inputs.probeSource],
  ];
  const weakened = JSON.stringify({ ...tauri, app: { ...tauri.app, security: { csp: { ...csp, 'default-src': "'self' 'unsafe-eval'" } } } });
  const elevated = JSON.stringify({ ...capability, permissions: [...rules, 'shell:allow-execute'] });
  const compromised = (source, field) => auditSecurityConfigurationFixture({ ...data, [field]: source });
  checks.push(['CSP mutation fixture is rejected', weakened !== data.tauriConfig && !compromised(weakened, 'tauriConfig'), 'in-memory negative fixture']);
  checks.push(['Shell permission mutation fixture is rejected', elevated !== data.tauriCapability && !compromised(elevated, 'tauriCapability'), 'in-memory negative fixture']);
  return {
    objective: 'Verify local desktop CSP/permission/host defaults and provider redirect/proxy policy, with unsafe configuration fixtures',
    checks: checks.map(([check, good, source]) => ({ check, status: good ? 'passed' : 'failed', source, detail: good ? 'Observed source configuration satisfies check' : 'Configuration missing or broader than approved scope' })),
    limitations: [
      'Static configuration review only; platform WebView enforcement, network interception and actual permission tests require separately installed application QA',
      'Vite TAURI_DEV_HOST override and CSP style unsafe-inline remain platform-specific considerations; release signing and hosted deployment are unverified',
    ],
  };
}

function auditSecurityConfigurationFixture(data) {
  try {
    const tauri = JSON.parse(data.tauriConfig);
    const capability = JSON.parse(data.tauriCapability);
    const csp = tauri.app?.security?.csp;
    return csp?.['default-src'] === "'self'" &&
      Array.isArray(capability.permissions) &&
      capability.permissions.every((permission) => permission === 'core:default');
  } catch { return false; }
}

function graphCycles(graph) {
  const seen = new Set();
  const stack = new Set();
  const loops = new Set();
  const visit = (node, path) => {
    if (stack.has(node)) {
      const start = path.indexOf(node);
      loops.add([...path.slice(start), node].join(' -> '));
      return;
    }
    if (seen.has(node)) return;
    seen.add(node);
    stack.add(node);
    for (const dep of [...(graph.get(node) ?? [])].sort()) visit(dep, [...path, dep]);
    stack.delete(node);
  };
  for (const node of [...graph.keys()].sort()) visit(node, [node]);
  return [...loops].sort();
}

function auditDependencies() {
  const crates = resolve(root, 'crates');
  const manifests = readdirSync(crates, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(join(crates, entry.name, 'Cargo.toml')))
    .map((entry) => `crates/${entry.name}/Cargo.toml`);
  manifests.push('apps/desktop/src-tauri/Cargo.toml');
  manifests.sort();
  const byFile = new Map();
  const graph = new Map();
  const edges = [];
  const missing = [];
  for (const manifest of manifests) {
    const text = read(manifest);
    const crate = text.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
    if (!crate) missing.push(`no package name in ${manifest}`);
    byFile.set(resolve(root, manifest).toLowerCase(), crate);
    graph.set(crate, []);
  }
  for (const manifest of manifests) {
    const text = read(manifest);
    const source = byFile.get(resolve(root, manifest).toLowerCase());
    for (const line of text.split(/\r?\n/)) {
      const declaration = line.match(/^\s*([\w-]+)\s*=\s*\{(.*)\}\s*$/);
      if (!declaration) continue;
      const local = declaration[2].match(/\bpath\s*=\s*"([^"]+)"/);
      if (!local) continue;
      const manifestTarget = resolve(root, dirname(manifest), local[1], 'Cargo.toml');
      const target = byFile.get(manifestTarget.toLowerCase());
      if (!target) {
        missing.push(`${source} -> ${manifestTarget}`);
        continue;
      }
      graph.get(source).push(target);
      edges.push({ from: source, to: target });
    }
  }
  const cycles = graphCycles(graph);
  const withTestCycle = new Map([...graph].map(([name, deps]) => [name, [...deps]]));
  withTestCycle.set('constellation-core', ['constellation-desktop']);
  const negativeCycles = graphCycles(withTestCycle);
  const checks = [
    { check: 'All Rust manifests declare a package name', status: missing.some((m) => m.startsWith('no package')) ? 'failed' : 'passed', source: 'crates', detail: `${manifests.length} package manifests inspected` },
    { check: 'Path dependencies resolve to an audited local manifest', status: missing.length ? 'failed' : 'passed', source: 'crates', detail: `${edges.length} local edges, ${missing.length} unresolved` },
    { check: 'Local crate dependency graph is acyclic', status: cycles.length ? 'failed' : 'passed', source: 'crates', detail: cycles.join('; ') || 'No cycles among local path dependencies' },
    { check: 'Core does not depend on desktop', status: !(graph.get('constellation-core') ?? []).includes('constellation-desktop') ? 'passed' : 'failed', source: inputs.native, detail: 'Rust architecture layer direction inspected' },
    { check: 'Backup CLI depends on backup codec', status: (graph.get('constellation-backup-cli') ?? []).includes('constellation-backup') ? 'passed' : 'failed', source: 'crates/constellation-backup-cli/Cargo.toml', detail: 'SQLite backup integration edge exists' },
    { check: 'Desktop native crate links core/evidence/backup', status: ['constellation-core', 'constellation-evidence', 'constellation-backup', 'constellation-backup-cli'].every((name) => (graph.get('constellation-desktop') ?? []).includes(name)) ? 'passed' : 'failed', source: 'apps/desktop/src-tauri/Cargo.toml', detail: 'UI host boundary dependencies inspected' },
    { check: 'Cycle detector rejects an introduced core→desktop backedge', status: negativeCycles.length ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: `${negativeCycles.length} cycle(s) in deliberately cyclic test graph` },
  ];
  return {
    objective: 'Audit all Constellation Rust path-dependency edges for actual cycles and enforce core-to-desktop layering',
    checks,
    manifest_inputs: Object.fromEntries(manifests.map((path) =>
      [path, { path, sha256: hash(read(path)) }])),
    graph: { packages: [...graph.keys()].sort(), edges: edges.sort((a, b) => a.from.localeCompare(b.from) || a.to.localeCompare(b.to)), cycles, negative_fixture_cycles: negativeCycles, unresolved: missing },
    limitations: [
      'Only statically declared Cargo path dependencies; dynamically loaded runtime modules or JavaScript imports are outside scope',
      'No cycle is currently present, so no source patch or rebuild was needed',
    ],
  };
}

// An audit recognizes a narrow source contract and demonstrates that removing
// its relevant safeguard changes the outcome. These are static development
// exercises, not a claim that a GUI click or adversarial model was executed.
function fileToolSections(data) {
  const between = (text, start, end) => {
    const position = text.indexOf(start);
    if (position < 0) return '';
    const finish = end ? text.indexOf(end, position + start.length) : -1;
    return text.slice(position, finish < 0 ? undefined : finish);
  };
  return {
    prepare: between(data.native, 'fn studio_prepare_local_file_operation(', '#[tauri::command]\nfn studio_reject_local_file_operation('),
    reserve: between(data.native, 'fn reserve_native_review_if_task_pending(', '#[tauri::command]\nfn studio_approve_local_file_operation('),
    approve: between(data.native, 'fn studio_approve_local_file_operation(', '#[cfg_attr(mobile,'),
    dispatch: between(data.native, 'fn execute_prepared_file_if_task_pending(', 'fn reserve_native_review_if_task_pending('),
    cancel: between(data.native, 'fn studio_cancel_task(', '#[tauri::command]\nfn studio_'),
    hostPrepare: between(data.toolHost, 'pub(crate) fn prepare(', 'pub(crate) fn pending_task('),
    hostReserve: between(data.toolHost, 'pub(crate) fn pending_native_approval(', 'pub(crate) fn deny('),
    hostExecute: between(data.toolHost, 'pub(crate) fn approve_and_execute(', '/// A stable 32-byte key'),
  };
}

function toolTrustConditions(data) {
  const s = fileToolSections(data);
  return [
    ['Native consent fields derive from Rust window and reserved operation', /window\s*\.hwnd\(\)/.test(s.approve) && /host\.pending_native_approval\(operation_id, digest\)/.test(s.reserve) && /create_content: pending\.create_content\.as_deref\(\)/.test(s.approve)],
    ['Native confirmation is required before dispatch', /confirm_native_consent\(owner_hwnd, &native_request\)/.test(s.approve) && /Ok\(Some\(proof\)\)/.test(s.approve) && /with_local_file_host\(&state, \|host\| host\.deny\(operation_id\)\)/.test(s.approve)],
    ['Native YES proof is sealed and bound to operation and digest', /pub\(crate\) struct NativeConsentProof\s*\{\s*operation_id:\s*String,\s*operation_digest:\s*String/.test(data.nativeConsent) && /self\.operation_id == operation_id && self\.operation_digest == operation_digest/.test(data.nativeConsent)],
    ['Independent native owner and actual YES button notification required', /GetWindowThreadProcessId\(owner, &mut pid\)/.test(data.nativeConsent) && /yes_button_clicked\(wparam, lparam/.test(data.nativeConsent) && /expected_button != 0 && lparam == expected_button/.test(data.nativeConsent)],
    ['Tool host limits actions to read and create_new', /"read"\s*=>/.test(s.hostPrepare) && /"create_new"\s*=>/.test(s.hostPrepare) && /_ => return Err\("Unsupported file operation"/.test(s.hostPrepare) && !/"process"\s*=>/.test(s.hostPrepare)],
    ['Prepared content hash is verified against native review', /Sha256::digest\(content\.as_bytes\(\)\)/.test(s.hostReserve) && /sha256\.as_deref\(\) != Some\(actual\.as_str\(\)\)/.test(s.hostReserve)],
    ['Final dispatch requires immutable-digest proof verification and one-use removal', /\.remove\(&operation_id\)/.test(s.hostExecute) && /!native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/.test(s.hostExecute) && /pending\.approval_ticket != expected_digest/.test(s.hostExecute)],
  ];
}

function auditAgentToolTrust(data) {
  const cases = toolTrustConditions(data);
  const withoutConsent = { ...data, native: data.native.replace('confirm_native_consent(owner_hwnd, &native_request)', '/* omitted native consent */') };
  const withoutProof = { ...data, toolHost: data.toolHost.replace(/!native_proof\.verify\(&operation_id\.to_string\(\),\s*pending\.prepared\.digest\(\)\)/, 'false') };
  const withoutButton = { ...data, nativeConsent: data.nativeConsent.replace('expected_button != 0 && lparam == expected_button', 'true') };
  const checks = [
    ...cases.map(([label, pass]) => ({ check: label, status: pass ? 'passed' : 'failed', source: 'native + tool_host + native_consent', detail: pass ? 'Observed trusted boundary in scoped source' : 'Required boundary missing' })),
    { check: 'Removing native confirmation fails the independent trust audit', status: withoutConsent.native !== data.native && !toolTrustConditions(withoutConsent)[1][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Bypassed OS consent before dispatch' },
    { check: 'Removing sealed-proof verification fails the trust audit', status: withoutProof.toolHost !== data.toolHost && !toolTrustConditions(withoutProof)[6][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Removed operation/digest proof verification' },
    { check: 'Accepting any native YES notification fails the trust audit', status: withoutButton.nativeConsent !== data.nativeConsent && !toolTrustConditions(withoutButton)[3][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Eliminated HWND origin validation' },
  ];
  return {
    objective: 'Trace Constellation renderer-to-native tool authority and replay protection; reject simulated bypasses of consent, proof and click origin',
    checks,
    limitations: [
      'Static gate inspection and in-memory source mutations; does not prove physical user input or prevent privileged OS window-message injection',
      'Only the bounded local file action path is audited, not arbitrary agent prompts, external MCPs or browser tools',
    ],
  };
}

function authorizationConditions(data) {
  const s = fileToolSections(data);
  return [
    ['New local file actions use the saved task identity and pending state', /task\.state != TaskState::Pending/.test(s.prepare) && /agent_id: task\.agent_id/.test(s.prepare) && /room_id: task\.room_id/.test(s.prepare)],
    ['Preflight denies stale tasks before native approval reservation', /task\.state == TaskState::Pending/.test(s.reserve) && /host\.deny\(operation_id\)\?/.test(s.reserve) && /host\.pending_native_approval\(operation_id, digest\)/.test(s.reserve)],
    ['Final task state is rechecked at dispatch and stale approval is denied', /task\.state == TaskState::Pending/.test(s.dispatch) && /host\.deny\(operation_id\)\?/.test(s.dispatch)],
    ['Cancellation and approval share the task dispatch lock', /task_tool_boundary/.test(s.cancel) && (s.approve.match(/task_tool_boundary/g) ?? []).length >= 2],
    ['Executor grants only immutable prepared scope', /gate\.grant\(pending\.prepared\.scope\(\)\.clone\(\)\)/.test(s.hostExecute) && /\.approve\(&gate, operation_id, &pending\.prepared\)/.test(s.hostExecute)],
    ['One-use operation ID removed before effect execution', /\.pending\s*\.remove\(&operation_id\)/.test(s.hostExecute) && /\.execute\(/.test(s.hostExecute)],
    ['Rejection path journals denial without execution', /self\.executor\s*\.deny\(operation_id, &pending\.prepared\)/.test(data.toolHost)],
  ];
}

function auditAuthorizationBoundary(data) {
  const cases = authorizationConditions(data);
  const missingFinalGate = { ...data, native: data.native.replace(/(fn execute_prepared_file_if_task_pending\([\s\S]*?)task\.state == TaskState::Pending/, '$1task.state == TaskState::Running') };
  const unscopedGrant = { ...data, toolHost: data.toolHost.replace('gate.grant(pending.prepared.scope().clone())', 'gate.grant(unscoped_scope)') };
  const missingPreflight = { ...data, native: data.native.replace(/(fn reserve_native_review_if_task_pending\([\s\S]*?)task\.state == TaskState::Pending/, '$1task.state == TaskState::Cancelled') };
  return {
    objective: 'Audit two-stage Pending-task authorization, server-derived scope, cancellation lock and one-use local file grants with mutation regressions',
    checks: [
      ...cases.map(([label, pass]) => ({ check: label, status: pass ? 'passed' : 'failed', source: 'native + tool_host', detail: pass ? 'Observed narrowed authorization path' : 'Authorization boundary missing' })),
      { check: 'Pending-to-Running dispatch mutation is detected', status: missingFinalGate.native !== data.native && !authorizationConditions(missingFinalGate)[2][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated stale task dispatch' },
      { check: 'Unscoped permission grant mutation is detected', status: unscopedGrant.toolHost !== data.toolHost && !authorizationConditions(unscopedGrant)[4][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated broad grant' },
      { check: 'Skipped pending preflight mutation is detected', status: missingPreflight.native !== data.native && !authorizationConditions(missingPreflight)[1][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated approval after cancellation' },
    ],
    limitations: [
      'Static authorization contract only: does not execute native dialogs, cross-user roles, access-control tokens or task cancellation races',
      'Validates task-scoped local file actions; multi-tenant authorization is not claimed because this product has no proven tenant model',
    ],
  };
}

function secretSignals(source) {
  const patterns = [
    ['private_key', /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/g],
    ['provider_token', /\b(?:sk-(?:live|test)-|ghp_)[A-Za-z0-9]{20,}/g],
    ['json_credential', /"(?:api[_-]?key|client[_-]?secret|access[_-]?token)"\s*:\s*"(?!\$|\{|<|YOUR_|REPLACE_|example|placeholder)[^"\r\n]{12,}"/gi],
  ];
  const findings = [];
  for (const [kind, regex] of patterns) {
    for (const match of source.matchAll(regex)) {
      findings.push({ kind, line: source.slice(0, match.index).split(/\r?\n/).length });
    }
  }
  return findings;
}

function auditSecretLeakage(data) {
  const scanned = ['tauriConfig', 'tauriCapability', 'viteConfig'].flatMap((key) =>
    secretSignals(data[key]).map((found) => ({ ...found, file: inputs[key] })));
  const synthetic = [
    'const leaked = "sk-live-' + 'A'.repeat(24) + '";',
    '{"api_key":"' + 'A'.repeat(32) + '"}',
    '-----BEGIN PRIVATE KEY-----\n' + 'B'.repeat(32),
  ];
  const risky = synthetic.map((content) => secretSignals(content));
  const benign = secretSignals('{"api_key":"YOUR_API_KEY_HERE"} const token = process.env.API_KEY;');
  const displayed = JSON.stringify(scanned);
  return {
    objective: 'Scan Constellation desktop public configuration for credential literals, verify redacted findings, and reject synthetic private-key/token leaks',
    checks: [
      { check: 'Desktop static configuration has no high-confidence embedded credential literals', status: scanned.length === 0 ? 'passed' : 'failed', source: inputs.tauriConfig, detail: scanned.length ? `${scanned.length} redacted finding(s)` : 'No configured token/private-key pattern found' },
      { check: 'Synthetic API token is flagged', status: risky[0].some((item) => item.kind === 'provider_token') ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Token pattern type and line only' },
      { check: 'Synthetic JSON API key is flagged', status: risky[1].some((item) => item.kind === 'json_credential') ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Credential value is omitted' },
      { check: 'Synthetic PEM private key is flagged', status: risky[2].some((item) => item.kind === 'private_key') ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Key body is omitted' },
      { check: 'Placeholder and environment-based configuration are not treated as committed secrets', status: benign.length === 0 ? 'passed' : 'failed', source: 'in-memory benign fixture', detail: 'Avoids false positive for documented example and environment variable' },
      { check: 'Scanner findings cannot echo a credential value', status: risky.every((items) => items.every((item) => Object.keys(item).join(',') === 'kind,line')) && !displayed.includes('sk-live-') ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Output contains only finding type and line number' },
      { check: 'Provider credential persistence uses OS vault and zeroized session wrappers', status: data.native.includes('use zeroize::Zeroizing;') && data.native.includes('Vault.load(id)') && data.native.includes('Zeroizing::new(secret.expose().to_string())') ? 'passed' : 'failed', source: inputs.native, detail: 'Rust source includes keyring reference and zeroizing wrapper; runtime not exercised' },
    ],
    limitations: [
      'Scan is limited to three known public desktop configuration files, plus a source-code credential-path inspection; it is not a complete repository/history or entropy scanner',
      'No real credentials are created, printed or authenticated; mutation fixtures use synthetic fixed strings',
    ],
  };
}

function uxFileActionConditions(data) {
  const ui = data.ui;
  return [
    ['File action request holds a busy flag through native async result', /async function approveFileTool\(\)/.test(ui) && /fileToolBusy = true;/.test(ui) && /fileToolBusy = false;/.test(ui)],
    ['Approval and rejection controls prevent duplicate requests', /disabled=\{fileToolBusy\} onclick=\{\(\) => void approveFileTool\(\)\}/.test(ui) && /disabled=\{fileToolBusy\} onclick=\{\(\) => void rejectFileTool\(\)\}/.test(ui)],
    ['Reviewed path and exact content appear before native confirmation', /<code>\{fileToolPreview\.canonical_path\}<\/code>/.test(ui) && /<pre>\{fileToolPreview\.reviewed_content\}<\/pre>/.test(ui)],
    ['Preview checks returned task, action and digest before accepting response', /preview\.task_id !== requestedTaskId/.test(ui) && /preview\.action !== fileToolAction/.test(ui) && /preview\.digest/.test(ui)],
    ['Native rejection or uncertain completion leaves visible recovery information', ui.includes('Outcome uncertain or approval rejected.') && /fileToolPreview = null;/.test(ui)],
    ['Validation errors appear in accessible alert and fields retain guarded state', /role="alert">\{fileToolError\}/.test(ui) && /disabled=\{fileToolBusy \|\| !!fileToolPreview\}/.test(ui)],
    ['File actions only rendered for pending tasks', ui.includes("fileToolTaskId === task.id && task.state === 'pending'")],
  ];
}

function auditUxState(data) {
  const cases = uxFileActionConditions(data);
  const missingBusy = { ...data, ui: data.ui.replace('disabled={fileToolBusy} onclick={() => void approveFileTool()}', 'onclick={() => void approveFileTool()}') };
  const missingAlert = { ...data, ui: data.ui.replace('role="alert">{fileToolError}', '>{fileToolError}') };
  const missingTask = { ...data, ui: data.ui.replace('preview.task_id !== requestedTaskId', 'false') };
  return {
    objective: 'Check Constellation local file-action UI review, pending/loading/error states, disabled controls and rejection recovery with targeted regressions',
    checks: [
      ...cases.map(([label, pass]) => ({ check: label, status: pass ? 'passed' : 'failed', source: inputs.ui, detail: pass ? 'Observed file-action UI state contract' : 'Expected user state missing' })),
      { check: 'Removing approval loading guard is detected', status: missingBusy.ui !== data.ui && !uxFileActionConditions(missingBusy)[1][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated repeat approval control' },
      { check: 'Removing accessible error announcement is detected', status: missingAlert.ui !== data.ui && !uxFileActionConditions(missingAlert)[5][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated silent approval error' },
      { check: 'Removing task identity check is detected', status: missingTask.ui !== data.ui && !uxFileActionConditions(missingTask)[3][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated cross-task stale preview' },
    ],
    limitations: [
      'Static Svelte file-action path only; actual screen-reader announcements, keyboard transitions and pending-state visuals require installed GUI tests',
      'Does not assert that every app-wide async state, offline flow or provider error path has been exercised',
    ],
  };
}

function checkAtomicSql(source, weakenedUnique = false) {
  const schema = coreSchemaSql(source);
  if (!schema) return { ready: false };
  const db = new DatabaseSync(':memory:');
  try {
    db.exec(weakenedUnique
      ? schema.replace('operation_id TEXT UNIQUE NOT NULL', 'operation_id TEXT NOT NULL')
      : schema);
    const add = (id, op) => {
      db.exec('BEGIN IMMEDIATE');
      try {
        db.prepare('INSERT INTO rooms (id,data) VALUES (?,?)').run(id, '{}');
        db.prepare('INSERT INTO events (operation_id,fingerprint,occurred_at_ms,kind,result) VALUES (?,?,?,?,?)')
          .run(op, 'fixture', 1, '{}', '{}');
        db.exec('COMMIT');
        return true;
      } catch {
        db.exec('ROLLBACK');
        return false;
      }
    };
    const committed = add('room1', 'op1');
    const replay = add('room2', 'op1');
    const roomCount = db.prepare('SELECT COUNT(*) AS n FROM rooms').get().n;
    const eventCount = db.prepare('SELECT COUNT(*) AS n FROM events').get().n;
    return { ready: true, committed, replay, roomCount, eventCount };
  } catch {
    return { ready: false };
  } finally {
    db.close();
  }
}

function checkWorkflowCas(source) {
  const schema = source.match(/tx\.execute_batch\(\s*"([\s\S]*?)"\s*,?\s*\)/)?.[1];
  const sql = source.match(/"(UPDATE workflow_runs\s+[\s\S]*?WHERE id = \?3(?: AND revision = \?4)?)"/)?.[1];
  if (!schema || !sql) return { ready: false };
  const db = new DatabaseSync(':memory:');
  try {
    db.exec(schema);
    db.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)')
      .run('wf', 1, 1, Buffer.from('initial'), '0'.repeat(64));
    const firstSnapshot = db.prepare('SELECT revision FROM workflow_runs WHERE id=?').get('wf').revision;
    const args = [Buffer.from('new'), '1'.repeat(64), 'wf', firstSnapshot];
    const arity = Math.max(0, ...[...sql.matchAll(/\?(\d+)/g)].map((match) => Number(match[1])));
    const first = db.prepare(sql).run(...args.slice(0, arity)).changes;
    const stale = db.prepare(sql).run(...args.slice(0, arity)).changes;
    const revision = db.prepare('SELECT revision FROM workflow_runs WHERE id=?').get('wf').revision;
    return { ready: true, first, stale, revision };
  } catch {
    return { ready: false };
  } finally {
    db.close();
  }
}

function auditDataConsistency(data) {
  const atomic = checkAtomicSql(data.coreStore);
  const weakened = checkAtomicSql(data.coreStore, true);
  const cas = checkWorkflowCas(data.workflowStore);
  const mutated = checkWorkflowCas(data.workflowStore.replace('WHERE id = ?3 AND revision = ?4', 'WHERE id = ?3'));
  const conditions = [
    ['Core mutation and event share a committed SQLite transaction', /let tx = conn\.transaction_with_behavior\(TransactionBehavior::Immediate\)\?;/.test(data.coreStore) && /record_event\([\s\S]*?tx\.commit\(\)\?;/.test(data.coreStore)],
    ['Duplicate event ID triggers rollback of partial room mutation', atomic.ready && atomic.committed && !atomic.replay && atomic.roomCount === 1 && atomic.eventCount === 1],
    ['Workflow initial CAS succeeds and stale concurrent snapshot fails', cas.ready && cas.first === 1 && cas.stale === 0 && cas.revision === 2],
    ['Relaxing core event uniqueness permits an invalid duplicate', weakened.ready && weakened.replay && weakened.roomCount === 2 && weakened.eventCount === 2],
    ['Removing workflow revision predicate permits a stale second write', mutated.ready && mutated.first === 1 && mutated.stale === 1 && mutated.revision === 3],
  ];
  return {
    objective: 'Execute real Constellation SQLite schema and workflow update SQL for transaction rollback, duplicate operation replay and stale revision interleavings',
    checks: conditions.map(([check, pass], index) => ({
      check, status: pass ? 'passed' : 'failed',
      source: index < 2 || index === 3 ? inputs.coreStore : inputs.workflowStore,
      detail: pass ? 'Observed isolated SQLite commit/replay/CAS outcome' : 'Storage invariant or negative mutation failed',
    })),
    limitations: [
      'Node SQLite exercises extracted production SQL on isolated in-memory tables, not Rust mutex scheduling or multi-process concurrency',
      'Covers two deterministic interleavings and one transaction rollback; full WAL crash recovery and remote synchronization are unverified',
    ],
  };
}

function inspectDevEnv(data) {
  const declarations = [...data.viteConfig.matchAll(/process\.env\.([A-Z][A-Z0-9_]*)/g)]
    .map((hit) => hit[1]);
  const unique = [...new Set(declarations)];
  const fallback = data.viteConfig.match(/host:\s*host\s*\|\|\s*["']([^"']+)["']/)?.[1];
  const hmr = /hmr:\s*host\s*\?\s*\{\s*protocol:\s*["']ws["'],\s*host,\s*port:\s*(\d+)/.exec(data.viteConfig);
  const strict = /strictPort:\s*true/.test(data.viteConfig);
  const keys = [...data.native.matchAll(/std::env::var\(variable\)/g)].length;
  const safeDefault = fallback === '127.0.0.1';
  const evaluate = (value) => ({
    host: value || fallback,
    hmr: value ? { host: value, port: hmr ? Number(hmr[1]) : null } : undefined,
  });
  const base = evaluate(undefined);
  const override = evaluate('192.0.2.3');
  const conditions = [
    ['Vite configuration key discovery is unique and bounded', unique.length === 1 && unique[0] === 'TAURI_DEV_HOST'],
    ['Missing optional host selects loopback and disables remote HMR', safeDefault && base.host === '127.0.0.1' && base.hmr === undefined],
    ['Explicit host and HMR target the same value and fixed websocket port', override.host === '192.0.2.3' && override.hmr?.host === override.host && override.hmr.port === 1421 && strict],
    ['Rust provider environment credentials fail closed when missing', keys > 0 && /std::env::var\(variable\)\.map_err\(\|_\| "Configured provider credential is unavailable"\)/.test(data.native)],
    ['Local process execution clears ambient environment before selected passthrough', /\.env_clear\(\)/.test(data.toolsExecutor) && /if let Some\(path\) = std::env::var_os\("PATH"\)/.test(data.toolsExecutor)],
  ];
  return { conditions, unique };
}

function auditEnvironmentConfig(data) {
  const baseline = inspectDevEnv(data);
  const noFallback = inspectDevEnv({ ...data, viteConfig: data.viteConfig.replace('host: host || "127.0.0.1"', 'host: host || "0.0.0.0"') });
  const wrongHmr = inspectDevEnv({ ...data, viteConfig: data.viteConfig.replace('port: 1421,', 'port: 9142,') });
  const failOpen = inspectDevEnv({ ...data, native: data.native.replace('std::env::var(variable).map_err(|_| "Configured provider credential is unavailable")', 'std::env::var(variable).unwrap_or_default()') });
  const check = [
    ...baseline.conditions.map(([name, ok]) => ({ check: name, status: ok ? 'passed' : 'failed', source: 'vite + native + toolsExecutor', detail: ok ? 'Evaluated configuration branch or source default' : 'Configuration mismatch' })),
    { check: 'Unsafe default all-interface bind mutation is detected', status: !noFallback.conditions[1][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated missing host and all-interface bind' },
    { check: 'Mismatched HMR port mutation is detected', status: !wrongHmr.conditions[2][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated misconfigured websocket port' },
    { check: 'Silent provider credential fallback mutation is detected', status: !failOpen.conditions[3][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Simulated missing env credential returning empty value' },
  ];
  return {
    objective: 'Discover and evaluate Constellation Vite TAURI_DEV_HOST branches, credential fail-closed behavior and subprocess env isolation against bad-configuration mutations',
    checks: check,
    limitations: [
      'The Vite branch is evaluated through a restricted interpreter of its inspected host/HMR expressions; no Vite process or deployed configuration store is started',
      'Only explicitly observed dev-host, credential-reference and process environment controls are covered; no externally provisioned secret is accessed',
    ],
  };
}

function platformMatrix(source) {
  const target = source.match(/name:\s*Tool library and catalogue validation[\s\S]*?matrix:[\s\S]*?os:\s*\[([^\]]+)\]/)?.[1] ?? '';
  return new Set(target.split(',').map((item) => item.trim()).filter(Boolean));
}

function inspectCrossPlatform(data) {
  const matrix = platformMatrix(data.ci);
  const unix = data.toolsExecutor.includes('#[cfg(unix)]') && data.toolsExecutor.includes('unix_process::');
  const windows = data.toolsExecutor.includes('#[cfg(windows)]') && data.toolsExecutor.includes('windows_process::');
  const stableWindows = win32.normalize('C:\\Work Space\\نام\\..\\file.txt') === 'C:\\Work Space\\file.txt';
  const stableUnix = posix.normalize('/tmp/Work Space/نام/../file.txt') === '/tmp/Work Space/file.txt';
  const conditions = [
    ['CI requests pinned Windows, Linux and macOS tool-library jobs', ['ubuntu-24.04', 'windows-2025', 'macos-15'].every((os) => matrix.has(os))],
    ['Platform-specific process cleanup has Windows and Unix branches', unix && windows],
    ['Windows Git config sink avoids POSIX device paths', /"GIT_CONFIG_GLOBAL",\s*if cfg!\(windows\) \{ "NUL" \} else \{ "\/dev\/null" \}/.test(data.toolsExecutor)],
    ['Portable path fixtures preserve spaces and Unicode for both OS conventions', stableWindows && stableUnix],
    ['CI invokes the Unix and Windows descendant regression suites', /--test unix_process_groups/.test(data.ci) && /--test windows_process_tree/.test(data.ci)],
  ];
  return conditions;
}

function auditCrossPlatform(data) {
  const noUnixRunner = { ...data, ci: data.ci.replace(
    /(name: Tool library and catalogue validation[\s\S]*?os: \[ubuntu-24\.04, windows-2025), macos-15\]/,
    '$1]',
  ) };
  // The tool-library matrix is a separate target from the other workflows.
  const noUnixBranch = { ...data, toolsExecutor: data.toolsExecutor.replaceAll('unix_process::', 'removed_posix_group::') };
  const wrongNull = { ...data, toolsExecutor: data.toolsExecutor.replace(
    '"GIT_CONFIG_GLOBAL",\n                if cfg!(windows) { "NUL" } else { "/dev/null" },',
    '"GIT_CONFIG_GLOBAL",\n                "/dev/null",',
  ) };
  const conditions = inspectCrossPlatform(data);
  return {
    objective: 'Cross-check Constellation multi-OS CI, platform cleanup branches and OS-specific process environment with Unicode/space path fixtures and regressions',
    checks: [
      ...conditions.map(([name, ok]) => ({ check: name, status: ok ? 'passed' : 'failed', source: 'CI + toolsExecutor', detail: ok ? 'Observed targeted source and portable path fixture' : 'Platform assumption missing' })),
      { check: 'Missing macOS tool runner is detected', status: !inspectCrossPlatform(noUnixRunner)[0][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'CI matrix lost macos-15' },
      { check: 'Removing Unix descendant cleanup is detected', status: !inspectCrossPlatform(noUnixBranch)[1][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'POSIX process group cleanup removed' },
      { check: 'Using POSIX device for Windows Git configuration is detected', status: !inspectCrossPlatform(wrongNull)[2][1] ? 'passed' : 'failed', source: 'in-memory negative fixture', detail: 'Replaced OS conditional with /dev/null' },
    ],
    limitations: [
      'Windows-host Node path fixtures and source/CI review only; no macOS or Linux runner was executed here',
      'CI job definitions do not attest to hosted CI outcomes, installed packages or real cross-platform GUI behavior',
    ],
  };
}

export function exercise(name, overrides = {}) {
  if (!names.has(name)) throw new Error('Unknown or unapproved development skill exercise');
  const files = Object.fromEntries(Object.entries(inputs).map(([key, path]) =>
    [key, Object.hasOwn(overrides, key) ? overrides[key] : read(path)]));
  const result = name === 'api-contract-auditor'
    ? auditContract(files)
    : name === 'input-validation-auditor'
      ? auditValidation(files)
      : name === 'documentation-accuracy-auditor'
        ? auditDocs(files)
        : name === 'dependency-cycle-auditor' ? auditDependencies()
          : name === 'completion-claims-auditor' ? auditCompletionClaims(files)
            : name === 'database-schema-reviewer' ? auditDatabaseSchema(files)
              : name === 'security-misconfiguration-auditor' ? auditSecurityConfiguration(files)
                : name === 'agent-tool-trust-auditor' ? auditAgentToolTrust(files)
                  : name === 'authorization-boundary-auditor' ? auditAuthorizationBoundary(files)
                    : name === 'secret-leakage-detector' ? auditSecretLeakage(files)
                      : name === 'ux-state-completeness-auditor' ? auditUxState(files)
                        : name === 'data-consistency-auditor' ? auditDataConsistency(files)
                          : name === 'environment-config-auditor' ? auditEnvironmentConfig(files)
                            : auditCrossPlatform(files);
  return {
    id: `skill:${name}`,
    project: 'Constellation',
    command: `node skills-manifest/exercise-local.mjs ${name} --write-receipt`,
    outcome: result.checks.every((check) => check.status === 'passed') ? 'passed' : 'failed',
    objective: result.objective,
    checks: result.checks,
    limitations: result.limitations,
    ...(result.graph ? { graph: result.graph } : {}),
    inputs: result.manifest_inputs ?? Object.fromEntries(
      (name === 'data-consistency-auditor' ? ['coreStore', 'workflowStore']
        : name === 'environment-config-auditor' ? ['viteConfig', 'native', 'toolsExecutor']
          : name === 'cross-platform-readiness-auditor' ? ['ci', 'toolsExecutor']
            : name === 'agent-tool-trust-auditor' ? ['native', 'toolHost', 'nativeConsent']
        : name === 'authorization-boundary-auditor' ? ['native', 'toolHost']
          : name === 'secret-leakage-detector' ? ['native', 'tauriConfig', 'tauriCapability', 'viteConfig']
            : name === 'ux-state-completeness-auditor' ? ['ui']
              : name === 'api-contract-auditor' ? ['ui', 'native']
        : name === 'input-validation-auditor' ? ['ui', 'native', 'codec', 'cli']
          : name === 'documentation-accuracy-auditor' ? ['ui', 'native', 'guide', 'qa', 'windowsQa']
            : name === 'completion-claims-auditor' ? ['claimsLedger', 'activePlan']
              : name === 'database-schema-reviewer' ? ['coreStore', 'workflowStore']
                : ['tauriConfig', 'tauriCapability', 'viteConfig', 'probeSource'])
        .map((key) => [key, { path: inputs[key], sha256: hash(files[key]) }]),
    ),
    installed_source_sha256: hash(readFileSync(resolve(
      process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation', name, 'SKILL.md',
    ))),
    exercise_script_sha256: hash(readFileSync(fileURLToPath(import.meta.url))),
  };
}

function main() {
  const [, , name, flag] = process.argv;
  if (!names.has(name) || (flag && flag !== '--write-receipt')
    || process.argv.length > 4) {
    process.stderr.write('Usage: node skills-manifest/exercise-local.mjs <approved-skill-name> [--write-receipt]\n');
    process.exitCode = 2;
    return;
  }
  const result = exercise(name);
  const failures = result.checks.filter((check) => check.status !== 'passed');
  process.stdout.write(JSON.stringify({
    id: result.id, outcome: result.outcome,
    passed: result.checks.length - failures.length,
    failed: failures.length,
    failures: failures.map((check) => check.check),
  }, null, 2) + '\n');
  if (failures.length) {
    process.exitCode = 1;
    return;
  }
  if (flag === '--write-receipt') {
    const location = join(here, 'evidence', name + '.json');
    mkdirSync(dirname(location), { recursive: true });
    const receipt = {
      ...result,
      observed_on: new Date().toISOString().slice(0, 10),
      scope: 'static_source_contract',
      // This receipt proves the script ran and the recorded source state; it
      // does not claim live tool execution, hidden agent work or GUI testing.
    };
    writeFileSync(location, JSON.stringify(receipt, null, 2) + '\n', { flag: 'w' });
    process.stdout.write(`Receipt: ${relative(root, location)} SHA-256 ${hash(readFileSync(location))}\n`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
