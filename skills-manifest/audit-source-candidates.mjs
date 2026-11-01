#!/usr/bin/env node
// Read-only, offline prequalification of distinct sibling-repository skills.
// This NEVER installs a skill, writes a receipt, or awards qualification credit.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');
const upstreamRoot = resolve(repoRoot, '..', 'vibe-coding-skills');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
// Immutable original review cohort: promotion must not silently shrink audit
// coverage or permit a different skill to inherit one of the twenty slots.
const originalTwenty = Object.freeze([
  'existing-work-preservation', 'mcp-reliability-auditor',
  'multi-agent-integration-reviewer', 'project-context-recovery',
  'architecture-complexity-auditor', 'loop-breaker',
  'reproduction-builder', 'root-cause-debugger',
  'ci-failure-investigator', 'environment-parity-auditor',
  'production-readiness-auditor', 'token-efficient-workflow',
  'dead-feature-detector', 'release-verification-agent',
  'authentication-security-auditor', 'dependency-vulnerability-auditor',
  'integration-reality-checker', 'missing-test-detector',
  'regression-detector', 'ui-flow-verifier',
]);
const normalized = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const within = (parent, child) => {
  const path = relative(parent, child);
  return !!path && path !== '..' && !path.startsWith('..\\')
    && !path.startsWith('../') && !isAbsolute(path);
};
const git = (root, ...args) => execFileSync('git', ['-C', root, ...args], {
  encoding: args[0] === 'status' || args[0] === 'rev-parse' ? 'utf8' : 'buffer',
  windowsHide: true,
  maxBuffer: 1024 * 1024,
  stdio: ['ignore', 'pipe', 'ignore'],
});

/**
 * Read only locally observed, unqualified sibling skill files and an immutable
 * local Git revision. Installed and actually exercised skills are handled by
 * validate.mjs; source-review results here always have zero qualification credit.
 */
export function auditSourceCandidates(manifest, options = {}) {
  const sourceRoot = options.sourceRoot ?? upstreamRoot;
  const managedRoot = options.managedRoot ??
    join(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation');
  const errors = [];
  const reports = [];
  const entries = manifest.entries ?? [];
  const candidates = originalTwenty.map((name) => entries.find((entry) =>
    entry.id === 'skill:' + name));
  const previouslyVetted = entries.filter((entry) => entry.kind === 'skill' && entry.status === 'installed_vetted');
  const revisions = [...new Set(previouslyVetted.map((entry) => entry.source?.upstream_revision))];
  let revision = '';
  try {
    revision = String(git(sourceRoot, 'rev-parse', 'HEAD')).trim();
    if (String(git(sourceRoot, 'status', '--porcelain')).trim()) {
      errors.push('Sibling upstream checkout is modified; immutable-source review unavailable');
    }
  } catch {
    errors.push('Cannot inspect local sibling Git checkout');
  }
  if (!/^[a-f0-9]{40}$/.test(revision) || revisions.length !== 1 || revisions[0] !== revision) {
    errors.push('Source revision differs from existing vetted local Git pin');
  }
  let licenseBytes;
  try {
    licenseBytes = readFileSync(join(sourceRoot, 'LICENSE'));
    if (!licenseBytes.toString('utf8').includes('MIT License')
      || sha256(normalized(licenseBytes)) !== sha256(git(sourceRoot, 'show', revision + ':LICENSE'))) {
      errors.push('MIT license differs from pinned local Git blob');
    }
  } catch {
    errors.push('Cannot verify pinned MIT license');
  }
  const seenPaths = new Set();
  const seenSha = new Set();
  for (const entry of candidates) {
    if (!entry || entry.kind !== 'skill'
      || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
      errors.push('Original reviewed candidate missing, renamed or invalidly promoted');
      continue;
    }
    const id = entry.id;
    const name = id?.replace(/^skill:/, '');
    const prefix = '../vibe-coding-skills/skills/';
    const sourcePath = (entry.status === 'installed_vetted'
      ? entry.source?.upstream_path : entry.source?.path)?.replaceAll('\\', '/');
    if (!/^skill:[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(id ?? '')
      || typeof sourcePath !== 'string' || !sourcePath.startsWith(prefix)
      || !sourcePath.endsWith('/' + name + '/SKILL.md')) {
      errors.push(id + ': invalid upstream source identity');
      continue;
    }
    const relativeSource = sourcePath.slice('../vibe-coding-skills/'.length);
    const diskPath = resolve(repoRoot, sourcePath);
    if (!within(sourceRoot, diskPath) || !existsSync(diskPath)
      || !within(realpathSync(sourceRoot), realpathSync(diskPath))) {
      errors.push(id + ': upstream source escapes checkout or is missing');
      continue;
    }
    try {
      const bytes = readFileSync(diskPath);
      const pinnedBytes = git(sourceRoot, 'show', revision + ':' + relativeSource);
      const digest = sha256(bytes);
      const front = bytes.toString('utf8');
      const validHeaders = front.startsWith('---\n') || front.startsWith('---\r\n');
      const distinct = !seenPaths.has(relativeSource.toLowerCase()) && !seenSha.has(digest);
      seenPaths.add(relativeSource.toLowerCase());
      seenSha.add(digest);
      if (!validHeaders || !front.split(/\r?\n/).slice(0, 12).includes('name: ' + name)
        || !front.includes('# Safety restrictions') || !front.includes('# Completion criteria')) {
        errors.push(id + ': source lacks matching frontmatter or safety/completion contract');
      }
      if (bytes.byteLength > 128 * 1024 || digest !== entry.source.sha256
        || sha256(normalized(bytes)) !== sha256(pinnedBytes)) {
        errors.push(id + ': source differs from pinned Git blob or inventory SHA');
      }
      if (entry.license?.status !== 'verified' || entry.license.spdx !== 'MIT'
        || !distinct) {
        errors.push(id + ': license or distinct source identity unverified');
      }
      const installedPath = join(managedRoot, name, 'SKILL.md');
      const installed = existsSync(installedPath) && statSync(installedPath).isFile();
      const installMatches = installed && sha256(readFileSync(installedPath)) === digest;
      if (installed && !installMatches) errors.push(id + ': managed installation differs from pinned source');
      reports.push({
        id, source_sha256: digest, git_revision: revision,
        license: 'MIT (pinned local Git blob)', publisher_identity: 'unverified',
        managed_installation: installed ? (installMatches ? 'source-matched' : 'mismatch') : 'absent',
        project_exercise: entry.status === 'installed_vetted'
          ? 'refer_to_strict_independent_receipt_validation' : 'unqualified',
        qualification_credit: 0,
      });
    } catch {
      errors.push(id + ': unable to inspect pinned source or managed installation');
    }
  }
  const unexpected = entries.filter((entry) => entry.kind === 'skill'
    && entry.status === 'source_reviewed' && !originalTwenty.includes(entry.id.slice(6)));
  if (unexpected.length || reports.length !== 20) {
    errors.push('Original 20 candidate identities are missing or supplemented');
  }
  return {
    ok: errors.length === 0, errors,
    summary: {
      source_reviewed: candidates.filter((entry) => entry?.status === 'source_reviewed').length,
      source_checked: reports.length,
      promoted_vetted: candidates.filter((entry) => entry?.status === 'installed_vetted').length,
      uninstalled: reports.filter((entry) => entry.managed_installation === 'absent').length,
      installed_without_exercise: reports.filter((report) =>
        report.managed_installation === 'source-matched'
        && report.project_exercise === 'unqualified').length,
      installed_unvetted_user_level: entries.filter((entry) => entry.status === 'installed_unvetted').length,
      source_review_credit: 0,
    },
    reports,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 2) {
    process.stderr.write('Usage: node skills-manifest/audit-source-candidates.mjs\n');
    process.exitCode = 2;
  } else {
    const manifest = JSON.parse(readFileSync(join(here, 'inventory.json'), 'utf8'));
    const result = auditSourceCandidates(manifest);
    process.stdout.write(JSON.stringify(result, null, 2) + '\n');
    if (!result.ok) process.exitCode = 1;
  }
}
