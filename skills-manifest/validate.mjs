#!/usr/bin/env node
// Standard-library-only verification. Read-only even in --verify-local mode.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { resolve, dirname, isAbsolute, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

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
        if (entry.license?.status !== 'verified'
          || !isObject(entry.exercise)
          || !isNonEmpty(entry.exercise?.command)
          || !isNonEmpty(entry.exercise?.result)
          || entry.exercise?.outcome !== 'passed'
          || !isNonEmpty(entry.exercise?.observed_on)
          || !isNonEmpty(entry.exercise?.receipt_path)
          || !/^[a-f0-9]{64}$/.test(entry.exercise?.receipt_sha256 ?? '')) {
          errors.push(`${at}: installed_vetted requires license and real passing exercise evidence plus a hashed receipt`);
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
      && isObject(entry.exercise) && isNonEmpty(entry.exercise.receipt_path)) {
      try {
        const receiptPath = resolveEvidencePath(entry.exercise.receipt_path, options.environment);
        const evidenceRoot = resolve(repoRoot, 'docs', 'dev-tools', 'evidence');
        const child = relative(evidenceRoot, receiptPath);
        if (!child || child === '..' || child.startsWith(`..${'\\'}`)
          || child.startsWith('../') || isAbsolute(child)) {
          errors.push(`${at} exercise receipt must be under docs/dev-tools/evidence`);
        } else if (!existsSync(receiptPath) || !statSync(receiptPath).isFile()) {
          errors.push(`${at} exercise receipt does not exist`);
        } else {
          const hash = createHash('sha256').update(readFileSync(receiptPath)).digest('hex');
          if (hash !== entry.exercise.receipt_sha256) {
            errors.push(`${at} exercise receipt SHA256 mismatch`);
          }
        }
      } catch {
        errors.push(`${at} could not verify exercise receipt`);
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
