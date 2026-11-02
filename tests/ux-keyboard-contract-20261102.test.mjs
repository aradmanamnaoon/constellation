// Static keyboard/a11y contract only. Never report these source assertions as
// proof that hardware keys, a screen reader, or true browser zoom worked.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const testsDir = fileURLToPath(new URL('.', import.meta.url));
const src = (relative) => readFileSync(resolve(testsDir, '../apps/desktop/src', relative), 'utf8');
const page = src('routes/+page.svelte');
const palette = src('lib/WorkspaceCommandPalette.svelte');

test('global Ctrl+K uses preventDefault and preserves an unrelated modal', () => {
  assert.match(page, /\(event\.metaKey \|\| event\.ctrlKey\).*event\.key\.toLowerCase\(\) === 'k'/);
  assert.match(page, /if \(!dialog\) dialog = 'commands';/);
  assert.match(page, /if \(!dialog \|\| dialog === 'commands'\) void focusCurrentDialogField\(\);/);
  assert.match(page, /event\.preventDefault\(\)/);
});

test('palette ArrowDown/ArrowUp wrap, Enter activates, and IME is ignored', () => {
  assert.match(palette, /event\.isComposing \|\| event\.keyCode === 229/);
  assert.match(palette, /event\.key === 'ArrowDown' \|\| event\.key === 'ArrowUp'/);
  assert.match(palette, /\(currentIndex \+ \(event\.key === 'ArrowDown' \? 1 : -1\) \+ commands\.length\) % commands\.length/);
  assert.match(palette, /event\.key === 'Enter'/);
  assert.match(palette, /if \(selected\) activate\(selected\)/);
});

test('Escape is handled globally and modal teardown requests previous focus', () => {
  assert.match(page, /if \(event\.key === 'Escape'\)/);
  assert.match(page, /if \(dialog\) closeDialog\(\)/);
  assert.match(page, /const previousFocus = document\.activeElement/);
  assert.match(page, /previousFocus\?\.focus\(\)/);
  assert.match(page, /node\.addEventListener\('keydown', trapTab\)/);
});

test('palette exposes combobox/listbox/option active descendant semantics', () => {
  for (const marker of [
    'aria-label="Search workspace commands and rooms"',
    'role="combobox"',
    'aria-controls="workspace-command-results"',
    'aria-describedby="workspace-command-hint"',
    'aria-activedescendant=',
    'role="listbox"',
    'aria-label="Workspace destinations"',
    'role="option"',
    'aria-selected=',
    'tabindex="-1"',
  ]) {
    assert.ok(palette.includes(marker), `Missing source accessibility marker: ${marker}`);
  }
});

test('static page stylesheet acknowledges zoomed viewport, not evidence of real zoom', () => {
  const css = src('routes/studio.css');
  assert.match(css, /200% zoomed/);
  assert.match(css, /command-dialog/);
});
