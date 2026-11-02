import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { prepareProviderRevision } from '../apps/desktop/src/lib/provider-revision.ts';

test('revising never copies a saved key, vault reference or original identity into the save input', () => {
  const old = {
    id: 'old-connection', name: 'Lab', kind: 'custom_compatible', endpoint: 'https://example.invalid/v1',
    model: 'model-1', remembered_key: true, api_key: 'must-not-copy', credential_reference: 'must-not-copy',
  };
  const draft = prepareProviderRevision(old);
  assert.deepEqual(draft, {
    sourceId: old.id, name: old.name, kind: old.kind, endpoint: old.endpoint,
    model: old.model, key: '', rememberKey: false, sourceRememberedKey: true,
  });
  assert.ok(!JSON.stringify(draft).includes('must-not-copy'));
});

test('legacy kind gets deterministic compatible defaults without retrieving secrets', () => {
  const draft = prepareProviderRevision({ id: 'legacy', name: 'Legacy', endpoint: 'http://localhost:1234/v1', model: 'm' });
  assert.equal(draft.kind, 'openai_chat');
  assert.equal(draft.sourceRememberedKey, false);
  assert.equal(draft.key, '');
});

test('revision dialog is explicit, no hidden provider probes, no credential reuse', () => {
  const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  assert.match(page, /onclick=\{\(\) => openProviderRevision\(provider\)\}/);
  assert.match(page, /providerRevisionSavedKey && !providerKey\.trim\(\)/);
  assert.match(page, /Keys are never shown or copied/);
  assert.match(page, /changing its name, model, endpoint or API format creates a separate profile/);
});
