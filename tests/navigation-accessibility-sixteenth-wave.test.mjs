import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const page = readFileSync(new URL('../apps/desktop/src/routes/+page.svelte', import.meta.url), 'utf8');
const styles = readFileSync(new URL('../apps/desktop/src/routes/studio.css', import.meta.url), 'utf8');
const navigation = readFileSync(new URL('../apps/desktop/src/lib/workspace-navigation.ts', import.meta.url), 'utf8');

test('keyboard-first users can bypass persistent chrome to a focusable main landmark', () => {
  assert.match(page, /<a class="skip-workspace-link" href="#workspace-main">Skip to workspace content<\/a>/);
  assert.match(page, /<main class="main" id="workspace-main" tabindex="-1">/);
  assert.match(styles, /\.skip-workspace-link:focus-visible\s*\{[^}]*transform:none/);
  assert.match(styles, /\.studio \.main:focus-visible\s*\{[^}]*outline:3px solid var\(--color-focus\)/);
});

test('rail sections and current room expose selection semantics independent of color', () => {
  for (const section of ['home', 'rooms', 'tasks', 'evidence', 'library', 'settings']) {
    assert.ok(page.includes(`aria-current={screen === '${section}' ? 'page' : undefined}`),
      `missing current-page state for ${section}`);
  }
  assert.match(page, /aria-current=\{roomId === r\.id \? 'true' : undefined\}/);
});

test('command palette includes the Evidence destination and task copy states existing controls accurately', () => {
  assert.match(page, /<WorkspaceCommandPalette rooms=\{snapshot\.rooms\}/);
  assert.match(navigation, /label: 'Evidence'.*section: 'evidence'/);
  assert.match(page, /Text-only model execution is available with an explicitly selected provider/);
  assert.match(page, /local file actions still require separate native approval/);
  assert.doesNotMatch(page, /Execution and tool permissions will be available in a future build/);
});
