import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { paginateLocal } from '../apps/desktop/src/lib/local-pagination.ts';

test('task pagination preserves deterministic order and exact boundaries', () => {
  const tasks = Array.from({ length: 92 }, (_, index) => ({ id: index }));
  const first = paginateLocal(tasks, 0);
  assert.equal(first.items.length, 40);
  assert.equal(first.items[0].id, 0);
  assert.equal(first.items.at(-1).id, 39);
  assert.equal(first.lastPage, 2);
  const middle = paginateLocal(tasks, 1);
  assert.deepEqual(middle.items.map((task) => task.id), Array.from({ length: 40 }, (_, i) => i + 40));
  const last = paginateLocal(tasks, 2);
  assert.deepEqual(last.items.map((task) => task.id), Array.from({ length: 12 }, (_, i) => i + 80));
  assert.equal(paginateLocal(tasks, 500).page, 2);
  assert.equal(paginateLocal(tasks, -4).page, 0);
  assert.equal(paginateLocal([], 20).items.length, 0);
  assert.equal(paginateLocal(tasks, NaN).page, 0);
  assert.throws(() => paginateLocal(tasks, 0, 0), RangeError);
  assert.throws(() => paginateLocal(tasks, 0, 101), RangeError);
});

test('task list filters and page navigation do not alter task execution authority', () => {
  const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  assert.match(page, /paginateLocal\(visibleRoomTasks, taskPage, taskPageSize\)/);
  assert.match(page, /{#each pagedRoomTasks as task \(task.id\)}/);
  assert.match(page, /aria-label="Task result pages"/);
  assert.match(page, /taskPage = 0/);
  assert.match(page, /Next tasks/);
  assert.match(page, /Previous tasks/);
  assert.match(page, /disabled=\{!isNative \|\| working \|\| !!runningTaskId/);
});
