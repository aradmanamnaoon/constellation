import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { filterProviderDirectory } from '../apps/desktop/src/lib/provider-directory.ts';

const profiles = [
  { id: 'a', name: 'Local Ollama', endpoint: 'http://127.0.0.1:11434', model: 'llama3', kind: 'ollama_native' },
  { id: 'b', name: 'Remote Lab', endpoint: 'https://research.invalid/v1', model: 'research-1', kind: 'custom_compatible' },
  { id: 'c', name: 'Extra Lab', endpoint: 'https://private.invalid/v1', model: 'research-2', kind: 'custom_compatible' },
];
const probes = { b: { status: 'compatible_text' }, c: { status: 'unauthorized' } };

test('local provider directory search is token-based, stable and case-insensitive', () => {
  assert.deepEqual(filterProviderDirectory(profiles, probes, 'lab research', 'all').map((p) => p.id), ['b', 'c']);
  assert.deepEqual(filterProviderDirectory(profiles, probes, 'LLAMA local', 'all').map((p) => p.id), ['a']);
  assert.deepEqual(filterProviderDirectory(profiles, probes, 'not found', 'all'), []);
  assert.deepEqual(filterProviderDirectory(profiles, probes, '', 'all'), profiles);
});

test('probe categories only indicate this session text status, never all-model capability', () => {
  assert.deepEqual(filterProviderDirectory(profiles, probes, '', 'untested').map((p) => p.id), ['a']);
  assert.deepEqual(filterProviderDirectory(profiles, probes, '', 'text-compatible').map((p) => p.id), ['b']);
  assert.deepEqual(filterProviderDirectory(profiles, probes, '', 'attention').map((p) => p.id), ['c']);
  assert.deepEqual(filterProviderDirectory(profiles, {}, '', 'attention'), []);
});

test('provider filters stay local and never initiate probes implicitly', () => {
  const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  const module = readFileSync(new URL('../apps/desktop/src/lib/provider-directory.ts', import.meta.url), 'utf8');
  assert.match(page, /filteredProviders = \$derived\(filterProviderDirectory\(/);
  assert.match(page, /\{#each filteredProviders as provider \(provider\.id\)\}/);
  assert.match(page, /id="provider-directory-search"/);
  assert.match(page, /Text test passed in this session only/);
  assert.doesNotMatch(module, /\binvoke\s*\(|\bfetch\s*\(/);
});
