import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { DraftPersistence } from '../apps/desktop/src/lib/draft-persistence.ts';

test('native-backed room drafts preserve exact text, CAS revision and separate rooms', async () => {
  const rows = new Map([['room-a', { room_id: 'room-a', content: 'recovered', revision: 4 }]]);
  const events = [];
  const persistence = new DraftPersistence(async (id, content, expectedRevision) => {
    const before = rows.get(id);
    if ((before?.revision ?? 0) !== expectedRevision) throw new Error('stale revision');
    const result = { room_id: id, content, revision: expectedRevision + 1 };
    rows.set(id, result);
    return result;
  }, (id, state) => events.push([id, state]), 100_000);
  persistence.hydrate([...rows.values()]);
  assert.equal(persistence.text('room-a'), 'recovered');
  persistence.change('room-a', '  unsent\n🧪');
  persistence.change('room-b', 'Other room');
  assert.equal(persistence.hasUnsaved(), true);
  assert.equal(await persistence.flush('room-b'), true);
  assert.equal(await persistence.flush('room-a'), true);
  assert.equal(rows.get('room-a').revision, 5);
  assert.equal(rows.get('room-a').content, '  unsent\n🧪');
  assert.equal(rows.get('room-b').content, 'Other room');
  assert.equal(persistence.hasUnsaved(), false);
  assert.ok(events.some(([id, state]) => id === 'room-a' && state === 'saved'));
  persistence.dispose();
});

test('stale editor fails closed rather than clobbering another window', async () => {
  const saved = new Map([['id', { room_id: 'id', content: 'other window', revision: 2 }]]);
  const failures = [];
  const persistence = new DraftPersistence(async (roomId, content, expectedRevision) => {
    if (saved.get(roomId).revision !== expectedRevision) throw new Error('draft changed');
    return { room_id: roomId, content, revision: expectedRevision + 1 };
  }, (id, state, error) => { if (state === 'error') failures.push(error); }, 100_000);
  persistence.hydrate([{ room_id: 'id', content: 'first window', revision: 1 }]);
  persistence.change('id', 'local change');
  assert.equal(await persistence.flush('id'), false);
  assert.equal(persistence.hasUnsaved(), true);
  assert.equal(persistence.text('id'), 'local change');
  assert.equal(saved.get('id').content, 'other window');
  assert.match(failures[0], /draft changed/);
  persistence.replaceFromDisk('id', saved.get('id'));
  assert.equal(persistence.hasUnsaved(), false);
  persistence.dispose();
});

test('in-flight older save is serialized before latest edit and cannot win', async () => {
  let confirmFirst;
  let calls = 0;
  const persistence = new DraftPersistence(async (id, content, revision) => {
    calls++;
    if (calls === 1) await new Promise((resolve) => { confirmFirst = resolve; });
    return { room_id: id, content, revision: revision + 1 };
  }, () => {}, 100_000);
  persistence.change('room', 'older');
  const save = persistence.flush('room');
  persistence.change('room', 'newer');
  assert.equal(persistence.hasUnsaved(), true);
  confirmFirst();
  assert.equal(await save, true);
  assert.equal(calls, 2);
  assert.equal(persistence.text('room'), 'newer');
  assert.equal(persistence.hasUnsaved(), false);
  persistence.dispose();
});

test('renderer never stores draft text in localStorage or initiates a model request', () => {
  const source = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  assert.match(source, /studio_list_room_drafts/);
  assert.match(source, /studio_save_room_draft/);
  assert.match(source, /draftPersistence\.flush\(targetRoomId\)/);
  assert.match(source, /studio_send_saved_draft/);
  assert.match(source, /draftPersistence\.savedRevision\(targetRoomId, content\)/);
  assert.match(source, /draftPersistence\.confirmSent\(targetRoomId, content, expectedDraftRevision\)/);
  assert.match(source, /oninput=\{\(event\) => changeComposerDraft/);
  assert.doesNotMatch(source, /localStorage\.setItem\([^\n]*draft/i);
});

test('confirmed native atomic send clears saved text without a second write', async () => {
  const saved = [];
  const events = [];
  const drafts = new DraftPersistence(async (id, text, rev) => {
    saved.push([id, text, rev]);
    return { room_id: id, content: text, revision: rev + 1 };
  }, (room, status) => events.push([room, status]), 100_000);
  drafts.change('room', '  hello 🧪  ');
  assert.equal(await drafts.flush('room'), true);
  const revision = drafts.savedRevision('room', 'hello 🧪');
  assert.equal(revision, 1);
  assert.equal(drafts.confirmSent('room', 'hello 🧪', revision), true);
  assert.equal(drafts.text('room'), '');
  assert.equal(drafts.hasUnsaved(), false);
  assert.equal(drafts.savedRevision('room', 'hello 🧪'), null);
  assert.equal(saved.length, 1, 'Clearing after successful atomic send must not write another draft');
  assert.ok(events.some(([room, status]) => room === 'room' && status === 'saved'));
  drafts.dispose();
});

test('atomic send confirmation will not discard a newer local edit', async () => {
  const writes = [];
  const drafts = new DraftPersistence(async (id, content, rev) => {
    writes.push([content, rev]);
    return { room_id: id, content, revision: rev + 1 };
  }, () => {}, 100_000);
  drafts.hydrate([{ room_id: 'room', content: 'sent text', revision: 7 }]);
  const rev = drafts.savedRevision('room', 'sent text');
  drafts.change('room', 'New content typed before native ack');
  assert.equal(drafts.confirmSent('room', 'sent text', rev), false);
  assert.equal(drafts.text('room'), 'New content typed before native ack');
  assert.equal(drafts.hasUnsaved(), true);
  assert.equal(await drafts.flush('room'), true);
  assert.deepEqual(writes, [['New content typed before native ack', 8]]);
  assert.throws(() => drafts.confirmSent('room', 'sent text', rev), /revision changed/);
  drafts.dispose();
});
