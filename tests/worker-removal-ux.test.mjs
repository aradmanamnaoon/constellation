import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
const native = readFileSync(new URL('../apps/desktop/src-tauri/src/lib.rs', import.meta.url), 'utf8');
const core = readFileSync(new URL('../crates/constellation-core/src/store.rs', import.meta.url), 'utf8');

test('worker removal has explicit contextual buttons and confirmation', () => {
  assert.ok(page.includes('Remove unused worker'));
  assert.ok(page.includes('Confirm remove unused'));
  assert.ok(page.includes('Remove ${agent.name} from this room'));
  assert.ok(page.includes('aria-expanded={removeWorkerCandidate === agent.id}'));
});

test('unused worker deletion verifies immutable identity and history server-side', () => {
  assert.ok(page.includes("invoke('studio_remove_unused_agent'"));
  assert.ok(page.includes('expectedCreatedAtMs: worker.created_at_ms'));
  assert.ok(native.includes('fn studio_remove_unused_agent('));
  assert.ok(native.includes('remove_unused_agent(expected, OperationId::new())'));
  assert.ok(core.includes('EventKind::AgentRemoved'));
  assert.ok(core.includes('remove this worker from all saved room teams first'));
  assert.ok(core.includes('this worker authored messages'));
  assert.ok(core.includes('this worker owns saved tasks'));
});

test('room removal preserves unsaved edits or uses native CAS', () => {
  const start = page.indexOf('async function removeWorkerFromRoom(');
  const end = page.indexOf('async function removeUnusedWorker(', start);
  const action = page.slice(start, end);
  assert.ok(action.includes('if (unsavedRoomTeam)'));
  assert.ok(action.includes('toggleAgent(worker.id, false)'));
  assert.ok(action.includes("invoke('studio_set_room_members'"));
  assert.ok(action.includes('expectedMemberIds: room.members_configured ? room.member_ids : null'));
  assert.ok(!action.includes('studio_remove_unused_agent'));
});

test('selection, search and empty states use native buttons', () => {
  assert.ok(page.includes('<div class="room-search"><Search'));
  assert.ok(page.includes('<button type="button" class="room-search-clear"'));
  assert.ok(page.includes('<button type="button" class="sidebar-empty-action"'));
  assert.ok(page.includes('class:chosen={selectedAgentIds.includes(agent.id)} aria-pressed='));
  assert.ok(!page.includes('<div class="agent-options"><label'));
  assert.ok(page.includes('aria-label={`Copy URL for ${provider.name}`}'));
  assert.ok(page.includes('aria-label={`Copy model ID for ${provider.name}`}'));
  assert.ok(page.includes("navigator.clipboard.writeText(value)"));
});
