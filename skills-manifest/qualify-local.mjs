#!/usr/bin/env node
// Promote only locally installed, provenance-pinned skills with actual passing
// Constellation exercise receipts. Installs nothing and makes no network calls.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateManifest } from './validate.mjs';
import { runCandidate } from './reviewed-exercise.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstream = resolve(root, '..', 'vibe-coding-skills');
const managed = join(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation');
const manifestPath = join(here, 'inventory.json');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const canonicalText = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const candidates = [
  ['api-contract-auditor', 'testing'],
  ['input-validation-auditor', 'security'],
  ['documentation-accuracy-auditor', 'documentation'],
  ['dependency-cycle-auditor', 'architecture'],
  ['completion-claims-auditor', 'agent-reliability'],
  ['database-schema-reviewer', 'backend'],
  ['security-misconfiguration-auditor', 'security'],
  ['agent-tool-trust-auditor', 'security'],
  ['authorization-boundary-auditor', 'security'],
  ['secret-leakage-detector', 'security'],
  ['ux-state-completeness-auditor', 'frontend'],
  ['data-consistency-auditor', 'backend'],
  ['environment-config-auditor', 'deployment'],
  ['cross-platform-readiness-auditor', 'deployment'],
];
const expansion = [
  ['injection-risk-auditor', 'security'],
  ['migration-safety-reviewer', 'backend'],
  ['requirement-traceability-auditor', 'agent-reliability'],
  ['scope-drift-guard', 'agent-reliability'],
  ['dependency-grounding-auditor', 'dependencies'],
  ['accessibility-auditor', 'frontend'],
  ['responsive-design-auditor', 'frontend'],
  ['test-quality-auditor', 'testing'],
];
const git = (...argv) => execFileSync('git', ['-C', upstream, ...argv],
  { encoding: 'utf8', windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] }).trim();
const gitBlob = (reference) => execFileSync('git', ['-C', upstream, 'show', reference],
  { windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });

function main() {
  if (process.argv.length !== 2) {
    throw new Error('Usage: node skills-manifest/qualify-local.mjs');
  }
  if (!process.env.APPDATA || git('status', '--porcelain')) {
    throw new Error('Managed skills path missing or upstream has uncommitted changes');
  }
  const revision = git('rev-parse', 'HEAD');
  if (!/^[a-f0-9]{40}$/.test(revision)) throw new Error('Invalid Git revision');
  const origin = git('remote', 'get-url', 'origin');
  const installedLicense = readFileSync(join(managed, 'LICENSE'));
  const pinnedLicense = gitBlob(`${revision}:LICENSE`);
  if (sha(canonicalText(installedLicense)) !== sha(pinnedLicense)
    || !installedLicense.toString().includes('MIT License')) {
    throw new Error('MIT license does not match pinned source');
  }
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const runner = readFileSync(join(here, 'exercise-local.mjs'));
  for (const [name, category] of candidates) {
    const entry = manifest.entries.find((item) => item.id === `skill:${name}`);
    if (!entry || entry.kind !== 'skill') throw new Error('Missing distinct candidate: ' + name);
    const relativeSource = `skills/${category}/${name}/SKILL.md`;
    const installedBytes = readFileSync(join(managed, name, 'SKILL.md'));
    const sourceBytes = readFileSync(join(upstream, ...relativeSource.split('/')));
    const pinnedBytes = gitBlob(`${revision}:${relativeSource}`);
    if (sha(installedBytes) !== sha(sourceBytes)
      || sha(canonicalText(installedBytes)) !== sha(pinnedBytes)) {
      throw new Error('Installed skill source differs from pinned Git blob: ' + name);
    }
    const receiptPath = join(here, 'evidence', name + '.json');
    const receiptBytes = readFileSync(receiptPath);
    const receipt = JSON.parse(receiptBytes);
    const command = `node skills-manifest/exercise-local.mjs ${name} --write-receipt`;
    if (receipt.id !== entry.id || receipt.project !== 'Constellation'
      || receipt.outcome !== 'passed' || receipt.command !== command
      || receipt.installed_source_sha256 !== sha(installedBytes)
      || receipt.exercise_script_sha256 !== sha(runner)
      || !Array.isArray(receipt.checks) || receipt.checks.length < 3
      || !receipt.checks.every((check) => check.status === 'passed')) {
      throw new Error('Missing current passing project exercise: ' + name);
    }
    entry.status = 'installed_vetted';
    entry.source = {
      path: `%APPDATA%/chat-on-steroids/skills/constellation/${name}/SKILL.md`,
      provenance: `Exact local copy of ${origin} at commit ${revision}; local Git blob and source hash verified, publisher identity not independently authenticated`,
      sha256: sha(installedBytes),
      upstream_blob_sha256: sha(pinnedBytes),
      upstream_repo: '../vibe-coding-skills',
      upstream_path: `../vibe-coding-skills/${relativeSource}`,
      upstream_revision: revision,
    };
    entry.evidence = {
      command,
      output: `Observed ${receipt.checks.length}/${receipt.checks.length} passing source-audit checks in Constellation; see hashed receipt. Installation and pinned Git source/license matched locally.`,
    };
    entry.license = {
      status: 'verified', spdx: 'MIT',
      evidence: '%APPDATA%/chat-on-steroids/skills/constellation/LICENSE',
      sha256: sha(installedLicense),
      upstream_blob_sha256: sha(pinnedLicense),
    };
    entry.exercise = {
      command, outcome: 'passed', result: receipt.objective,
      observed_on: receipt.observed_on,
      receipt_path: `skills-manifest/evidence/${name}.json`,
      receipt_sha256: sha(receiptBytes),
      runner_path: 'skills-manifest/exercise-local.mjs',
      runner_sha256: sha(runner),
    };
    entry.limitations = [
      'This is a locally installed development audit skill, not a Constellation runtime MCP or tool-enabled agent',
      ...receipt.limitations,
      'Exact local Git revision and MIT license matched; upstream publisher identity was not independently authenticated',
    ];
  }
  for (const [name, category] of expansion) {
    const entry = manifest.entries.find((item) => item.id === `skill:${name}`);
    if (!entry || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
      throw new Error('Unknown or non-reviewed expansion candidate: ' + name);
    }
    const relativeSource = `skills/${category}/${name}/SKILL.md`;
    const installedBytes = readFileSync(join(managed, name, 'SKILL.md'));
    const sourceBytes = readFileSync(join(upstream, ...relativeSource.split('/')));
    const pinnedBytes = gitBlob(`${revision}:${relativeSource}`);
    if (sha(installedBytes) !== sha(sourceBytes)
      || sha(canonicalText(installedBytes)) !== sha(pinnedBytes)) {
      throw new Error('Expansion installed bytes differ from pinned upstream: ' + name);
    }
    const receiptBytes = readFileSync(join(here, 'evidence', 'reviewed', name + '.json'));
    const receipt = JSON.parse(receiptBytes.toString('utf8'));
    const current = runCandidate(name);
    if (receipt.id !== entry.id || receipt.outcome !== 'passed'
      || receipt.command !== `node skills-manifest/reviewed-exercise.mjs ${name} --write-receipt`
      || receipt.installed_source_sha256 !== sha(installedBytes)
      || receipt.exercise_script_sha256 !== current.exercise_script_sha256
      || current.outcome !== 'passed'
      || !receipt.checks.every((check) => check.result === 'pass')) {
      throw new Error('Expansion receipt or exercise did not match current installation: ' + name);
    }
    entry.status = 'installed_vetted';
    entry.source = {
      path: `%APPDATA%/chat-on-steroids/skills/constellation/${name}/SKILL.md`,
      provenance: `Exact local copy of ${origin} at commit ${revision}; source hash verified; publisher identity not independently authenticated`,
      sha256: sha(installedBytes),
      upstream_blob_sha256: sha(pinnedBytes),
      upstream_repo: '../vibe-coding-skills',
      upstream_path: `../vibe-coding-skills/${relativeSource}`,
      upstream_revision: revision,
    };
    entry.license = {
      status: 'verified', spdx: 'MIT',
      evidence: '%APPDATA%/chat-on-steroids/skills/constellation/LICENSE',
      sha256: sha(installedLicense), upstream_blob_sha256: sha(pinnedLicense),
    };
    entry.evidence = {
      command: receipt.command,
      output: `Observed ${receipt.checks.length} independently replayable offline checks against actual Constellation source and controlled negative fixtures. Source/license/managed install matched pinned local Git. No runtime MCP or publisher authentication.`,
    };
    entry.exercise = {
      command: receipt.command, outcome: 'passed', result: receipt.purpose,
      observed_on: receipt.observed_on,
      receipt_path: `skills-manifest/evidence/reviewed/${name}.json`,
      receipt_sha256: sha(receiptBytes),
      runner_path: 'skills-manifest/reviewed-exercise.mjs',
      runner_sha256: sha(readFileSync(join(here, 'reviewed-exercise.mjs'))),
    };
    entry.limitations = [
      'Local development audit with bounded source/SQL fixtures; neither full feature validation nor runtime app integration',
      ...receipt.limitations,
    ];
  }
  const validated = validateManifest(manifest, { verifyLocal: true });
  if (!validated.ok) throw new Error('Refusing promotion: ' + validated.errors.join('; '));
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  process.stdout.write(JSON.stringify(validated.counts, null, 2) + '\n');
}

try {
  main();
} catch (error) {
  process.stderr.write(`Qualification blocked: ${error.message}\n`);
  process.exitCode = 1;
}
