import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pinnedGeneratedPaths = Object.freeze([
  'catalog/roles.json',
  'catalog/roles.foundation.json',
  'catalog/roles-expansion.psv',
  'docs/mission/COMPLETE_EXECUTION_PLAN.md',
  'skills-manifest/inventory.json',
]);

function assertLfContract(attributes, file) {
  const exactLine = file + ' text eol=lf';
  assert.ok(
    attributes.split(/\r?\n/).includes(exactLine),
    'missing exact LF checkout contract for ' + file,
  );
}

test('all byte-verified generated source files have explicit LF checkout rules', () => {
  const attributes = readFileSync(path.join(root, '.gitattributes'), 'utf8');
  for (const file of pinnedGeneratedPaths) {
    assertLfContract(attributes, file);
    const bytes = readFileSync(path.join(root, file));
    assert.ok(!bytes.includes(13), file + ' must not contain CR bytes');
    const result = execFileSync('git', ['check-attr', 'eol', '--', file], {
      cwd: root, encoding: 'utf8', windowsHide: true,
    });
    assert.equal(result.trim(), file + ': eol: lf');
  }
});

test('negative mutation refuses removal of byte-stable generated role checkout rule', () => {
  const attributes = readFileSync(path.join(root, '.gitattributes'), 'utf8');
  // .gitattributes itself may be checked out with CRLF on Windows; the test
  // must be independent of its own line endings while requiring LF for the
  // explicitly pinned generated files.
  const mutated = attributes.replace(/catalog\/roles\.json text eol=lf\r?\n/, '');
  assert.notEqual(mutated, attributes);
  assert.throws(() => assertLfContract(mutated, 'catalog/roles.json'), assert.AssertionError);
});
