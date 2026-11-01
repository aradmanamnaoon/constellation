import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

// This validates the evidence *shape* of claimed PASS rows. It cannot attest
// that a receipt, tester, platform, binary or screenshot is authentic.
const root = resolve(import.meta.dirname, '..');
const ledger = readFileSync(resolve(root, 'docs/quality/acceptance-ledger.md'), 'utf8');

const fields = [
  'requirement_id', 'requirement', 'platform', 'build_sha', 'environment',
  'test_type', 'automated/manual', 'observed_result', 'screenshot_or_log',
  'verified_by', 'timestamp', 'blocker',
];

function parseRows(content) {
  return content.split(/\r?\n/)
    .filter((line) => /^\|\s*[A-Z][A-Z0-9]*-\d+\s*\|/.test(line))
    .map((line) => {
      const columns = line.split('|').slice(1, -1).map((value) => value.trim());
      assert.equal(columns.length, fields.length, `Malformed evidence row: ${line.slice(0, 60)}`);
      return Object.fromEntries(fields.map((field, i) => [field, columns[i]]));
    });
}

const missing = (value) => !value || /^(?:—|-|none|n\/a|unverified)$/i.test(value);

function incompletePassRows(rows) {
  return rows.flatMap((row) => {
    if (row.observed_result !== '**PASS**') return [];
    const gaps = [];
    for (const field of [
      'platform', 'build_sha', 'environment', 'test_type', 'automated/manual',
      'screenshot_or_log', 'verified_by', 'timestamp',
    ]) {
      if (missing(row[field])) gaps.push(field);
    }
    if (!/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/i.test(row.build_sha)) {
      gaps.push('build_sha_format');
    }
    if (!/^\d{4}-\d{2}-\d{2}(?:T|\s)/.test(row.timestamp)) gaps.push('timestamp_format');
    return gaps.length ? [{ id: row.requirement_id, platform: row.platform, gaps }] : [];
  });
}

test('master ledger preserves exactly 188 platform rows and the 14 x 5 native GUI matrix', () => {
  const rows = parseRows(ledger);
  assert.equal(rows.length, 188);
  const platforms = ['Windows', 'macOS-x64', 'macOS-arm64', 'Linux', 'Android'];
  for (let flow = 1; flow <= 14; flow++) {
    const id = `GUI-${String(flow).padStart(2, '0')}`;
    const relevant = rows.filter((row) => row.requirement_id === id);
    assert.deepEqual(relevant.map((row) => row.platform).sort(), [...platforms].sort(), id);
  }
  assert.equal(rows.filter((row) => row.requirement_id.startsWith('GUI-')).length, 70);
});

test('every current claimed PASS row has non-placeholder, attributable evidence metadata', () => {
  assert.deepEqual(incompletePassRows(parseRows(ledger)), []);
});

test('a forged PASS token without build, platform and observation metadata is rejected', () => {
  const empty = '| SHIP-01 | Windows setup | Windows | — | — | — | — | **PASS** | — | — | — | — |';
  const gaps = incompletePassRows(parseRows(empty));
  assert.equal(gaps.length, 1);
  assert(gaps[0].gaps.includes('build_sha'));
  assert(gaps[0].gaps.includes('screenshot_or_log'));
  assert(gaps[0].gaps.includes('verified_by'));
});

test('binary hashes alone cannot fill all installed-platform evidence fields', () => {
  const hash = 'a'.repeat(64);
  const incomplete = `| GUI-01 | Install/launch | Linux | ${hash} | — | installed GUI | automated | **PASS** | — | — | — | — |`;
  const gaps = incompletePassRows(parseRows(incomplete));
  assert.deepEqual(gaps[0].gaps.filter((field) => !field.endsWith('_format')).sort(),
    ['environment', 'screenshot_or_log', 'timestamp', 'verified_by']);
});
