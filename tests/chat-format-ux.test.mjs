import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { splitChatBlocks } from '../apps/desktop/src/lib/chat-format.ts';

const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');

test('plain text, HTML-looking content and Unicode are never interpreted as markup', () => {
  const source = '<script>alert(1)</script>\nمتن فارسی 🧠';
  assert.deepEqual(splitChatBlocks(source), [{ kind: 'text', text: source }]);
  assert.match(page, /<code>\{block\.text\}<\/code>/);
  assert.doesNotMatch(page, /\{@html\s+block\.text/);
});

test('fenced code remains a distinct block and preserves source language', () => {
  assert.deepEqual(splitChatBlocks('Before\n```python\nprint("hi")\n```\nAfter'), [
    { kind: 'text', text: 'Before' },
    { kind: 'code', text: 'print("hi")', language: 'python' },
    { kind: 'text', text: 'After' },
  ]);
  assert.deepEqual(splitChatBlocks('```\n<unsafe>\n```'), [
    { kind: 'code', text: '<unsafe>', language: '' },
  ]);
});

test('unclosed or oversized fences never silently drop content', () => {
  assert.deepEqual(splitChatBlocks('Start\n```js\nconst a = 1'), [
    { kind: 'text', text: 'Start' },
    { kind: 'text', text: '```js\nconst a = 1' },
  ]);
  const oversized = 'x'.repeat(1_000_001) + '\n```';
  assert.deepEqual(splitChatBlocks(oversized), [{ kind: 'text', text: oversized }]);
});

test('chat UI copy buttons require explicit user input and do not invoke providers', () => {
  assert.match(page, /onclick=\{\(\) => void copyMessageText\(message\.body\)\}/);
  assert.match(page, /onclick=\{\(\) => void copyMessageText\(block\.text\)\}/);
  assert.match(page, /await navigator\.clipboard\.writeText\(content\)/);
});
