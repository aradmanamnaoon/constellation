import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = (file) => readFileSync(new URL(file, import.meta.url), 'utf8');
const page = source('../apps/desktop/src/routes/+page.svelte');
const native = source('../apps/desktop/src-tauri/src/lib.rs');
const core = source('../crates/constellation-core/src/store.rs');

test('provider removal has a separate human confirmation and never requests a provider', () => {
  assert.match(page, /providerRemovalCandidate = \$state\(''\)/);
  assert.match(page, /Confirm removal of /);
  assert.match(page, /Confirm remove unused/);
  assert.match(page, /studio_remove_unused_provider/);
  assert.match(page, /provider\.remembered_key/);
  assert.match(page, /error = 'Removal was denied\./);
});

test('native removal checks workflow history and locks before credential-free store mutation', () => {
  assert.match(native, /fn studio_remove_unused_provider\(/);
  assert.match(native, /\.workflows\s*\.list\(1, 0\)/);
  assert.match(native, /\.remove_unused_provider\(expected, OperationId::new\(\)\)/);
  assert.match(native, /keys\.remove\(&provider_id\)/);
  assert.match(native, /studio_remove_unused_provider,/);
});

test('atomic core removal compares the full profile and protects old task/message/agent references', () => {
  assert.match(core, /pub fn remove_unused_provider\(/);
  assert.match(core, /if current != expected/);
  assert.match(core, /agent\.provider_id == Some\(expected\.id\)/);
  assert.match(core, /for table in \["tasks", "messages"\]/);
  assert.match(core, /EventKind::ProviderRemoved/);
  assert.match(core, /tx\.commit\(\)\?/);
});
