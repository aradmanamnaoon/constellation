import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { scanDocumentation } from './docs-links.mjs';

test('checks inline, reference and HTML images while ignoring external links and fenced examples', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'cstl-docs-'));
  try {
    mkdirSync(path.join(root, 'docs'));
    writeFileSync(path.join(root, 'docs', 'valid file.png'), 'fixture');
    writeFileSync(path.join(root, 'README.md'), [
      '[file](docs/valid%20file.png#anchor)',
      '![image][screenshot]',
      '[screenshot]: docs/valid%20file.png',
      '<img src="docs/valid%20file.png" alt="ok">',
      '[remote](https://example.com/nope)',
      '[section](#local-section)',
      '[folder](docs/)',
      '```md',
      '[not-a-real-link](missing.md)',
      '```'
    ].join('\n'));
    const result = scanDocumentation(root);
    assert.equal(result.documents, 1);
    assert.equal(result.localLinks, 4);
    assert.deepEqual(result.errors, []);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('fails for missing sources, uncommitted assets and paths outside repository', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'cstl-docs-'));
  try {
    writeFileSync(path.join(root, 'README.md'), '[missing](missing.md)\n[escape](../secret.txt)');
    const result = scanDocumentation(root, { trackedPaths: new Set() });
    assert.equal(result.errors.length, 3);
    assert.ok(result.errors.some((item) => item.includes('not tracked in Git')));
    assert.ok(result.errors.some((item) => item.includes('does not exist')));
    assert.ok(result.errors.some((item) => item.includes('escapes repository root')));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('public documentation must not link to an existing but excluded private input', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'cstl-public-doc-links-'));
  try {
    mkdirSync(path.join(root, 'docs'));
    writeFileSync(path.join(root, 'private-original.md'), '# Not distributable');
    writeFileSync(path.join(root, 'docs', 'published.md'),
      '[private master](../private-original.md)\n[broken public link](missing.md)');
    const trackedPaths = new Set(['docs/published.md']);
    const result = scanDocumentation(root, { trackedPaths });
    assert.equal(result.documents, 1);
    assert.equal(result.errors.length, 2);
    assert.ok(result.errors.some((error) =>
      error.includes('private-original.md') && error.includes('target is not tracked in Git')));
    assert.ok(result.errors.some((error) =>
      error.includes('missing.md') && error.includes('file does not exist')));
    // A non-clickable, truthful provenance note is valid publication text;
    // it must not provide an exception for the separate broken public link.
    writeFileSync(path.join(root, 'docs', 'published.md'),
      'Original master: withheld, not independently publicly verified.\n[broken public link](missing.md)');
    const redacted = scanDocumentation(root, { trackedPaths });
    assert.equal(redacted.errors.length, 1);
    assert.match(redacted.errors[0], /missing\.md/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
