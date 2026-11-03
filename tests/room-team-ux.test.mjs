import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
const native = readFileSync(new URL('../apps/desktop/src-tauri/src/lib.rs', import.meta.url), 'utf8');
const store = readFileSync(new URL('../crates/constellation-core/src/store.rs', import.meta.url), 'utf8');

test('room team UI persists an explicit selection, but never treats it as authorization', () => {
  assert.match(page, /invoke\('studio_set_room_members',/);
  assert.match(page, /expectedMemberIds: room\.members_configured \? room\.member_ids : null/);
  assert.match(page, /Save room team/);
  assert.match(page, /Discard changes/);
  assert.match(page, /not access control or a tool permission/);
});

test('room persistence has optimistic conflict checking and an audit event', () => {
  assert.match(store, /pub fn set_room_members\(/);
  assert.match(store, /room\.member_ids != expected_member_ids/);
  assert.match(store, /EventKind::RoomMembersChanged/);
  assert.match(store, /ensure_id\(&tx, "agents"/);
  assert.match(native, /studio_set_room_members,/);
  assert.match(native, /"members_configured":room\.member_ids\.is_some\(\)/);
});
