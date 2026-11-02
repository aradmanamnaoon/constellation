import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { SerialAsyncQueue } from '../apps/desktop/src/lib/serial-async-queue.ts';

test('overlapping refresh work applies in invocation order, not response completion order', async () => {
  const queue = new SerialAsyncQueue();
  const events = [];
  let releaseFirst;
  const first = queue.enqueue(async () => {
    events.push('first-start');
    await new Promise((resolve) => { releaseFirst = resolve; });
    events.push('first-apply');
    return 'old';
  });
  const second = queue.enqueue(async () => {
    events.push('second-start');
    events.push('second-apply');
    return 'new';
  });
  await Promise.resolve();
  assert.deepEqual(events, ['first-start'], 'second native fetch must not overtake first');
  releaseFirst();
  assert.deepEqual(await Promise.all([first, second]), ['old', 'new']);
  assert.deepEqual(events, ['first-start', 'first-apply', 'second-start', 'second-apply']);
});

test('a failed refresh does not strand subsequent updates', async () => {
  const queue = new SerialAsyncQueue();
  const first = queue.enqueue(async () => { throw new Error('offline bootstrap'); });
  const second = queue.enqueue(async () => 'recovered');
  await assert.rejects(first, /offline bootstrap/);
  assert.equal(await second, 'recovered');
});

test('queue handles many refresh invocations without concurrent mutation', async () => {
  const queue = new SerialAsyncQueue();
  let active = 0;
  let highWater = 0;
  const applied = [];
  await Promise.all(Array.from({ length: 300 }, (_, index) => queue.enqueue(async () => {
    active++;
    highWater = Math.max(highWater, active);
    await Promise.resolve();
    applied.push(index);
    active--;
  })));
  assert.equal(highWater, 1);
  assert.deepEqual(applied, Array.from({ length: 300 }, (_, index) => index));
});

test('renderer serializes entire native snapshot and scroll reconciliation', () => {
  const source = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  const start = source.indexOf('async function refresh()');
  const end = source.indexOf('function chatScrollChanged', start);
  const refresh = source.slice(start, end);
  assert.match(refresh, /return refreshQueue\.enqueue\(async \(\) => \{/);
  assert.match(refresh, /invoke<Snapshot>\('studio_bootstrap'\)/);
  assert.match(refresh, /snapshot = \{ \.\.\.next, tasks: next\.tasks \?\? \[\] \};/);
  assert.match(refresh, /await tick\(\)/);
  assert.match(refresh, /restoreMessageAnchor/);
  assert.ok(refresh.indexOf('await tick()') < refresh.lastIndexOf('});'),
    'DOM reconciliation must remain inside the serialized task');
});
