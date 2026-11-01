#!/usr/bin/env node
/**
 * Reproducible offline audits for source-reviewed, NOT installed skills.
 * Independent from the qualified exercise runner: existing receipts and their
 * pinned runner SHA remain untouched. These receipts never grant vetted credit.
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { DatabaseSync } from 'node:sqlite';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const upstream = resolve(repo, '..', 'vibe-coding-skills');
const sha = (value) => createHash('sha256').update(value).digest('hex');
const norm = (buffer) => Buffer.from(buffer.toString('utf8').replaceAll('\r\n', '\n'));
const paths = Object.freeze({
  'injection-risk-auditor': ['security', 'crates/constellation-core/src/store.rs'],
  'migration-safety-reviewer': ['backend', 'crates/constellation-workflows/src/repository.rs'],
  'requirement-traceability-auditor': ['agent-reliability', 'docs/quality/acceptance-ledger.md'],
  'scope-drift-guard': ['agent-reliability', 'AGENTS.md'],
  'dependency-grounding-auditor': ['dependencies', 'apps/desktop/package.json'],
  'accessibility-auditor': ['frontend', 'apps/desktop/src/routes/+page.svelte'],
  'responsive-design-auditor': ['frontend', 'apps/desktop/src/routes/studio.css'],
  'test-quality-auditor': ['testing', 'apps/desktop/src-tauri/src/tool_host.rs'],
});
const list = Object.keys(paths);

const check = (name, ok, detail) => ({
  name,
  result: ok ? 'pass' : 'fail',
  detail,
});

function injectionAudit(source) {
  const queries = [...source.matchAll(/"(SELECT 1 FROM (?:rooms|agents|tasks|messages) WHERE id = [^"]+)"/g)]
    .map((match) => match[1]);
  const dynamic = source.match(/fn id_exists\([\s\S]*?\n}\n/)?.[0] ?? '';
  const preparedCall = /\.query_row\(sql, \[id\], \|row\|/.test(dynamic);
  const allowlisted = /"rooms" =>/.test(dynamic) && /"agents" =>/.test(dynamic)
    && /"tasks" =>/.test(dynamic) && /"messages" =>/.test(dynamic)
    && /_ => return Err\(CoreError::InvalidInput\("unknown table"/.test(dynamic);
  const db = new DatabaseSync(':memory:');
  let parameterized = false;
  let injectedWouldBypass = false;
  let noMutation = false;
  try {
    db.exec('CREATE TABLE rooms(id TEXT PRIMARY KEY); INSERT INTO rooms VALUES (\'known\');');
    const malicious = "' OR 1=1 --";
    const sql = queries.find((value) => value.includes('FROM rooms'));
    if (sql) {
      try {
        parameterized = db.prepare(sql).get(malicious) === undefined
          && db.prepare(sql).get('known')?.[1] === 1;
      } catch {
        parameterized = false; // A mutated SQL signature is a failed audit, not a runner crash.
      }
      injectedWouldBypass = db.prepare(
        "SELECT 1 FROM rooms WHERE id = '" + malicious + "'",
      ).get()?.[1] === 1;
      noMutation = db.prepare('SELECT count(*) AS n FROM rooms').get().n === 1;
    }
  } finally {
    db.close();
  }
  return {
    purpose: 'Trace actual Rust ID lookup from bounded table allowlist to SQLite parameter binding; execute benign SQL injection and unsafe negative fixture',
    checks: [
      check('Four table lookups remain parameterized', queries.length === 4
        && queries.every((sql) => sql.endsWith('id = ?1')), JSON.stringify(queries)),
      check('Actual Rust query binds caller-provided ID', preparedCall, 'query_row(sql, [id])'),
      check('User input cannot choose arbitrary SQL table', allowlisted, 'id_exists table allowlist'),
      check('In-memory SQLite rejects SQL syntax in bound ID', parameterized, 'one valid and one malicious SELECT'),
      check('Unsafe interpolation fixture demonstrably bypasses equality', injectedWouldBypass, 'synthetic attacker-controlled WHERE clause'),
      check('Synthetic injection does not modify database', noMutation, 'one row remains'),
    ],
    limitations: ['Runs extracted SQLite statements, not Rust runtime dataflow or an end-to-end API penetration test',
      'Only ID lookup source-to-sink path reviewed; other SQL and process sinks are out of scope'],
  };
}

function migrationAudit(source) {
  const ddl = source.match(/tx\.execute_batch\(\s*"(CREATE TABLE IF NOT EXISTS workflow_runs[\s\S]*?)"\s*,?\s*\)/)?.[1] ?? '';
  const guard = source.includes('if version != 0 && version != DB_VERSION')
    && source.includes('if existing_tables != 0')
    && source.includes('tx.pragma_update(None, "user_version", DB_VERSION)')
    && source.includes('TransactionBehavior::Immediate');
  const dangerous = /\bDROP\s+(?:TABLE|COLUMN)\b|\bDELETE\s+FROM\b|\bALTER\s+TABLE\b[\s\S]*?\bRENAME\b/i;
  const noDestruction = ddl.length > 100 && !dangerous.test(ddl);
  const catchesDestruction = dangerous.test(ddl + '\nDROP TABLE workflow_runs;');
  let forward = false;
  let rollback = false;
  let preserved = false;
  let rejectedBadRevision = false;
  const db = new DatabaseSync(':memory:');
  try {
    if (ddl) {
      db.exec(ddl);
      const id = 'fixture-workflow';
      db.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)')
        .run(id, 1, 1, Buffer.from('snapshot'), 'a'.repeat(64));
      db.exec('BEGIN IMMEDIATE');
      db.exec('ALTER TABLE workflow_runs ADD COLUMN review_tag TEXT');
      db.exec('ROLLBACK');
      rollback = db.prepare('PRAGMA table_info(workflow_runs)').all()
        .every((column) => column.name !== 'review_tag');
      db.exec('BEGIN IMMEDIATE');
      db.exec('ALTER TABLE workflow_runs ADD COLUMN review_tag TEXT');
      db.exec('COMMIT');
      forward = db.prepare('PRAGMA table_info(workflow_runs)').all()
        .some((column) => column.name === 'review_tag');
      const row = db.prepare('SELECT payload,revision FROM workflow_runs WHERE id=?').get(id);
      preserved = Buffer.compare(row.payload, Buffer.from('snapshot')) === 0 && row.revision === 1;
      try {
        db.prepare('INSERT INTO workflow_runs (id,revision,schema_version,payload,sha256) VALUES (?,?,?,?,?)')
          .run('bad-revision', 0, 1, Buffer.from('x'), 'a'.repeat(64));
      } catch {
        rejectedBadRevision = true;
      }
    }
  } finally {
    db.close();
  }
  return {
    purpose: 'Classify production workflow SQL and exercise a real forward/rollback SQLite DDL migration with preserved bytes and negative records',
    checks: [
      check('Production workflow schema contains no destructive migration', noDestruction, 'DDL checked against destructive SQL operations'),
      check('Unknown version and foreign schema guarded in transactional migration', guard, 'source version/transaction checks'),
      check('Classifier rejects destructive synthetic fixture', catchesDestruction, 'DROP TABLE injected only in memory'),
      check('SQLite schema change rolls back transactionally', rollback, 'rolled-back ADD COLUMN absent'),
      check('SQLite additive migration commits', forward, 'review_tag column exists after COMMIT'),
      check('Existing persisted payload remains unchanged', preserved, 'original payload and revision'),
      check('Revision integrity constraint survives migration', rejectedBadRevision, 'zero revision rejected'),
    ],
    limitations: ['In-memory SQLite DDL and SQL reviewed; production file locking, backfills, large data, and concurrent mixed-version deployment untested',
      'No upgrade from prior shipped schema or production rollback is claimed'],
  };
}

function traceabilityAudit(source) {
  const rows = source.split(/\r?\n/).filter((line) => /^\| [A-Z][A-Z0-9-]*-[0-9]+ \|/.test(line));
  const parsed = rows.map((row) => row.split('|').slice(1, -1).map((cell) => cell.trim()));
  const keys = parsed.map((cells) => `${cells[0]}:${cells[2]}`);
  const duplicate = keys.length !== new Set(keys).size;
  const required = parsed.every((cells) => cells.length === 12 && cells[0] && cells[1] && cells[2]);
  const invalidStatus = parsed.filter((cells) => !/^(?:\*\*)?(UNVERIFIED|PASS|FAIL|PARTIAL|BLOCKED)(?:\*\*)?$/.test(cells[7]));
  const accepted = parsed.filter((cells) => cells[7].includes('PASS'));
  const hasEvidence = accepted.every((cells) => [3,4,8,9,10].every((idx) => cells[idx] && !['—','-','–'].includes(cells[idx])));
  const completeCoverage = (cells) => cells.length === 188
    && new Set(cells.map((item) => `${item[0]}:${item[2]}`)).size === 188;
  const shadow = (row) => [...parsed, [...row]];
  const duplicateCaught = !completeCoverage(shadow(parsed[0]));
  const omissionCaught = !completeCoverage(parsed.slice(1));
  const unsupportedPass = [...parsed[0]];
  unsupportedPass[7] = '**PASS**';
  const forgedPassCaught = [3,4,8,9,10].some((idx) => !unsupportedPass[idx] || ['—','-','–'].includes(unsupportedPass[idx]));
  return {
    purpose: 'Build platform-scoped requirement coverage matrix from actual acceptance ledger; detect missing columns, duplicate keys and unsupported PASS with adversarial fixtures',
    checks: [
      check('Parsed all 188 documented requirements into platform-scoped IDs', parsed.length === 188, `${parsed.length} rows`),
      check('All rows preserve complete evidence schema', required, '12 columns per row'),
      check('No repeated requirement/platform combination', !duplicate, `${keys.length} scoped keys`),
      check('All requirement statuses are recognized', invalidStatus.length === 0, `${invalidStatus.length} unknown statuses`),
      check('No PASS without environment, proof and tester', hasEvidence, `${accepted.length} explicitly PASS rows`),
      check('Duplicate scoped requirement fixture detected', duplicateCaught, 'duplicated first entry'),
      check('Omitted requirement fixture detected', omissionCaught, 'row count mismatch'),
      check('Evidence-free PASS fixture rejected', forgedPassCaught, 'synthetic unsupported PASS'),
    ],
    limitations: ['Audits acceptance ledger metadata completeness, not semantic coverage of original 1,237-line mission',
      'Actual runtime/build/test observations require their own independent evidence and cannot be inferred from the matrix'],
  };
}

function scopeAudit(source) {
  const rules = [
    'Do not change sibling repositories',
    'Do not print secrets',
    'Preserve repo/history',
    'Keep code changes reviewable',
  ];
  const present = rules.map((rule) => source.toLowerCase().includes(rule.toLowerCase()));
  // A deterministic synthetic delta against the current exact worker scope,
  // so an unrelated concurrent worker's changes are not misattributed.
  const permitted = ['skills-manifest/validate.test.mjs', 'skills-manifest/evidence/reviewed/example.json',
    'docs/qa/skills-expansion-2026-10-30.md'];
  const forbidden = ['apps/desktop/src-tauri/src/lib.rs', 'docs/mission/active-task-plan.md',
    '../vibe-coding-skills/LICENSE', 'crates/constellation-sync/src/lib.rs'];
  const owner = (path) => (path.startsWith('skills-manifest/') ||
    path === 'docs/qa/skills-expansion-2026-10-30.md') && !path.includes('..');
  const detectsDrift = forbidden.every((path) => !owner(path));
  const rejectsTraversal = !owner('skills-manifest/../apps/desktop/src/routes/+page.svelte');
  const rejectsAbsolute = !owner('/etc/passwd') && !owner('C:\\Windows\\System32\\drivers\\etc\\hosts')
    && !owner('skills-manifest/../../researchweave/src/lib.rs');
  return {
    purpose: 'Apply real repository ownership instructions to a scoped, synthetic changed-file set; reject adjacent workspace and traversal edits',
    checks: [
      check('All required preservation/secret/repository rules found', present.every(Boolean), JSON.stringify(rules)),
      check('Approved worker paths classified in scope', permitted.every(owner), JSON.stringify(permitted)),
      check('Unrelated app, docs, sibling repo and crate paths classified out of scope', detectsDrift, JSON.stringify(forbidden)),
      check('Path traversal cannot bypass ownership classification', rejectsTraversal, 'synthetic traversal path'),
      check('Absolute and cross-repository paths cannot be authorized', rejectsAbsolute, 'Unix, Windows and sibling-repository path fixtures'),
    ],
    limitations: ['Uses synthetic scope paths and repository instructions; does not inspect/revert actual concurrent users\' changes',
      'No source edits, git resets or process executions are undertaken by the exercise'],
  };
}

function dependencyAudit(source, inputs) {
  let manifest = null;
  let installedTauri = null;
  let installedSvelte = null;
  try {
    manifest = JSON.parse(source);
    installedTauri = JSON.parse(inputs.tauriApi);
    installedSvelte = JSON.parse(inputs.svelte);
  } catch { /* Malformed manifests must fail the recorded checks. */ }
  const expected = {...manifest?.dependencies, ...manifest?.devDependencies};
  const importer = inputs.pnpmLock.match(/\n  \.:\r?\n([\s\S]*?)\npackages:/)?.[1] ?? '';
  const locked = new Map();
  for (const row of importer.matchAll(/^      ('[^']+'|[^:\r\n]+):\r?\n        specifier: ([^\r\n]+)\r?\n        version: ([^\r\n]+)/gm)) {
    locked.set(row[1].replace(/^'|'$/g, ''), {specifier:row[2], version:row[3].split('(')[0]});
  }
  const direct = Object.entries(expected);
  const parse = (version) => /^([0-9]+)\.([0-9]+)\.([0-9]+)$/.exec(version)?.slice(1).map(Number);
  const compatible = (spec, version) => {
    const v = parse(version);
    const s = /^(\^|~)?(\d+)(?:\.(\d+))?(?:\.(\d+))?$/.exec(spec ?? '');
    if (!v || !s) return false;
    const a = [Number(s[2]), Number(s[3] ?? 0), Number(s[4] ?? 0)];
    const atLeastMinimum = v[0] > a[0] || v[0] === a[0] && (v[1] > a[1] || v[1] === a[1] && v[2] >= a[2]);
    return atLeastMinimum && (s[1] === '~' ? v[0] === a[0] && v[1] === a[1]
      : s[1] === '^' ? v[0] === a[0] : v.every((part,i) => part === a[i]));
  };
  const packages = inputs.pnpmLock.split('\npackages:\n')[1]?.split('\nsnapshots:\n')[0] ?? '';
  const packageRows = [...packages.matchAll(/^  ('[^']+'|[^:\r\n]+):\r?$/gm)]
    .map((m) => m[1].replace(/^'|'$/g, ''));
  const rustDep = (name) => inputs.cargoManifest.match(new RegExp(`^${name} = (?:\\{ version = )?"([0-9][^"]*)"`, 'm'))?.[1];
  const rustCompatible = (spec, version) => {
    const parsed = parse(version);
    const base = spec?.split('.').map(Number);
    return !!parsed && !!base && compatible('^' + spec, version)
      && (base[0] !== 0 || base.length < 2 || parsed[1] === base[1]);
  };
  const crates = [...inputs.cargoLock.matchAll(/\[\[package\]\]\r?\nname = "([^"]+)"\r?\nversion = "([^"]+)"\r?\nsource = "([^"]+)"\r?\nchecksum = "([a-f0-9]{64})"/g)];
  const cargo = ['tauri','serde','reqwest','keyring'].map((name) => ({
    name, spec: rustDep(name), found: crates.filter((m) => m[1] === name),
  }));
  const expectedNames = Object.keys(expected);
  const installerCompatible = installedTauri?.name === '@tauri-apps/api'
    && compatible(expected['@tauri-apps/api'], installedTauri?.version)
    && installedTauri.version === locked.get('@tauri-apps/api')?.version;
  const svelteCompatible = installedSvelte?.name === 'svelte'
    && compatible(expected.svelte, installedSvelte?.version)
    && installedSvelte.version === locked.get('svelte')?.version;
  return {
    purpose: 'Ground actual desktop JS/Rust direct dependencies in pinned lock records and installed package metadata, with synthetic stale and invented-version controls',
    checks: [
      check('Desktop importer matches all declared direct dependency names', expectedNames.length >= 10
        && expectedNames.length === locked.size && expectedNames.every((name) => locked.has(name)),
      `${expectedNames.length} declared versus ${locked.size} locked`),
      check('Every pnpm direct specifier matches its manifest declaration', direct.every(([name,spec]) => locked.get(name)?.specifier === spec), 'actual package.json and importer specs'),
      check('All resolved direct npm versions fit declared semver constraints', direct.every(([name,spec]) => compatible(spec, locked.get(name)?.version)), 'resolved importer versions'),
      check('Every npm direct package version exists in locked package records', direct.every(([name]) =>
        packageRows.some((row) => row.startsWith(`${name}@${locked.get(name)?.version}`))), 'lockfile packages evidence'),
      check('Actual installed Tauri API metadata matches resolved version', installerCompatible,
        `${installedTauri?.version ?? 'missing'} installed`),
      check('Actual installed Svelte metadata matches resolved version', svelteCompatible,
        `${installedSvelte?.version ?? 'missing'} installed`),
      check('Rust desktop declares Tauri, serde, reqwest and keyring', cargo.every((c) => !!c.spec), 'four Cargo direct dependency specs'),
      check('Cargo.lock resolves each selected Rust dependency with registry source and checksum', cargo.every((c) =>
        c.found.some((m) => rustCompatible(c.spec, m[2]) && m[3] === 'registry+https://github.com/rust-lang/crates.io-index')),
      `${cargo.map((c) => `${c.name}:${c.found.map((m) => m[2]).join(',')}`).join('; ')}`),
      check('Tauri native and JavaScript bridge have compatible major versions',
        rustDep('tauri') === '2' && expected['@tauri-apps/api'] === '^2'
          && expected['@tauri-apps/cli'] === '^2', 'JS/native Tauri v2 contract'),
      check('Synthetic missing npm package is rejected', !compatible('^2', '99.0.0')
        && !compatible('^2', 'invalid'), 'invented 99.0.0 and malformed version'),
      check('Synthetic incompatible semver and stale patch are rejected', !compatible('~6.0.3', '6.1.0')
        && !compatible('~6.0.3', '6.0.2'), 'TypeScript minor/patch boundary'),
    ],
    limitations: ['Uses local package.json, pnpm lockfile, Cargo manifest/lock and installed package metadata; no registry API, vulnerability lookup or remote publisher check',
      'Selected Rust crates and two installed JS packages are tested; other transitive packages, imports and runtime behavior remain outside this focused audit'],
  };
}

// These three audits deliberately inspect different behavior families:
// accessible names/interaction semantics, narrow-viewport CSS reflow, and
// executable negative assertions. They are source audits, NOT GUI/WCAG proofs.
function accessibilityAudit(ui, css) {
  const namedRail = /<nav\s+class="rail"\s+aria-label="Main navigation"/.test(ui)
    && ['Home', 'Rooms', 'Tasks', 'Evidence', 'Library', 'Settings']
      .every((name) => ui.includes(`aria-label="${name}"`));
  const modal = ui.includes('role="dialog" aria-modal="true"')
    && ui.includes('use:focusDialog') && ui.includes('aria-label="Close dialog"');
  const announcements = ui.includes('class="toast" role="status"')
    && ui.includes('class="error-toast" role="alert"')
    && ui.includes('role="alert">{fileToolError}');
  const keyboard = ui.includes('event.isComposing')
    && ui.includes('event.keyCode !== 229')
    && ui.includes('<svelte:window onkeydown={keyboard} />');
  const focus = css.includes('button:focus-visible, input:focus-visible')
    && css.includes('outline:3px solid')
    && css.includes('@media(forced-colors:active)');
  const reduced = css.includes('@media(prefers-reduced-motion:reduce)')
    && css.includes('animation:none!important');
  const findings = [
    check('Named navigation and six icon-only destinations', namedRail, 'Svelte rail control accessible names'),
    check('Native modal role, owner name, and focus action', modal, 'Svelte dialog semantics and focus'),
    check('Errors and success exposed by distinct live semantics', announcements, 'alert and status roles'),
    check('Keyboard shortcuts guard active IME composition', keyboard, 'Svelte keyboard and IME'),
    check('Visible focus and forced-colors rules retained', focus, 'CSS focus-visible/contrast'),
    check('Reduced-motion preference suppresses animation', reduced, 'CSS motion override'),
  ];
  return {purpose:'Review semantic keyboard, modal, alert and focus contracts of real Constellation UI with independent negative Svelte/CSS mutations',checks:findings,
    limitations:['Source semantics only: keyboard traversal, screen reader announcements, contrast ratios, touch and actual GUI were not exercised',
      'This is not WCAG 2.2 compliance evidence; dynamic focus behavior and native screen reader tests remain pending']};
}

function responsiveAudit(css) {
  const widths = [1050,760,520].every((n) => css.includes(`@media(max-width:${n}px)`));
  const mobile = css.includes('.mobile-menu{display:grid;}')
    && css.includes('.sidebar.open{transform:translateX(0);}')
    && css.includes('.sidebar:not(.open){visibility:hidden;pointer-events:none;}');
  const grids = css.includes('.action-grid,.roles-grid{grid-template-columns:1fr;}')
    && css.includes('.workflow-build-header,.workflow-step-selects{grid-template-columns:1fr;}');
  const form = css.includes('.evidence-search-row{flex-direction:column;align-items:stretch;}')
    && css.includes('.backup-form button[type="submit"]{width:100%;white-space:normal;');
  const mainRule = css.match(/(?:^|\n)\.main\s*\{([^}]+)\}/)?.[1] ?? '';
  const modalRule = css.match(/(?:^|\n)\.modal\s*\{([^}]+)\}/)?.[1] ?? '';
  const roots = /\bbody\s*\{[^}]*\bmin-width:\s*320px\s*;/.test(css)
    && /\bdisplay:\s*flex\s*;/.test(mainRule)
    && /\bflex-direction:\s*column\s*;/.test(mainRule)
    && /\bflex:\s*1\s*;/.test(mainRule)
    && /\bmin-width:\s*0\s*;/.test(mainRule);
  const modal = /\bwidth:\s*min\(100%,\s*465px\)\s*;/.test(modalRule)
    && /\bmax-height:\s*90dvh\s*;/.test(modalRule)
    && /\boverflow-y:\s*auto\s*;/.test(modalRule);
  const wraps = css.includes('overflow-wrap:anywhere') && css.includes('word-break:break-all;');
  return {purpose:'Inspect concrete 320/520/760/1050px responsive stylesheet contracts for layout, hidden-menu keyboard safety, form reflow and text overflow',checks:[
    check('Medium/tablet/narrow breakpoint declarations present',widths,'1050, 760 and 520px layout rules'),
    check('Narrow menu remains operable, closed nav is untabbable',mobile,'off-canvas mobile menu and visibility rules'),
    check('Content and workflow grids collapse to one column',grids,'responsive grid declarations'),
    check('Evidence and backup forms remain reachable at narrow widths',form,'search row and submit button reflow'),
    check('Base layout has documented minimum and shrinkable main',roots,'320px root and min-width:0'),
    check('Modal constrains width and scrolls vertically',modal,'max-height:90dvh and overflow-y:auto'),
    check('Long paths and text wrap in bounded panels',wraps,'word-break and overflow-wrap'),
  ],limitations:['Static CSS review only; no browser viewport render, screenshot, zoom/reflow measurement or pointer interaction executed',
    'A matching CSS selector does not prove absence of horizontal overflow, clipped content, or all touch-target sizes']};
}

function testQualityAudit(source) {
  const tests = [...source.matchAll(/#\[test\]/g)].length;
  const negative = source.includes('fn identical_reads_have_distinct_opaque_tickets_and_stale_or_forged_tickets_fail()')
    && source.includes('assert_ne!(ticket1, ticket2);')
    && source.includes('host.pending_native_approval(id2, ticket1).is_err()');
  const noDisclosure = source.includes('fn read_requires_consent_and_cannot_escape_workspace()')
    && source.includes('assert!(preview.get("sha256").is_none());')
    && source.includes('assert!(preview.get("byte_count").is_none());');
  const proof = source.includes('NativeConsentProof::fixture_for_test(&id2.to_string(), ticket2)')
    && source.includes('!native_proof.verify(&operation_id.to_string(), pending.prepared.digest())');
  const consumption = source.includes('fn native_confirmation_is_reserved_once_and_rejection_consumes_it()')
    && source.includes('host.pending_native_approval(operation_id, digest).is_err()');
  const effects = source.includes('!workspace.path().join("tampered.txt").exists()')
    && source.includes('assert_eq!(result["content"], "not exported");');
  const noClobber = source.includes('fn approval_is_one_use_and_write_is_no_clobber_across_restart()')
    && source.includes('WriteMode::CreateNew');
  return {purpose:'Map real native file-tool test functions to adversarial assertions for random ticket, confidentiality, proof binding, one-use and no-clobber behavior',checks:[
    check('Multiple independent native host test functions discovered',tests>=5,`${tests} Rust #[test] functions`),
    check('Random tickets and stale cross-operation replay tested',negative,'two real independent approvals'),
    check('No preconsent read fingerprint or length exposed',noDisclosure,'negative JSON field assertions'),
    check('Forged native proof is checked against private operation',proof,'ticket/native-proof mismatch fixture'),
    check('Duplicate consent and rejection explicitly tested',consumption,'one-use native reservation'),
    check('Tests assert content and absence of write side effects',effects,'real file path and returned content'),
    check('Durable no-clobber write and restart test exists',noClobber,'CreateNew restart test'),
  ],limitations:['Audits named Rust test functions and key assertions; does not execute the native Rust suite or calculate coverage or mutation score',
    'Synthetic source edits in companion Node tests verify this auditor flags missing assertions; OS-native GUI and real consent remain separate']};
}

export function runCandidate(name, overriddenSource, overriddenInputs = {}) {
  const candidate = paths[name];
  if (!candidate) throw new Error('Unapproved source-reviewed skill candidate');
  const entry = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8')).entries
    .find((item) => item.id === `skill:${name}`);
  if (!entry || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
    throw new Error('Candidate must be source reviewed or independently vetted');
  }
  const [category, inputPath] = candidate;
  const relativeUpstream = `skills/${category}/${name}/SKILL.md`;
  const pinned = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8',windowsHide:true}).trim();
  const dirty = execFileSync('git', ['-C', upstream, 'status', '--porcelain'], {encoding:'utf8',windowsHide:true});
  if (dirty || !/^[a-f0-9]{40}$/.test(pinned)) throw new Error('Upstream must be clean and revision-pinned');
  const sourceBytes = readFileSync(join(upstream, relativeUpstream));
  const pinnedBytes = execFileSync('git', ['-C', upstream, 'show', `${pinned}:${relativeUpstream}`],
    {windowsHide:true});
  const licenseBytes = readFileSync(join(upstream, 'LICENSE'));
  const pinnedLicense = execFileSync('git',['-C',upstream,'show',`${pinned}:LICENSE`],{windowsHide:true});
  if (sha(sourceBytes) !== entry.source.sha256 || sha(norm(sourceBytes)) !== sha(pinnedBytes)
    || sha(norm(licenseBytes)) !== sha(pinnedLicense)
    || !licenseBytes.toString('utf8').includes('MIT License') || entry.license.spdx !== 'MIT'
    || entry.license.status !== 'verified') throw new Error('Upstream source or declared MIT license differs');
  const programBytes = readFileSync(resolve(repo, inputPath));
  const dependencyInputs = name === 'dependency-grounding-auditor' ? Object.fromEntries(Object.entries({
    pnpmLock:'apps/desktop/pnpm-lock.yaml',
    tauriApi:'apps/desktop/node_modules/@tauri-apps/api/package.json',
    svelte:'apps/desktop/node_modules/svelte/package.json',
    cargoManifest:'apps/desktop/src-tauri/Cargo.toml',
    cargoLock:'apps/desktop/src-tauri/Cargo.lock',
  }).map(([key,path]) => [key, {path, bytes:readFileSync(resolve(repo,path))}]))
    : name === 'accessibility-auditor' ? {css:{path:'apps/desktop/src/routes/studio.css', bytes:readFileSync(resolve(repo,'apps/desktop/src/routes/studio.css'))}}
      : {};
  const managedPath = join(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation', name, 'SKILL.md');
  let installedSourceSha = null;
  try {
    const installedBytes = readFileSync(managedPath);
    if (sha(installedBytes) === sha(sourceBytes)) installedSourceSha = sha(installedBytes);
  } catch { /* Preserve source-only audit as unqualified. */ }
  const result = name === 'injection-risk-auditor' ? injectionAudit(overriddenSource ?? programBytes.toString('utf8'))
    : name === 'migration-safety-reviewer' ? migrationAudit(overriddenSource ?? programBytes.toString('utf8'))
      : name === 'requirement-traceability-auditor' ? traceabilityAudit(overriddenSource ?? programBytes.toString('utf8'))
        : name === 'scope-drift-guard' ? scopeAudit(overriddenSource ?? programBytes.toString('utf8'))
          : name === 'dependency-grounding-auditor' ? dependencyAudit(overriddenSource ?? programBytes.toString('utf8'),
              Object.fromEntries(Object.entries(dependencyInputs).map(([key,value]) =>
                [key, overriddenInputs[key] ?? value.bytes.toString('utf8')])))
            : name === 'accessibility-auditor' ? accessibilityAudit(overriddenSource ?? programBytes.toString('utf8'),
                overriddenInputs.css ?? dependencyInputs.css.bytes.toString('utf8'))
              : name === 'responsive-design-auditor' ? responsiveAudit(overriddenSource ?? programBytes.toString('utf8'))
                : testQualityAudit(overriddenSource ?? programBytes.toString('utf8'));
  return {
    id: `skill:${name}`,
    status: installedSourceSha ? 'installed_exercised_pending_manifest_verification' : 'source_reviewed_uninstalled_not_qualified',
    project: 'Constellation',
    qualification_credit: 0,
    command: `node skills-manifest/reviewed-exercise.mjs ${name} --write-receipt`,
    scope: 'bounded_offline_source_contract',
    purpose: result.purpose,
    checks: result.checks,
    outcome: result.checks.every((c) => c.result === 'pass') ? 'passed' : 'failed',
    limitations: [...result.limitations, 'A passing receipt alone never establishes installation, permission, or runtime MCP qualification',
      'Publisher identity was not independently authenticated; MIT license applies to observed pinned repository content'],
    upstream: {revision:pinned, path:`../vibe-coding-skills/${relativeUpstream}`,
      sha256:sha(sourceBytes), pinned_blob_sha256:sha(pinnedBytes),
      license_sha256:sha(licenseBytes), license_pinned_sha256:sha(pinnedLicense),
      publisher_identity:'unverified'},
    input: {path:inputPath, sha256:sha(programBytes)},
    inputs: {[inputPath]: {path:inputPath, sha256:sha(programBytes)},
      ...Object.fromEntries(Object.values(dependencyInputs).map(({path,bytes}) => [path,{path,sha256:sha(bytes)}]))},
    installed_source_sha256: installedSourceSha,
    exercise_script_sha256: sha(readFileSync(fileURLToPath(import.meta.url))),
    runner: {path:'skills-manifest/reviewed-exercise.mjs',
      sha256:sha(readFileSync(fileURLToPath(import.meta.url)))},
  };
}

function main() {
  const [name, flag] = process.argv.slice(2);
  if (!list.includes(name) || !['--write-receipt', '--check'].includes(flag)
    || process.argv.length !== 4) {
    process.stderr.write(`Usage: node skills-manifest/reviewed-exercise.mjs <${list.join('|')}> --check|--write-receipt\n`);
    process.exitCode = 2;
    return;
  }
  const result = runCandidate(name);
  if (result.outcome !== 'passed') {
    process.stderr.write(JSON.stringify(result.checks.filter(c => c.result === 'fail'),null,2)+'\n');
    process.exitCode = 1;
    return;
  }
  if (flag === '--write-receipt') {
    if (!result.installed_source_sha256) {
      process.stderr.write('Candidate has no exact verified managed installation; refusing exercise receipt\n');
      process.exitCode = 1;
      return;
    }
    const path = join(here,'evidence','reviewed', `${name}.json`);
    mkdirSync(dirname(path),{recursive:true});
    // Record the actual local execution date; historical fixed dates made
    // refreshed exercise receipts misleading after source revisions.
    const receipt = {...result, observed_on:new Date().toISOString().slice(0,10)};
    writeFileSync(path, JSON.stringify(receipt,null,2)+'\n', {flag:'w'});
    process.stdout.write(`${relative(repo,path)} sha256=${sha(readFileSync(path))} checks=${result.checks.length} qualification=0\n`);
  } else {
    process.stdout.write(`${result.id}: ${result.checks.length} local checks PASS, zero installed qualification credit\n`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
