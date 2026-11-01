#!/usr/bin/env node
// Promote only after independent installed source, MIT license and read-only
// real operational audit all agree. Existing unrelated credit may be stale.
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {isDeepStrictEqual} from 'node:util';
import {runTwentyFifth} from './twenty-fifth-exercise.mjs';
import {validateManifest} from './validate.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstream = resolve(root, '..', 'vibe-coding-skills');
const path = join(here, 'inventory.json');
const name = 'production-readiness-auditor';
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const norm = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));

function qualify() {
  if (process.argv.length !== 2) throw new Error('Unexpected arguments');
  if (!process.env.APPDATA) throw new Error('Missing managed APPDATA path');
  if (execFileSync('git', ['-C', upstream, 'status', '--porcelain'],
    {encoding: 'utf8', windowsHide: true}).trim()) {
    throw new Error('Upstream checkout changed');
  }
  const revision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const origin = execFileSync('git', ['-C', upstream, 'remote', 'get-url', 'origin'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (!/^[a-f0-9]{40}$/.test(revision)
    || origin !== 'https://github.com/aradmanamnaoon/vibe-coding-skills.git') {
    throw new Error('Unrecognized pinned Git source identity');
  }
  const sourcePath = 'skills/deployment/production-readiness-auditor/SKILL.md';
  const source = readFileSync(join(upstream, sourcePath));
  const pinned = execFileSync('git',
    ['-C', upstream, 'show', `${revision}:${sourcePath}`], {windowsHide: true});
  const installedPath = join(process.env.APPDATA, 'chat-on-steroids', 'skills',
    'constellation', name, 'SKILL.md');
  const installed = readFileSync(installedPath);
  const license = readFileSync(join(process.env.APPDATA, 'chat-on-steroids',
    'skills', 'constellation', 'LICENSE'));
  const pinnedLicense = execFileSync('git',
    ['-C', upstream, 'show', `${revision}:LICENSE`], {windowsHide: true});
  if (sha(source) !== sha(installed) || sha(norm(source)) !== sha(pinned)
    || sha(norm(license)) !== sha(pinnedLicense)
    || !license.toString('utf8').includes('MIT License')) {
    throw new Error('Installed source or verified license mismatch');
  }
  const raw = readFileSync(path);
  const manifest = JSON.parse(raw);
  const entry = manifest.entries.find((item) => item.id === `skill:${name}`);
  if (!entry || !['source_reviewed', 'installed_vetted'].includes(entry.status)
    || entry.source.sha256 !== sha(source)) {
    throw new Error('Source-reviewed candidate identity mismatch');
  }
  const first = runTwentyFifth(name);
  const second = runTwentyFifth(name);
  if (first.outcome !== 'passed' || !first.installed_source_sha256
    || !isDeepStrictEqual(first, second)) {
    throw new Error('Current real preflight exercise unstable, blocked or not installed');
  }
  const receiptPath = join(here, 'evidence', 'twenty-fifth', name + '.json');
  const receiptBytes = readFileSync(receiptPath);
  const receipt = JSON.parse(receiptBytes);
  if (!isDeepStrictEqual(receipt, {...first, observed_on: receipt.observed_on})
    || !/^\d{4}-\d{2}-\d{2}$/.test(receipt.observed_on)) {
    throw new Error('Exercise receipt not identical to independently replayed audit');
  }
  entry.status = 'installed_vetted';
  entry.source = {
    path: `%APPDATA%/chat-on-steroids/skills/constellation/${name}/SKILL.md`,
    provenance: `Exact managed local copy of ${origin} at ${revision}; account identity independently unverified`,
    sha256: sha(installed), upstream_blob_sha256: sha(pinned),
    upstream_repo: '../vibe-coding-skills',
    upstream_path: `../vibe-coding-skills/${sourcePath}`,
    upstream_revision: revision,
  };
  entry.evidence = {
    command: first.command,
    output: '10 independent assertions of an actual failed Windows release preflight, Git status/remote/lockfile corroboration and adverse false-green checks; production readiness is NOT certified; no MCP runtime.',
  };
  entry.license = {
    status: 'verified', spdx: 'MIT',
    evidence: '%APPDATA%/chat-on-steroids/skills/constellation/LICENSE',
    sha256: sha(license), upstream_blob_sha256: sha(pinnedLicense),
  };
  entry.exercise = {
    command: first.command, outcome: 'passed', result: first.purpose,
    observed_on: receipt.observed_on,
    receipt_path: `skills-manifest/evidence/twenty-fifth/${name}.json`,
    receipt_sha256: sha(receiptBytes),
    runner_path: 'skills-manifest/twenty-fifth-exercise.mjs',
    runner_sha256: first.exercise_script_sha256,
  };
  entry.limitations = [...first.limitations];
  const validated = validateManifest(manifest, {verifyLocal: true});
  const index = manifest.entries.findIndex((item) => item.id === `skill:${name}`);
  if (validated.errors.some((item) => item.startsWith(`entries[${index}]`))) {
    throw new Error('Strict independent validation rejected new candidate');
  }
  if (sha(readFileSync(path)) !== sha(raw)) {
    throw new Error('Inventory changed concurrently');
  }
  writeFileSync(path, JSON.stringify(manifest, null, 2) + '\n');
  process.stdout.write(JSON.stringify({
    promoted: [name], verified_count: validated.counts.vetted_installed_exercised,
    other_stale_or_failed_checks: validated.errors,
  }, null, 2) + '\n');
}

try { qualify(); }
catch (error) {
  process.stderr.write('Twenty-fifth-wave qualification blocked: ' + error.message + '\n');
  process.exitCode = 1;
}
