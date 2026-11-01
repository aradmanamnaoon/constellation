#!/usr/bin/env node
// Strict independent promotion of two original source-reviewed development skills.
// A receipt or install alone cannot grant qualification. No source installation.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { isDeepStrictEqual } from 'node:util';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { twentySecondNames, runTwentySecond } from './twenty-second-exercise.mjs';
import { validateManifest } from './validate.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const upstream = resolve(repo, '..', 'vibe-coding-skills');
const managed = join(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation');
const manifestPath = join(here, 'inventory.json');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const norm = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));

function main() {
  if (process.argv.length !== 2) throw new Error('Usage: node skills-manifest/qualify-twenty-second.mjs');
  if (!process.env.APPDATA) throw new Error('Missing APPDATA managed installation path');
  const upstreamStatus = execFileSync('git', ['-C', upstream, 'status', '--porcelain'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (upstreamStatus) throw new Error('Upstream dirty');
  const revision = execFileSync('git', ['-C', upstream, 'rev-parse', 'HEAD'],
    {encoding: 'utf8', windowsHide: true}).trim();
  const origin = execFileSync('git', ['-C', upstream, 'remote', 'get-url', 'origin'],
    {encoding: 'utf8', windowsHide: true}).trim();
  if (!/^[a-f0-9]{40}$/.test(revision) || !/^https:\/\/github\.com\/aradmanamnaoon\/vibe-coding-skills\.git$/.test(origin)) {
    throw new Error('Unrecognized pinned upstream repository identity');
  }
  const installedLicense = readFileSync(join(managed, 'LICENSE'));
  const pinnedLicense = execFileSync('git', ['-C', upstream, 'show', revision + ':LICENSE'],
    {windowsHide: true});
  if (sha(norm(installedLicense)) !== sha(pinnedLicense)
    || !installedLicense.toString('utf8').includes('MIT License')) {
    throw new Error('Unverified installed MIT license');
  }
  const raw = readFileSync(manifestPath);
  const manifest = JSON.parse(raw.toString('utf8'));
  const seenSourceHashes = new Set();
  for (const name of twentySecondNames) {
    const entry = manifest.entries.find((item) => item.id === `skill:${name}`);
    if (!entry || entry.kind !== 'skill'
      || !['source_reviewed', 'installed_vetted'].includes(entry.status)) {
      throw new Error('Unexpected candidate status: ' + name);
    }
    const current = runTwentySecond(name);
    if (current.outcome !== 'passed' || !current.installed_source_sha256
      || current.upstream.revision !== revision
      || seenSourceHashes.has(current.upstream.sha256)) {
      throw new Error('No distinct verified installation and exercise: ' + name);
    }
    seenSourceHashes.add(current.upstream.sha256);
    const receiptPath = join(here, 'evidence', 'twenty-second', name + '.json');
    const receiptBytes = readFileSync(receiptPath);
    const receipt = JSON.parse(receiptBytes);
    if (!isDeepStrictEqual(receipt, {...current, observed_on: receipt.observed_on})
      || !/^\d{4}-\d{2}-\d{2}$/.test(receipt.observed_on)) {
      throw new Error('Distinct receipt differs from independent current replay: ' + name);
    }
    entry.status = 'installed_vetted';
    entry.source = {
      path: `%APPDATA%/chat-on-steroids/skills/constellation/${name}/SKILL.md`,
      provenance: `Exact installed local copy of ${origin} at commit ${revision}; publisher identity not independently authenticated`,
      sha256: current.upstream.sha256,
      upstream_blob_sha256: current.upstream.pinned_blob_sha256,
      upstream_repo: '../vibe-coding-skills',
      upstream_path: current.upstream.path,
      upstream_revision: revision,
    };
    entry.evidence = {
      command: current.command,
      output: `${receipt.checks.length} real, individually replayable local audit assertions, including separate adverse fixtures and matched pinned source/license/managed install. No runtime MCP.`,
    };
    entry.license = {
      status: 'verified',
      spdx: 'MIT',
      evidence: '%APPDATA%/chat-on-steroids/skills/constellation/LICENSE',
      sha256: sha(installedLicense),
      upstream_blob_sha256: sha(pinnedLicense),
    };
    entry.exercise = {
      command: current.command,
      outcome: 'passed',
      result: current.purpose,
      observed_on: receipt.observed_on,
      receipt_path: `skills-manifest/evidence/twenty-second/${name}.json`,
      receipt_sha256: sha(receiptBytes),
      runner_path: 'skills-manifest/twenty-second-exercise.mjs',
      runner_sha256: current.exercise_script_sha256,
    };
    entry.limitations = [...current.limitations];
  }
  // Validate the candidate schema and all local identity/receipt checks.
  // Older entries may be stale due to concurrent changes; never relabel them
  // or overwrite their receipts to force the aggregate score upward.
  const validated = validateManifest(manifest, {verifyLocal: true});
  for (const name of twentySecondNames) {
    const index = manifest.entries.findIndex((item) => item.id === `skill:${name}`);
    if (validated.errors.some((item) => item.startsWith(`entries[${index}]`))) {
      throw new Error('New candidate failed strict independent verification: ' + name);
    }
  }
  if (sha(readFileSync(manifestPath)) !== sha(raw)) {
    throw new Error('Inventory changed concurrently during qualification');
  }
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  process.stdout.write(JSON.stringify({
    promoted: twentySecondNames,
    current_verified_count: validated.counts.vetted_installed_exercised,
    other_stale_receipt_errors: validated.errors.filter((item) => item.startsWith('entries[')),
  }, null, 2) + '\n');
}

try {
  main();
} catch (error) {
  process.stderr.write('Twenty-second-wave qualification blocked: ' + error.message + '\n');
  process.exitCode = 1;
}
