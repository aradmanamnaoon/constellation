#!/usr/bin/env node
// Offline refresh of PREVIOUSLY installed/vetted skills only. Check mode is
// read-only; --write executes each approved project-specific verifier and
// refreshes its distinct receipt only while upstream and inputs remain stable.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { exercise } from './exercise-local.mjs';
import { runCandidate } from './reviewed-exercise.mjs';
import { runSeventeenth } from './seventeenth-exercise.mjs';
import { runEighteenth } from './eighteenth-exercise.mjs';
import { runNineteenth } from './nineteenth-exercise.mjs';
import { runTwentySecond } from './twenty-second-exercise.mjs';
import { runTwentyFifth } from './twenty-fifth-exercise.mjs';
import { runTwentySixth } from './twenty-sixth-exercise.mjs';
import { validateManifest } from './validate.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const upstream = resolve(root, '..', 'vibe-coding-skills');
const managed = resolve(process.env.APPDATA ?? '', 'chat-on-steroids', 'skills', 'constellation');
const manifestPath = join(here, 'inventory.json');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const normalize = (bytes) => Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
const inside = (parent, child) => {
  const rel = relative(parent, child);
  return !!rel && rel !== '..' && !rel.startsWith('../')
    && !rel.startsWith('..\\') && !isAbsolute(rel);
};
const pinned = (revision, path) => execFileSync('git', [
  '-C', upstream, 'show', revision + ':' + path,
], { windowsHide: true, maxBuffer: 1024 * 1024, stdio: ['ignore', 'pipe', 'ignore'] });

export function inspectInstalled(entry, seen = new Set()) {
  if (entry.kind !== 'skill' || entry.status !== 'installed_vetted'
    || !/^skill:[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(entry.id)) {
    throw new Error('not a pre-qualified installed skill');
  }
  const name = entry.id.slice(6);
  const expected = resolve(managed, name, 'SKILL.md');
  const installed = resolve(root, entry.source.path.replace('%APPDATA%', process.env.APPDATA ?? ''));
  if (expected.toLowerCase() !== installed.toLowerCase()
    || !statSync(installed).isFile()
    || realpathSync(expected).toLowerCase() !== realpathSync(installed).toLowerCase()
    || !inside(realpathSync(managed).toLowerCase(), realpathSync(installed).toLowerCase())) {
    throw new Error('managed installation identity/realpath mismatch');
  }
  const installedBytes = readFileSync(installed);
  if (sha(installedBytes) !== entry.source.sha256
    || !installedBytes.toString('utf8').split(/\r?\n/).slice(0, 12).includes('name: ' + name)) {
    throw new Error('installed file hash/frontmatter mismatch');
  }
  const source = resolve(root, entry.source.upstream_path);
  if (resolve(root, entry.source.upstream_repo) !== upstream
    || !inside(upstream, source) || !inside(realpathSync(upstream), realpathSync(source))
    || !/^[a-f0-9]{40}$/.test(entry.source.upstream_revision)
    || sha(readFileSync(source)) !== sha(installedBytes)
    || sha(normalize(installedBytes)) !== entry.source.upstream_blob_sha256
    || sha(pinned(entry.source.upstream_revision,
      relative(upstream, source).replaceAll('\\', '/'))) !== entry.source.upstream_blob_sha256) {
    throw new Error('pinned Git source mismatch');
  }
  const license = readFileSync(join(managed, 'LICENSE'));
  if (entry.license.status !== 'verified' || entry.license.spdx !== 'MIT'
    || sha(license) !== entry.license.sha256
    || !license.toString('utf8').includes('MIT License')
    || sha(normalize(license)) !== sha(pinned(entry.source.upstream_revision, 'LICENSE'))) {
    throw new Error('pinned MIT license mismatch');
  }
  const runnerName = entry.exercise.runner_path.endsWith('/reviewed-exercise.mjs')
    ? 'reviewed-exercise.mjs'
    : entry.exercise.runner_path.endsWith('/seventeenth-exercise.mjs')
      ? 'seventeenth-exercise.mjs'
      : entry.exercise.runner_path.endsWith('/eighteenth-exercise.mjs')
        ? 'eighteenth-exercise.mjs'
        : entry.exercise.runner_path.endsWith('/nineteenth-exercise.mjs')
          ? 'nineteenth-exercise.mjs'
          : entry.exercise.runner_path.endsWith('/twenty-second-exercise.mjs')
            ? 'twenty-second-exercise.mjs'
            : entry.exercise.runner_path.endsWith('/twenty-fifth-exercise.mjs')
              ? 'twenty-fifth-exercise.mjs'
              : entry.exercise.runner_path.endsWith('/twenty-sixth-exercise.mjs')
                ? 'twenty-sixth-exercise.mjs'
      : 'exercise-local.mjs';
  const runner = resolve(root, entry.exercise.runner_path);
  if (runner !== join(here, runnerName)) throw new Error('unsupported verifier path');
  const receipt = resolve(root, entry.exercise.receipt_path);
  const expectedReceipt = join(here, 'evidence',
    runnerName === 'reviewed-exercise.mjs' ? 'reviewed'
      : runnerName === 'seventeenth-exercise.mjs' ? 'seventeenth'
        : runnerName === 'eighteenth-exercise.mjs' ? 'eighteenth'
          : runnerName === 'nineteenth-exercise.mjs' ? 'nineteenth'
            : runnerName === 'twenty-second-exercise.mjs' ? 'twenty-second'
              : runnerName === 'twenty-fifth-exercise.mjs' ? 'twenty-fifth'
                : runnerName === 'twenty-sixth-exercise.mjs' ? 'twenty-sixth' : '', name + '.json');
  if (receipt !== expectedReceipt || !inside(join(here, 'evidence'), receipt)
    || realpathSync(receipt).toLowerCase() !== receipt.toLowerCase()) {
    throw new Error('forged/aliased receipt path');
  }
  if (seen.has(receipt.toLowerCase())) throw new Error('duplicate receipt');
  seen.add(receipt.toLowerCase());
  return { name, runner, runnerName, receipt };
}

const verify = ({ name, runnerName }) => runnerName === 'reviewed-exercise.mjs'
  ? runCandidate(name) : runnerName === 'seventeenth-exercise.mjs'
    ? runSeventeenth(name) : runnerName === 'eighteenth-exercise.mjs'
      ? runEighteenth(name) : runnerName === 'nineteenth-exercise.mjs'
        ? runNineteenth(name) : runnerName === 'twenty-second-exercise.mjs'
          ? runTwentySecond(name) : runnerName === 'twenty-fifth-exercise.mjs'
            ? runTwentyFifth(name) : runnerName === 'twenty-sixth-exercise.mjs'
              ? runTwentySixth(name) : exercise(name);

export function refresh({ write = false } = {}) {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const seen = new Set();
  const outcomes = [];
  let renewed = 0;
  for (const entry of manifest.entries.filter((item) => item.status === 'installed_vetted')) {
    try {
      const state = inspectInstalled(entry, seen);
      const first = verify(state);
      const second = verify(state);
      const stable = isDeepStrictEqual(first, second);
      if (!stable || first.outcome !== 'passed') {
        outcomes.push({ id: entry.id, status: stable ? 'failing_checks' : 'unstable_source',
          failed_checks: first.checks.filter((c) => !['pass', 'passed'].includes(c.status ?? c.result))
            .map((c) => c.name ?? c.check) });
        continue;
      }
      if (write) {
        // The actual independent CLI runs; this does not synthesize receipts.
        execFileSync(process.execPath, [state.runner, state.name, '--write-receipt'],
          { cwd: root, windowsHide: true, timeout: 30000, stdio: ['ignore', 'pipe', 'pipe'] });
        const receiptBytes = readFileSync(state.receipt);
        const receipt = JSON.parse(receiptBytes);
        const after = verify(state);
        const expected = state.runnerName === 'exercise-local.mjs'
          ? { ...after, observed_on: receipt.observed_on, scope: 'static_source_contract' }
          : { ...after, observed_on: receipt.observed_on };
        if (after.outcome !== 'passed' || !isDeepStrictEqual(first, after)
          || !isDeepStrictEqual(receipt, expected)) {
          outcomes.push({ id: entry.id, status: 'changed_during_execution' });
          continue;
        }
        entry.exercise.receipt_sha256 = sha(receiptBytes);
        entry.exercise.runner_sha256 = sha(readFileSync(state.runner));
        entry.exercise.observed_on = receipt.observed_on;
        renewed++;
      }
      outcomes.push({ id: entry.id, status: write ? 'refreshed' : 'eligible_stable' });
    } catch (error) {
      outcomes.push({ id: entry.id, status: 'blocked', reason: String(error.message) });
    }
  }
  if (write && renewed) writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  const verified = validateManifest(manifest, { verifyLocal: true });
  return {
    renewed, counts: verified.counts, strict_validation_ok: verified.ok,
    strict_errors: verified.errors, outcomes,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length > 3 || !['--check', '--write'].includes(process.argv[2] ?? '--check')) {
    process.stderr.write('Usage: node skills-manifest/requalify-stable.mjs [--check|--write]\n');
    process.exitCode = 2;
  } else {
    const result = refresh({ write: process.argv[2] === '--write' });
    process.stdout.write(JSON.stringify(result, null, 2) + '\n');
    if (result.outcomes.some((item) => item.status === 'blocked')) process.exitCode = 1;
  }
}
