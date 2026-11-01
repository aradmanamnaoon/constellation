#!/usr/bin/env node
/**
 * Read-only, conservative, path-complete review of non-ignored Git-untracked
 * release-source candidates. No network, staging, filesystem writes or secrets
 * printed. Per-path output: node scripts/qa/check-release-source-curation.mjs --jsonl
 * Summary output: node scripts/qa/check-release-source-curation.mjs
 * After someone else stages files, separately inspect the proposed index:
 * node scripts/qa/check-release-source-curation.mjs --staged
 *
 * Decisions are review queues, NEVER automatic Git-add permission.
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = realpathSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..'));
const TEXT_EXT = new Set([
  '.rs', '.toml', '.lock', '.js', '.mjs', '.ts', '.svelte', '.css',
  '.ps1', '.sh', '.py', '.md', '.json', '.yml', '.yaml', '.txt',
  '.html', '.gitignore',
]);
const MAX_TEXT_BYTES = 3_000_000;
const SHA = (bytes) => createHash('sha256').update(bytes).digest('hex');
const normalized = (name) => name.replaceAll('\\', '/');
const secretIndicators = Object.freeze([
  ['PRIVATE_KEY_DELIMITER', /-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----/g],
  ['GITHUB_TOKEN_SHAPE', /\bgh[opsru]_[A-Za-z0-9_]{24,}\b/g],
  ['AWS_ACCESS_KEY_SHAPE', /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g],
  ['OPENAI_TOKEN_SHAPE', /\bsk-(?:proj-)?[A-Za-z0-9_-]{24,}\b/g],
  ['HARDCODED_BEARER_SHAPE', /\bBearer\s+[A-Za-z0-9._~+/-]{35,}/gi],
  ['URL_PASSWORD_SHAPE', /https?:\/\/[^\s/]{1,80}:[^\s/@]{8,80}@/gi],
  ['QUOTED_SECRET_ASSIGNMENT', /\b(?:api[_-]?key|access[_-]?token|client[_-]?secret|password)\s*[:=]\s*["'][A-Za-z0-9._/+~=-]{28,}["']/gi],
]);
const FORBIDDEN = /(^|\/)(?:\.env(?:\..*)?|node_modules|target|dist|build|\.svelte-kit|__pycache__|coverage|test-results|playwright-report)(?:\/|$)|\.(?:pem|p12|pfx|jks|keystore|key|exe|dll|msi|dmg|appimage|apk|aab|sqlite|sqlite3|db)$/i;

function gitNames(args) {
  return execFileSync('git', ['-C', ROOT, 'ls-files', '-z', ...args], {
    encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, windowsHide: true,
  }).split('\0').filter(Boolean).map(normalized);
}
function stagedNames() {
  return execFileSync('git', ['-C', ROOT, 'diff', '--cached', '--name-only',
    '--diff-filter=ACMR', '-z'], {
    encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, windowsHide: true,
  }).split('\0').filter(Boolean).map(normalized);
}

function classify(name) {
  if (FORBIDDEN.test(name)) return ['EXCLUDE_UNSAFE_PATH', 'Build, credential or runtime state is forbidden in source Git'];
  if (name.startsWith('skills-manifest/evidence/'))
    return ['HOLD_LOCAL_RECEIPT', 'Machine-bound skill exercise receipt; assess privacy, reproducibility and publishing need'];
  if (name.startsWith('docs/qa/screenshots/'))
    return ['HOLD_IMAGE_RIGHTS_PRIVACY', 'UI screenshots need manual privacy, visibility, copyright and unique-evidence review'];
  if (/^docs\/security\/.*\.(?:cdx\.)?json$/i.test(name))
    return ['HOLD_GENERATED_SECURITY', 'Prior SBOM/scan snapshot: regenerate from frozen lockfiles and verify licensing'];
  if (name === 'docs/qa/untracked-source-inventory-20261101.json')
    return ['EXCLUDE_STALE_INVENTORY', 'Historical inventory is not authoritative after source edits'];
  if (name === 'docs/mission/COMPLETE_EXECUTION_PLAN.md')
    return ['REVIEW_GENERATED_REQUIRED', 'Generated plan is referenced by CI tests; confirm deterministic regeneration from current inputs'];
  if (/^(?:docs\/mission\/(?:NEXT_CHAT_PROMPT|WORKER_ASSIGNMENTS|active-task-plan|continuation-handoff-[^/]+)\.md|MASTER_CONTINUATION\.md|Codex_Master_Prompt_Local_Agent_Collaboration_Platform\.md)$/.test(name))
    return ['HOLD_INTERNAL_CONTEXT', 'Possible private runbook, agent handoff, account context or nonpublication instructions'];
  if (name.startsWith('docs/qa/'))
    return ['HOLD_QA_PROVENANCE', 'Historical or current QA claim; review source pins, screenshots, credentials, false PASS and necessity'];
  if (name.startsWith('docs/release/'))
    return ['HOLD_RELEASE_SNAPSHOT', 'Historical prerelease snapshot; never elevate stale artifact hashes to release evidence'];
  if (name.startsWith('docs/design/'))
    return ['HOLD_DESIGN_RIGHTS', 'Design assets/references require attribution and independent visual rights review'];
  if (name.startsWith('docs/security/'))
    return ['HOLD_SECURITY_REPORT', 'Review claims, local paths, age, third-party license and disclosure risk'];
  if (name === 'LICENSE')
    return ['INCLUDE_LEGAL_REVIEW', 'Root project license, compare upstream rights to declarations'];
  if (/^(?:README\.md|CHANGELOG\.md|CONTRIBUTING\.md|SECURITY\.md)$/.test(name))
    return ['INCLUDE_DOC_REVIEW', 'Public project documentation; verify claims and contacts'];
  if (name === '.gitignore')
    return ['INCLUDE_CI_REVIEW', 'Security-sensitive ignore rules; inspect tracked and private state exclusions'];
  if (/^docs\/(?:[^/]+\.md|mission\/(?:generate-complete-execution-plan\.mjs|master-feature-acceptance\.md)|quality\/[^/]+\.md)$/.test(name))
    return ['INCLUDE_DOC_REVIEW', 'Potential public documentation or source-generator contract; verify content and links'];
  if (name.startsWith('docs/')) return ['HOLD_DOC_UNCLASSIFIED', 'Document lacks source-publication classification'];
  if (name.startsWith('.github/workflows/'))
    return ['INCLUDE_CI_REVIEW', 'Required CI source; inspect permissions, dispatch, pinned actions and shell injection'];
  if (/^crates\/[^/]+\/Cargo\.lock$/.test(name))
    return ['INCLUDE_LOCK_REVIEW', 'Required standalone Cargo lock; compare manifest, dependency resolution and license obligations'];
  if (/^crates\/[^/]+\/Cargo\.toml$/.test(name))
    return ['INCLUDE_CRATE_REVIEW', 'Rust package manifest; verify legal declaration, dependencies and source references'];
  if (name.startsWith('crates/'))
    return ['INCLUDE_CODE_REVIEW', 'Rust implementation/test/config candidate; audit secret literals and actual callers'];
  if (name.startsWith('apps/desktop/'))
    return ['INCLUDE_CODE_REVIEW', 'Native/renderer source; require versioned reproducible build and integration tests'];
  if (name.startsWith('catalog/'))
    return ['INCLUDE_CODE_REVIEW', 'Role/evaluation source; fixtures must not imply live role qualification'];
  if (name.startsWith('tests/'))
    return ['INCLUDE_TEST_REVIEW', 'Executable test or fixture; review secrets, opt-in effects and platform tooling'];
  if (name.startsWith('scripts/release/'))
    return ['INCLUDE_CI_REVIEW', 'Release preflight script; verify read-only default and any fail-closed publishing boundary'];
  if (name.startsWith('skills-manifest/'))
    return ['INCLUDE_SKILL_SOURCE_REVIEW', 'Manifest tools/tests are source, distinct from local exercise receipts'];
  if (name.startsWith('scripts/qa/'))
    return ['INCLUDE_TEST_REVIEW', 'Read-only QA helper; review its output/side effects before staging'];
  return ['HOLD_UNCLASSIFIED', 'Requires explicit owner classification, not blanket git add'];
}

function inspectPng(data) {
  const flags = [];
  const pngSignature = Buffer.from([137,80,78,71,13,10,26,10]);
  if (data.length < 33 || !data.subarray(0, 8).equals(pngSignature) ||
      data.toString('ascii', 12, 16) !== 'IHDR' || data.readUInt32BE(8) !== 13) {
    return { flags: ['INVALID_PNG_HEADER'], dimensions: null };
  }
  const width = data.readUInt32BE(16);
  const height = data.readUInt32BE(20);
  if (width === 0 || height === 0 || width > 16000 || height > 16000)
    flags.push('PNG_UNEXPECTED_DIMENSIONS');
  let at = 8, chunks = 0, final = false;
  while (at + 12 <= data.length && chunks++ < 100000) {
    const size = data.readUInt32BE(at);
    if (size > data.length - at - 12) { flags.push('PNG_TRUNCATED_CHUNK'); break; }
    const tag = data.toString('ascii', at + 4, at + 8);
    if (['tEXt', 'iTXt', 'zTXt', 'eXIf'].includes(tag)) flags.push('PNG_POTENTIAL_METADATA');
    at += 12 + size;
    if (tag === 'IEND') { final = true; break; }
  }
  if (!final || at !== data.length) flags.push('PNG_MISSING_END_OR_TRAILING_DATA');
  return { flags: [...new Set(flags)], dimensions: [width, height] };
}

function scanText(name, buffer) {
  const flags = [];
  const findings = [];
  if (buffer.length > MAX_TEXT_BYTES) return { flags: ['TEXT_TOO_LARGE_TO_INSPECT'], findings };
  const source = buffer.toString('utf8');
  if (source.includes('\uFFFD')) flags.push('UTF8_DECODE_REPLACEMENT');
  if (source.includes('\0')) flags.push('TEXT_HAS_NUL_BYTE');
  for (const [kind, regex] of secretIndicators) {
    regex.lastIndex = 0;
    for (const match of source.matchAll(regex)) {
      findings.push({ kind, line: source.slice(0, match.index).split('\n').length });
      if (findings.length >= 30) break;
    }
    if (findings.length >= 30) { flags.push('SECRET_HIT_REPORT_CAPPED'); break; }
  }
  if (name.endsWith('.json')) {
    try { JSON.parse(source.replace(/^\uFEFF/, '')); } catch { flags.push('INVALID_JSON'); }
  }
  if (/^crates\/[^/]+\/Cargo\.toml$/.test(name) &&
      !/^license\s*=\s*["'](?:MIT|Apache-2\.0|BSD-2-Clause|BSD-3-Clause)["']/m.test(source))
    flags.push('CARGO_LICENSE_MISSING_OR_OTHER');
  if (name.endsWith('.md')) {
    if (/!\[[^\]]*\]\(https?:\/\//i.test(source) ||
        /<img\s+[^>]*src\s*=\s*["']https?:\/\//i.test(source))
      flags.push('EXTERNAL_IMAGE_RIGHTS_REVIEW');
    if (/^.*(?:C:\\Users\\|\/home\/[^\s/]+\/|AppData\\Local\\Temp\\).*$/mi.test(source))
      flags.push('LOCAL_HOME_PATH_POSSIBLE');
    if (/\bPASS\b/.test(source) && /^docs\/qa\//.test(name))
      flags.push('HISTORICAL_PASS_CLAIM_REVIEW');
  }
  return { flags, findings };
}

function readOne(name) {
  const [suggestedAction, rationale] = classify(name);
  const base = {
    path: name, action: suggestedAction, rationale, flags: [], findings: [],
    bytes: null, sha256: null,
  };
  if (path.posix.isAbsolute(name) || name.split('/').some((part) => part === '..' || part === '' || part === '.') ||
      /[\u0000-\u001F]/.test(name))
    return { ...base, action: 'BLOCK_UNSAFE_NAME', flags: ['UNSAFE_GIT_PATH'] };
  const absolute = path.resolve(ROOT, name);
  if (!absolute.startsWith(ROOT + path.sep)) {
    return { ...base, action: 'BLOCK_PATH_ESCAPE', flags: ['ROOT_ESCAPE'] };
  }
  let info;
  try { info = lstatSync(absolute); } catch {
    return { ...base, action: 'BLOCK_UNREADABLE', flags: ['FILE_MISSING_OR_UNREADABLE'] };
  }
  if (info.isSymbolicLink() || !info.isFile())
    return { ...base, action: 'BLOCK_UNSAFE_TYPE', flags: ['SYMLINK_OR_NONFILE'] };
  const data = readFileSync(absolute);
  base.bytes = data.length;
  base.sha256 = SHA(data);
  if (name.toLowerCase().endsWith('.png')) {
    const result = inspectPng(data);
    base.flags.push(...result.flags);
    base.pngDimensions = result.dimensions;
    if (base.flags.some((f) => f.startsWith('INVALID_PNG') || f.startsWith('PNG_TRUNCATED') || f.startsWith('PNG_MISSING')))
      base.action = 'BLOCK_INVALID_PNG';
  } else if (TEXT_EXT.has(path.extname(name).toLowerCase()) ||
             ['LICENSE', 'Dockerfile', '.gitignore'].includes(path.basename(name))) {
    const result = scanText(name, data);
    base.flags.push(...result.flags);
    base.findings = result.findings; // only indicator categories and line numbers
    if (result.findings.length) base.action = 'HOLD_SECRET_SIGNATURE_REVIEW';
    if (base.flags.some((f) => ['TEXT_TOO_LARGE_TO_INSPECT','INVALID_JSON','TEXT_HAS_NUL_BYTE'].includes(f)))
      base.action = 'HOLD_FORMAT_REVIEW';
  } else {
    base.flags.push('UNKNOWN_FILE_FORMAT');
    base.action = 'HOLD_UNKNOWN_FORMAT';
  }
  return base;
}

function main() {
  const args = new Set(process.argv.slice(2));
  if ([...args].some((arg) => !['--jsonl', '--summary', '--include-paths', '--self-test', '--staged'].includes(arg)))
    throw new Error('Use --jsonl, --summary, --include-paths, --staged or --self-test (no writes).');
  if (args.has('--self-test')) {
    const expected = [
      ['docs/qa/screenshots/example.png', 'HOLD_IMAGE_RIGHTS_PRIVACY'],
      ['skills-manifest/evidence/example.json', 'HOLD_LOCAL_RECEIPT'],
      ['crates/constellation-core/Cargo.lock', 'INCLUDE_LOCK_REVIEW'],
      ['docs/mission/COMPLETE_EXECUTION_PLAN.md', 'REVIEW_GENERATED_REQUIRED'],
      ['scripts/release/platform-preflight.mjs', 'INCLUDE_CI_REVIEW'],
      ['.env', 'EXCLUDE_UNSAFE_PATH'],
    ];
    for (const [name, kind] of expected) {
      if (classify(name)[0] !== kind)
        throw new Error('Classification regression');
    }
    const fakeToken = `ghp_${'Z'.repeat(30)}`;
    const scan = scanText('tests/synthetic.txt', Buffer.from(`test-only = "${fakeToken}"`));
    if (!scan.findings.some((f) => f.kind === 'GITHUB_TOKEN_SHAPE'))
      throw new Error('Synthetic credential signature regression');
    if (!inspectPng(Buffer.from('not-a-png')).flags.includes('INVALID_PNG_HEADER'))
      throw new Error('Invalid PNG must be rejected');
    if (scanText('tests/synthetic.json', Buffer.from('{invalid')).flags.includes('INVALID_JSON') === false)
      throw new Error('Invalid JSON must be rejected');
    process.stdout.write('SOURCE_CURATION_SELF_TEST_PASS cases=9 no_files_written\n');
    return;
  }
  const indexed = gitNames(['--cached']);
  const untracked = gitNames(['--others', '--exclude-standard']);
  const staged = stagedNames();
  const selection = args.has('--staged') ? 'staged' : 'untracked';
  const selected = selection === 'staged' ? staged : untracked;
  if (new Set(selected).size !== selected.length)
    throw new Error('Duplicate Git paths; review input before proceeding.');
  const rows = [...selected].sort().map(readOne);
  // Active workers may stage source while scanning. Never emit a reassuring
  // complete-inventory summary based on a mixture of two Git index states.
  const indexedAfter = gitNames(['--cached']);
  const untrackedAfter = gitNames(['--others', '--exclude-standard']);
  const stagedAfter = stagedNames();
  if (JSON.stringify([...indexed].sort()) !== JSON.stringify(indexedAfter.sort()) ||
      JSON.stringify([...untracked].sort()) !== JSON.stringify(untrackedAfter.sort()) ||
      JSON.stringify([...staged].sort()) !== JSON.stringify(stagedAfter.sort())) {
    throw new Error('Git index changed during curation: retry after source freeze');
  }
  const shaGroups = new Map();
  for (const row of rows) {
    if (row.sha256) {
      if (!shaGroups.has(row.sha256)) shaGroups.set(row.sha256, []);
      shaGroups.get(row.sha256).push(row.path);
    }
  }
  for (const row of rows) {
    if (row.sha256 && shaGroups.get(row.sha256).length > 1)
      row.flags.push('BYTE_IDENTICAL_OTHER_UNTRACKED');
  }
  if (args.has('--jsonl')) {
    for (const row of rows) process.stdout.write(JSON.stringify(row) + '\n');
    return;
  }
  const byAction = Object.fromEntries([...new Set(rows.map((r) => r.action))].sort()
    .map((action) => [action, rows.filter((r) => r.action === action).length]));
  const byGroup = {};
  for (const row of rows) {
    const group = row.path.startsWith('.') ? row.path.split('/')[0] : row.path.split('/')[0];
    byGroup[group] = (byGroup[group] || 0) + 1;
  }
  const flagged = rows.filter((r) => r.findings.length > 0)
    .map((r) => ({ path: r.path, findings: r.findings }));
  const duplicates = [...shaGroups.values()].filter((group) => group.length > 1)
    .map((members) => ({ members }));
  const summary = {
    schema: 'constellation.release-source-curation.readonly.v1',
    repository: ROOT, selection, indexedCount: indexed.length,
    untrackedCount: untracked.length, stagedCount: staged.length,
    selectedCount: rows.length, reviewedPaths: rows.length,
    totalBytes: rows.reduce((sum, r) => sum + (r.bytes || 0), 0),
    byAction, byGroup, secretSignatureCandidates: flagged,
    invalidOrUnreadable: rows.filter((r) => r.action.startsWith('BLOCK_')).map((r) => r.path),
    byteIdenticalGroups: duplicates,
    noAutoStageApproval: true,
    safeToPublishWithoutHumanReview: false,
  };
  process.stdout.write(JSON.stringify(summary, null, 2) + '\n');
  if (args.has('--include-paths')) {
    for (const row of rows) process.stdout.write(JSON.stringify(row) + '\n');
  }
}

try { main(); } catch (error) {
  // Do not stringify error: it could expose a user-provided path or content.
  process.stderr.write('SOURCE_CURATION_ERROR read_only_check_failed\n');
  process.exitCode = 1;
}
