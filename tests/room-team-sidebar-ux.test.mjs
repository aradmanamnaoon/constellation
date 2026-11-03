import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
const css = readFileSync(new URL('../apps/desktop/src/routes/studio.css', import.meta.url), 'utf8');
const rust = readFileSync(new URL('../apps/desktop/src-tauri/src/lib.rs', import.meta.url), 'utf8');
const store = readFileSync(new URL('../crates/constellation-core/src/store.rs', import.meta.url), 'utf8');

test('room sidebar offers discoverable keyboard-accessible team editor and membership state', () => {
  assert.match(page, /async function openRoomTeamEditor\(\)/);
  assert.match(page, /picker\.open = true/);
  assert.match(page, /picker\.querySelector<HTMLElement>\('summary'\)\?\.focus\(\)/);
  assert.match(page, /class:in-room=\{!!room && selectedAgentIds\.includes\(agent\.id\)\}/);
  assert.match(page, /Edit room team · \{selectedAgentIds\.length\} selected/);
  assert.match(page, /onclick=\{\(\) => void openRoomTeamEditor\(\)\}/);
  assert.match(css, /\.agent-list button\.agent-item:focus-visible/);
});

test('one native transaction ties local message, draft clear, and the same replay ID', () => {
  assert.match(page, /studio_send_saved_draft/);
  assert.match(page, /expectedDraftRevision/);
  assert.match(rust, /fn studio_send_saved_draft\(/);
  assert.match(rust, /append_user_message_and_clear_draft\(/);
  assert.match(store, /pub fn append_user_message_and_clear_draft\(/);
  assert.match(store, /SELECT content, revision FROM room_drafts/);
  assert.match(store, /UPDATE room_drafts SET content='', revision=\?1/);
  assert.match(store, /local_send_clears_draft_in_same_transaction_and_replay_cannot_duplicate/);
});
