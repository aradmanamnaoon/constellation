import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { paginateLocal } from '../apps/desktop/src/lib/local-pagination.ts';

test('ten thousand synthetic items retain stable local task identities at bounded card count', () => {
  const tasks = Array.from({ length: 10_000 }, (_, i) => ({ id: `task-${i}`, state: i % 2 ? 'done' : 'open' }));
  const shown = new Set();
  const start = performance.now();
  for (let page = 0; page < 250; page++) {
    const result = paginateLocal(tasks, page, 40);
    assert.equal(result.items.length, 40);
    for (const task of result.items) {
      assert.ok(!shown.has(task.id));
      shown.add(task.id);
    }
  }
  assert.equal(shown.size, tasks.length);
  assert.ok(Number.isFinite(performance.now() - start));
  // Pure helper regression only: it cannot certify native 10k snapshot/UI performance.
});

test('chat scroll preserves reading anchor and offers an explicit latest-message action', () => {
  const source = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  const stylesheet = readFileSync(new URL('../apps/desktop/src/routes/studio.css', import.meta.url), 'utf8');
  assert.match(source, /firstVisibleMessageAnchor\(\)/);
  assert.match(source, /restoreMessageAnchor\(visibleAnchor\)/);
  assert.match(source, /data-message-id=\{message\.id\}/);
  assert.match(source, /Jump to latest/);
  assert.match(source, /unreadMessageCount \+= added/);
  assert.match(source, /onscroll=\{chatScrollChanged\}/);
  assert.match(source, /chatNearBottom && messagePage === 0/);
  assert.match(source, /chatScrollElement\.scrollTop = messages\.length === 0 \? 0 : chatScrollElement\.scrollHeight/);
  assert.match(stylesheet, /\.chat-jump-toolbar\s*\{/);
  assert.match(source, /class:compact=\{messages\.length > 0\}/);
  assert.match(stylesheet, /\.room-intro\.compact\s*\{[^}]*padding:13px 0 15px/);
  assert.match(source, /notice = draftCleared[\s\S]*?await tick\(\);\s*scrollChatToBottom\(\);/);
});
