#!/usr/bin/env node
/** Deterministically build prompt-only roles from preserved curated sources. */
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const dir = dirname(fileURLToPath(import.meta.url));
const base = JSON.parse(readFileSync(join(dir, 'roles.foundation.json'), 'utf8'));
const lines = readFileSync(join(dir, 'roles-expansion.psv'), 'utf8')
  .split(/\r?\n/)
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith('#'));

const expanded = lines.map((line, index) => {
  const parts = line.split('|').map((field) => field.trim());
  if (parts.length !== 5
    || parts.some((field, fieldIndex) => field.length < (fieldIndex === 1 ? 3 : 10))) {
    throw new Error('Invalid specialization at line ' + (index + 1));
  }
  const [name, category, summary, method, deliverable] = parts;
  const id = name.toLowerCase().replace(/[^a-z0-9 ]/g, '').replace(/ +/g, '-');
  return {
    id, name, category, summary,
    instructions: [
      method,
      'Begin by clarifying the scope, inputs, decision criteria and relevant failure conditions.',
      'Make assumptions and uncertainty explicit; verify factual claims using authorized sources only.',
      'Never imply access to files, tools, credentials, network data or runtime capabilities that are unavailable.',
      'If actual execution cannot be verified, describe a reproducible method and label outcomes untested.',
    ].join(' '),
    deliverable,
  };
});
const roles = [...base.roles, ...expanded];
const identities = new Set(roles.map((role) => role.id));
const names = new Set(roles.map((role) => role.name.toLowerCase()));
if (base.roles.length !== 42 || expanded.length !== 158 || roles.length !== 200
  || identities.size !== roles.length || names.size !== roles.length) {
  throw new Error('Role count or identity collision; review source entries');
}
const output = { ...base, roles };
const encoded = JSON.stringify(output, null, 2) + '\n';
const destination = join(dir, 'roles.json');
if (process.argv.includes('--check')) {
  if (readFileSync(destination, 'utf8') !== encoded) {
    process.stderr.write('roles.json differs; run node catalog/build-roles.mjs\n');
    process.exitCode = 1;
  } else {
    process.stdout.write('Verified 200 prompt-only roles (42 preserved, 158 curated).\n');
  }
} else if (process.argv.length === 2) {
  writeFileSync(destination, encoded);
  process.stdout.write('Built 200 prompt-only roles (42 preserved, 158 curated).\n');
} else {
  process.stderr.write('Usage: node catalog/build-roles.mjs [--check]\n');
  process.exitCode = 2;
}
