import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { composerEnterAction, IME_ENTER_GRACE_MS } from '../apps/desktop/src/lib/composer-keyboard.ts';

const key = (overrides = {}) => ({
  key: 'Enter', shiftKey: false, repeat: false, isComposing: false,
  keyCode: 13, timeStamp: 1000, ...overrides,
});

test('ordinary Enter sends, Shift+Enter stays multiline, auto-repeat never double-sends', () => {
  assert.equal(composerEnterAction(key(), false, null), 'send');
  assert.equal(composerEnterAction(key({ shiftKey: true }), false, null), 'keep-editing');
  assert.equal(composerEnterAction(key({ repeat: true }), false, null), 'keep-editing');
  assert.equal(composerEnterAction(key({ key: 'a' }), false, null), 'keep-editing');
});

test('active IME composition and keyCode 229 cannot send a draft', () => {
  assert.equal(composerEnterAction(key(), true, null), 'keep-editing');
  assert.equal(composerEnterAction(key({ isComposing: true }), false, null), 'keep-editing');
  assert.equal(composerEnterAction(key({ keyCode: 229 }), false, null), 'keep-editing');
});

test('late IME Enter after compositionend is suppressed without sending or newline', () => {
  assert.equal(composerEnterAction(key({ timeStamp: 500 }), false, 500), 'suppress-ime-commit');
  assert.equal(composerEnterAction(key({ timeStamp: 500 + IME_ENTER_GRACE_MS }), false, 500), 'suppress-ime-commit');
  assert.equal(composerEnterAction(key({ timeStamp: 651 }), false, 500), 'send');
  assert.equal(composerEnterAction(key({ timeStamp: 450 }), false, 500), 'send');
});

test('Svelte composer wires composition callbacks to safe action without triggering AI', () => {
  const source = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
  for (const marker of ['oncompositionstart=', 'oncompositionend=', 'onkeydown={handleComposerKeydown}',
    'composerEnterAction(event, composerCompositionActive, composerLastCompositionEnd)',
    "if (action === 'send') void sendMessage();"]) {
    assert.ok(source.includes(marker), `Composer missing ${marker}`);
  }
});
