import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const get = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const page = get('../apps/desktop/src/routes/+page.svelte');
const rust = get('../apps/desktop/src-tauri/src/lib.rs');
const store = get('../crates/constellation-core/src/store.rs');

test('OS-key deletion is separately confirmed and never triggers a provider request', () => {
  assert.match(page, /providerForgetCandidate = \$state\(''\)/);
  assert.match(page, /providerForgetConfirmation !== provider\.name/);
  assert.match(page, /Forget saved key/);
  assert.match(page, /Delete saved API key/);
  assert.match(page, /studio_forget_remembered_provider_key/);
  assert.match(page, /Already-running requests may still complete/);
});

test('native host verifies the expected saved profile and uses OS vault only when user requests', () => {
  assert.match(rust, /fn studio_forget_remembered_provider_key\(/);
  assert.match(rust, /expected_name != confirmed_name/);
  assert.match(rust, /if current != expected/);
  assert.match(rust, /match Vault\.delete\(vault_key\)/);
  assert.match(rust, /VaultError::NotFound/);
  assert.match(rust, /forget_remembered_provider_credential\(expected, OperationId::new\(\)\)/);
  assert.match(rust, /studio_forget_remembered_provider_key,/);
});

test('core uses atomic CAS and retains provider ID and event ledger', () => {
  assert.match(store, /pub fn forget_remembered_provider_credential\(/);
  assert.match(store, /if current != expected/);
  assert.match(store, /EventKind::ProviderSaved\s*\{\s*provider_id: retired\.id,?\s*\}/);
  assert.match(store, /forgotten_credential_preserves_provider_identity_and_rejects_stale_profile/);
});
