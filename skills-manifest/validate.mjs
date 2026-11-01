#!/usr/bin/env node
// Standard-library-only verification. Read-only even in --verify-local mode.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { resolve, dirname, isAbsolute, relative, posix } from 'node:path';
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

const manifestDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(manifestDir, '..');
const ID_PATTERN = /^(cli|skill|mcp):[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
const STATES = {
  cli: new Set(['observed_available']),
  skill: new Set(['source_reviewed', 'installed_unvetted', 'installed_vetted']),
  mcp: new Set(['configured_unverified', 'installed_vetted']),
};

const isNonEmpty = (value) => typeof value === 'string' && value.trim().length > 0;
const isObject = (value) =>
  value !== null && typeof value === 'object' && !Array.isArray(value);
const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');
const canonicalText = (buffer) =>
  Buffer.from(buffer.toString('utf8').replaceAll('\r\n', '\n'));
const inside = (parent, file) => {
  const child = relative(parent, file);
  return !!child && child !== '..' && !child.startsWith('../')
    && !child.startsWith('..\\') && !isAbsolute(child);
};

export function resolveEvidencePath(location, environment = process.env) {
  if (!isNonEmpty(location)) throw new Error('Missing path');
  let result = location;
  const matches = [...location.matchAll(/%([A-Z][A-Z0-9_]*)%/g)];
  for (const match of matches) {
    const replacement = environment[match[1]];
    if (!isNonEmpty(replacement)) throw new Error('Missing environment variable for evidence path');
    result = result.replaceAll(match[0], replacement);
  }
  if (/%[A-Z][A-Z0-9_]*%/.test(result)) {
    throw new Error('Unexpanded evidence variable');
  }
  return isAbsolute(result) ? resolve(result) : resolve(repoRoot, result);
}

/**
 * Returns problems and derived totals. Never trusts a claim of 200 entries
 * merely because it is written in JSON. CLI tools never count as skills/MCPs.
 *
 * @param {unknown} manifest
 * @param {{verifyLocal?:boolean,requireTarget?:boolean,environment?:NodeJS.ProcessEnv}} [options]
 */
export function validateManifest(manifest, options = {}) {
  const errors = [];
  const counts = {
    total_entries: 0,
    cli_observed: 0,
    skill_source_reviewed: 0,
    skill_installed_unvetted: 0,
    mcp_configured_unverified: 0,
    vetted_installed_exercised: 0,
    remaining_to_target: 0,
  };
  if (!isObject(manifest)) return { ok: false, errors: ['manifest must be an object'], counts };
  if (manifest.schema_version !== 1) errors.push('schema_version must equal 1');
  if (manifest.target_vetted_skill_mcp_count !== 200) {
    errors.push('target_vetted_skill_mcp_count must equal 200');
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(manifest.observed_on ?? '')) {
    errors.push('observed_on must use YYYY-MM-DD');
  }
  if (!Array.isArray(manifest.entries)) {
    errors.push('entries must be an array');
    return { ok: false, errors, counts };
  }
  counts.total_entries = manifest.entries.length;
  const seen = new Set();
  const seenCapability = new Set();
  // A renamed manifest record does not create a second installed capability.
  // Compare normalized locations as well as names: two distinct IDs must not
  // spend one source artifact, pinned upstream file or exercise receipt twice.
  const observedSources = new Set();
  const vettedUpstreams = new Set();
  const vettedReceipts = new Set();
  const uniqueLocation = (value) => posix.normalize(
    String(value).trim().replaceAll('\\', '/'),
  ).toLowerCase();
  for (const [index, entry] of manifest.entries.entries()) {
    const at = `entries[${index}]`;
    const errorsBeforeEntry = errors.length;
    let eligibleVetted = false;
    if (!isObject(entry)) {
      errors.push(`${at} must be an object`);
      continue;
    }
    if (!ID_PATTERN.test(entry.id ?? '')) errors.push(`${at}.id has invalid format`);
    const normalized = String(entry.id).toLowerCase();
    if (seen.has(normalized)) errors.push(`${at} duplicate ID: ${normalized}`);
    seen.add(normalized);
    if (!Object.hasOwn(STATES, entry.kind)) {
      errors.push(`${at}.kind must be cli, skill or mcp`);
      continue;
    }
    if (!normalized.startsWith(`${entry.kind}:`)) {
      errors.push(`${at}.id must match kind`);
    }
    if (!STATES[entry.kind].has(entry.status)) {
      errors.push(`${at}.status invalid for ${entry.kind}`);
    }
    if (entry.kind !== 'cli') {
      const canonical = normalized.replace(/^(skill|mcp):/, '');
      if (seenCapability.has(canonical)) {
        errors.push(`${at} duplicates a development capability name: ${canonical}`);
      }
      seenCapability.add(canonical);
      if (isNonEmpty(entry.source?.path)) {
        const sourceKey = uniqueLocation(entry.source.path);
        if (observedSources.has(sourceKey)) {
          errors.push(`${at} reuses a source artifact already assigned to another skill/MCP`);
        }
        observedSources.add(sourceKey);
      }
    }
    if (!isNonEmpty(entry.purpose)) errors.push(`${at}.purpose missing`);
    if (!isObject(entry.source) || !isNonEmpty(entry.source.path)
      || !isNonEmpty(entry.source.provenance)) {
      errors.push(`${at}.source requires path and provenance`);
    }
    if (!isObject(entry.evidence) || !isNonEmpty(entry.evidence.command)
      || !isNonEmpty(entry.evidence.output)) {
      errors.push(`${at}.evidence requires observed command and output`);
    }
    if (!isObject(entry.license)
      || !['verified', 'unverified'].includes(entry.license.status)) {
      errors.push(`${at}.license requires verified/unverified status`);
    } else if (entry.license.status === 'verified') {
      if (!isNonEmpty(entry.license.spdx) || !isNonEmpty(entry.license.evidence)) {
        errors.push(`${at}.license verified requires SPDX and evidence path`);
      }
    } else if (entry.license.spdx !== null || entry.license.evidence !== null) {
      errors.push(`${at}.license unverified must use null SPDX and evidence`);
    }
    if (!Array.isArray(entry.limitations)
      || entry.limitations.length === 0 || entry.limitations.some((item) => !isNonEmpty(item))) {
      errors.push(`${at}.limitations must name concrete limitations`);
    }
    if (entry.kind === 'cli') {
      counts.cli_observed++;
      if (!isNonEmpty(entry.evidence?.version)) {
        errors.push(`${at}.evidence.version is required for a CLI`);
      }
    } else {
      if (!/^[a-f0-9]{64}$/.test(entry.source?.sha256 ?? '')) {
        errors.push(`${at}.source.sha256 must be a real SHA-256 digest`);
      }
      if (!entry.source?.path?.replaceAll('\\', '/').endsWith('/SKILL.md')
        && entry.kind === 'skill') {
        errors.push(`${at} skill source path must end with SKILL.md`);
      }
      if (entry.status === 'source_reviewed') {
        counts.skill_source_reviewed++;
        if (entry.license?.status !== 'verified') {
          errors.push(`${at}: source_reviewed requires verified license`);
        }
      }
      if (entry.status === 'installed_unvetted') counts.skill_installed_unvetted++;
      if (entry.status === 'configured_unverified') counts.mcp_configured_unverified++;
      if (entry.status === 'installed_vetted') {
        if (isNonEmpty(entry.source?.upstream_repo)
          && isNonEmpty(entry.source?.upstream_path)) {
          const upstreamKey = `${uniqueLocation(entry.source.upstream_repo)}::${uniqueLocation(entry.source.upstream_path)}`;
          if (vettedUpstreams.has(upstreamKey)) {
            errors.push(`${at} reuses another vetted entry's pinned upstream source`);
          }
          vettedUpstreams.add(upstreamKey);
        }
        if (isNonEmpty(entry.exercise?.receipt_path)) {
          const receiptKey = uniqueLocation(entry.exercise.receipt_path);
          if (vettedReceipts.has(receiptKey)) {
            errors.push(`${at} reuses another vetted entry's exercise receipt`);
          }
          vettedReceipts.add(receiptKey);
        }
        if (entry.license?.status !== 'verified'
          || !/^[a-f0-9]{64}$/.test(entry.license?.sha256 ?? '')
          || !/^[a-f0-9]{40}$/.test(entry.source?.upstream_revision ?? '')
          || !/^[a-f0-9]{64}$/.test(entry.source?.upstream_blob_sha256 ?? '')
          || !isNonEmpty(entry.source?.upstream_repo)
          || !isNonEmpty(entry.source?.upstream_path)
          || !/^[a-f0-9]{64}$/.test(entry.license?.upstream_blob_sha256 ?? '')
          || !isObject(entry.exercise)
          || !isNonEmpty(entry.exercise?.command)
          || !isNonEmpty(entry.exercise?.result)
          || entry.exercise?.outcome !== 'passed'
          || !isNonEmpty(entry.exercise?.observed_on)
          || !isNonEmpty(entry.exercise?.receipt_path)
          || !isNonEmpty(entry.exercise?.runner_path)
          || !/^[a-f0-9]{64}$/.test(entry.exercise?.runner_sha256 ?? '')
          || !/^[a-f0-9]{64}$/.test(entry.exercise?.receipt_sha256 ?? '')) {
          errors.push(`${at}: installed_vetted requires pinned upstream provenance, verified license, and passing exercise plus hashed runner/receipt`);
        } else {
          eligibleVetted = true;
        }
      }
    }
    if (options.verifyLocal && isObject(entry.source) && isNonEmpty(entry.source.path)) {
      try {
        const location = resolveEvidencePath(entry.source.path, options.environment);
        if (!existsSync(location) || !statSync(location).isFile()) {
          errors.push(`${at} evidence source file missing`);
        } else if (entry.kind === 'skill' || entry.kind === 'mcp') {
          const file = readFileSync(location);
          const digest = createHash('sha256').update(file).digest('hex');
          if (digest !== entry.source.sha256) errors.push(`${at} SHA256 mismatch`);
          if (entry.kind === 'skill') {
            // Exact front matter name is a basic provenance check, not a code safety audit.
            const frontMatter = file.toString('utf8').split(/\r?\n/).slice(0, 12);
            const expectedName = entry.id.slice('skill:'.length);
            if (!frontMatter.includes(`name: ${expectedName}`)) {
              errors.push(`${at} frontmatter skill name mismatch`);
            }
          }
        }
      } catch {
        errors.push(`${at} could not verify local source`);
      }
    }
    if (options.verifyLocal && entry.license?.status === 'verified') {
      try {
        const source = resolveEvidencePath(entry.license.evidence, options.environment);
        if (!existsSync(source) || !statSync(source).isFile()) {
          errors.push(`${at} license evidence missing`);
        } else {
          const text = readFileSync(source, 'utf8');
          const expected = entry.license.spdx;
          if (expected === 'MIT' && !text.includes('MIT License')) {
            errors.push(`${at} license evidence does not contain MIT license text`);
          }
          if (entry.status === 'installed_vetted'
            && sha256(readFileSync(source)) !== entry.license.sha256) {
            errors.push(`${at} installed license fingerprint mismatch`);
          }
          if (expected === 'MIT OR Unlicense'
            && !(text.includes('Unlicense') && text.includes('MIT'))) {
            errors.push(`${at} license evidence does not contain both licenses`);
          }
        }
      } catch {
        errors.push(`${at} could not verify license evidence`);
      }
    }
    if (options.verifyLocal && entry.status === 'installed_vetted'
      && entry.kind === 'skill') {
      try {
        const installed = resolveEvidencePath(entry.source.path, options.environment);
        const managedRoot = resolveEvidencePath(
          '%APPDATA%/chat-on-steroids/skills/constellation', options.environment,
        );
        const expected = resolve(managedRoot, entry.id.slice('skill:'.length), 'SKILL.md');
        if (installed.toLowerCase() !== expected.toLowerCase()) {
          errors.push(`${at} vetted skill must use its actual managed installation path`);
        }
        const upstreamRepo = resolveEvidencePath(entry.source.upstream_repo, options.environment);
        const upstream = resolveEvidencePath(entry.source.upstream_path, options.environment);
        if (!inside(upstreamRepo, upstream)) {
          errors.push(`${at} upstream source must be within the pinned repository`);
        } else {
          const relativeUpstream = relative(upstreamRepo, upstream).replaceAll('\\', '/');
          const pinnedFile = execFileSync('git', [
            '-C', upstreamRepo, 'show',
            `${entry.source.upstream_revision}:${relativeUpstream}`,
          ], { maxBuffer: 1024 * 1024, windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
          if (sha256(pinnedFile) !== entry.source.upstream_blob_sha256
            || sha256(canonicalText(readFileSync(upstream))) !== entry.source.upstream_blob_sha256
            || sha256(readFileSync(upstream)) !== entry.source.sha256) {
            errors.push(`${at} installed skill does not match pinned upstream revision`);
          }
          const pinnedLicense = execFileSync('git', [
            '-C', upstreamRepo, 'show', `${entry.source.upstream_revision}:LICENSE`,
          ], { maxBuffer: 1024 * 1024, windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
          if (sha256(pinnedLicense) !== entry.license.upstream_blob_sha256
            || sha256(canonicalText(readFileSync(
              resolveEvidencePath(entry.license.evidence, options.environment),
            ))) !== entry.license.upstream_blob_sha256) {
            errors.push(`${at} installed license does not match pinned upstream revision`);
          }
        }
      } catch {
        errors.push(`${at} could not verify the pinned installed source and license`);
      }
    }
    if (options.verifyLocal && entry.status === 'installed_vetted'
      && isObject(entry.exercise) && isNonEmpty(entry.exercise.receipt_path)) {
      try {
        const receiptPath = resolveEvidencePath(entry.exercise.receipt_path, options.environment);
        const evidenceRoots = [
          resolve(repoRoot, 'docs', 'dev-tools', 'evidence'),
          resolve(manifestDir, 'evidence'),
        ];
        if (!evidenceRoots.some((dir) => inside(dir, receiptPath))) {
          errors.push(`${at} exercise receipt must be under approved local evidence directories`);
        } else if (!existsSync(receiptPath) || !statSync(receiptPath).isFile()) {
          errors.push(`${at} exercise receipt does not exist`);
        } else {
          const file = readFileSync(receiptPath);
          if (sha256(file) !== entry.exercise.receipt_sha256) {
            errors.push(`${at} exercise receipt SHA256 mismatch`);
          } else {
            const receipt = JSON.parse(file.toString('utf8'));
            const runner = resolveEvidencePath(entry.exercise.runner_path, options.environment);
            if (!inside(manifestDir, runner) || !statSync(runner).isFile()
              || sha256(readFileSync(runner)) !== entry.exercise.runner_sha256) {
              errors.push(`${at} pinned exercise runner missing or modified`);
            }
            // A digest can attest to the bytes of a receipt without proving
            // that its assertions were executed. Re-run only the reviewed,
            // effect-free Constellation exercise implementation on this host.
            // Unknown skill names and MCPs fail closed until their own
            // independently reviewable verification is implemented.
            if (runner === resolve(manifestDir, 'reviewed-exercise.mjs')) {
              try {
                const current = runCandidate(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} reviewed exercise receipt differs from independently replayed checks`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this reviewed skill`);
              }
            } else if (runner === resolve(manifestDir, 'seventeenth-exercise.mjs')) {
              try {
                const current = runSeventeenth(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} seventeenth-wave exercise differs from independently replayed checks`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this seventeenth-wave skill`);
              }
            } else if (runner === resolve(manifestDir, 'eighteenth-exercise.mjs')) {
              try {
                const current = runEighteenth(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} eighteenth-wave exercise differs from independently replayed checks`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this eighteenth-wave skill`);
              }
            } else if (runner === resolve(manifestDir, 'nineteenth-exercise.mjs')) {
              try {
                const current = runNineteenth(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} nineteenth-wave exercise differs from independently replayed checks`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this nineteenth-wave skill`);
              }
            } else if (runner === resolve(manifestDir, 'twenty-second-exercise.mjs')) {
              try {
                const current = runTwentySecond(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} twenty-second-wave exercise differs from independently replayed checks`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this twenty-second-wave skill`);
              }
            } else if (runner === resolve(manifestDir, 'twenty-fifth-exercise.mjs')) {
              try {
                const current = runTwentyFifth(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} twenty-fifth-wave operational audit differs from independent real preflight replay`);
                }
              } catch {
                errors.push(`${at} no approved real operational readiness verifier for this skill`);
              }
            } else if (runner === resolve(manifestDir, 'twenty-sixth-exercise.mjs')) {
              try {
                const current = runTwentySixth(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                })) {
                  errors.push(`${at} twenty-sixth-wave measured source-read exercise differs from independent replay`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this twenty-sixth-wave skill`);
              }
            } else if (runner !== resolve(manifestDir, 'exercise-local.mjs')) {
              errors.push(`${at} no approved executable verifier for this receipt`);
            } else {
              try {
                const current = exercise(entry.id.slice('skill:'.length));
                if (current.outcome !== 'passed' || !isDeepStrictEqual(receipt, {
                  ...current,
                  observed_on: receipt.observed_on,
                  scope: 'static_source_contract',
                })) {
                  errors.push(`${at} exercise receipt differs from independently replayed checks`);
                }
              } catch {
                errors.push(`${at} no approved executable verifier for this skill/MCP`);
              }
            }
            if (receipt.id !== entry.id || receipt.project !== 'Constellation'
              || receipt.command !== entry.exercise.command
              || (receipt.objective ?? receipt.purpose) !== entry.exercise.result
              || receipt.outcome !== 'passed'
              || receipt.installed_source_sha256 !== entry.source.sha256
              || receipt.exercise_script_sha256 !== entry.exercise.runner_sha256
              || receipt.observed_on !== entry.exercise.observed_on
              || !Array.isArray(receipt.checks) || receipt.checks.length < 3
              || !receipt.checks.every((check) =>
                check.status === 'passed' || check.result === 'pass')) {
              errors.push(`${at} exercise receipt assertions or source identity invalid`);
            }
            if (!isObject(receipt.inputs) || !Object.keys(receipt.inputs).length) {
              errors.push(`${at} exercise receipt lacks input snapshots`);
            } else {
              for (const item of Object.values(receipt.inputs)) {
                const location = resolveEvidencePath(item.path, options.environment);
                if (!inside(repoRoot, location) || !statSync(location).isFile()
                  || sha256(readFileSync(location)) !== item.sha256) {
                  errors.push(`${at} exercised source changed since receipt creation`);
                  break;
                }
              }
            }
          }
        }
      } catch {
        errors.push(`${at} could not verify exercise receipt or source snapshot`);
      }
    }
    // A self-declared exercise receipt earns zero credit until its actual
    // source, licensing and receipt fingerprints were checked on this host.
    if (eligibleVetted && options.verifyLocal && errors.length === errorsBeforeEntry) {
      counts.vetted_installed_exercised++;
    }
  }
  counts.remaining_to_target = Math.max(
    0,
    (Number.isInteger(manifest.target_vetted_skill_mcp_count)
      ? manifest.target_vetted_skill_mcp_count : 200) - counts.vetted_installed_exercised,
  );
  if (Object.hasOwn(manifest, 'claimed_vetted_count')
    && manifest.claimed_vetted_count !== counts.vetted_installed_exercised) {
    errors.push('claimed_vetted_count does not match actual supported entries');
  }
  if (options.requireTarget
    && (!options.verifyLocal
      || counts.vetted_installed_exercised !== manifest.target_vetted_skill_mcp_count)) {
    errors.push('200 vetted installed and exercised skills/MCPs target is unmet; require --verify-local with receipts');
  }
  return { ok: errors.length === 0, errors, counts };
}

function main() {
  const args = new Set(process.argv.slice(2));
  if (args.has('--help')) {
    process.stdout.write('Usage: node skills-manifest/validate.mjs [--verify-local] [--require-target]\n');
    return;
  }
  const allowed = new Set(['--verify-local', '--require-target']);
  if ([...args].some((arg) => !allowed.has(arg))) {
    process.stderr.write('Unrecognized argument\n');
    process.exitCode = 2;
    return;
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(resolve(manifestDir, 'inventory.json'), 'utf8'));
  } catch {
    process.stderr.write('Could not parse inventory.json\n');
    process.exitCode = 1;
    return;
  }
  const result = validateManifest(manifest, {
    verifyLocal: args.has('--verify-local'),
    requireTarget: args.has('--require-target'),
  });
  process.stdout.write(`${JSON.stringify(result.counts, null, 2)}\n`);
  if (!result.ok) {
    for (const error of result.errors) process.stderr.write(`ERROR: ${error}\n`);
    process.exitCode = 1;
  } else {
    process.stdout.write('Manifest evidence validated; no claim of 200-item completion.\n');
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
