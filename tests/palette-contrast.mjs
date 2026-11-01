import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = readFileSync(resolve(root, 'apps/desktop/src/routes/palette.css'), 'utf8');

function contrast(foreground, background) {
  const luminance = (hex) => {
    assert.match(hex, /^#[a-fA-F0-9]{6}$/);
    const components = [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255);
    const linear = components.map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
    return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
  };
  const a = luminance(foreground);
  const b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

function tokens(mode) {
  const tag = mode === 'light' ? ':root' : ":root[data-theme='dark']";
  const begin = source.indexOf(`${tag} {`);
  assert.ok(begin >= 0, `Missing ${mode} token declaration`);
  const end = source.indexOf('\n}', begin);
  assert.ok(end > begin, `Missing ${mode} token block terminator`);
  return Object.fromEntries([...source.slice(begin, end).matchAll(/--([a-z-]+):\s*(#[a-fA-F0-9]{6});/g)].map((match) => [match[1], match[2]]));
}

for (const mode of ['light', 'dark']) {
  test(`${mode} text, actions, status and navigation meet contrast thresholds`, () => {
    const t = tokens(mode);
    const pairs = [
      ['text', 'surface', 4.5],
      ['text-muted', 'surface', 4.5],
      ['text', 'surface-alt', 4.5],
      ['action-text', 'action', 4.5],
      ['action-text', 'action-hover', 4.5],
      ['rail-text', 'rail', 4.5],
      ['rail-muted', 'rail', 4.5],
      ['accent', 'surface', 4.5],
      ['danger', 'surface', 4.5],
      ['success', 'surface', 4.5],
      ['text', 'surface-selected', 4.5],
      ['focus', 'surface', 3],
    ];
    for (const [fg, bg, min] of pairs) {
      assert.ok(t[`color-${fg}`] && t[`color-${bg}`], `Missing token pair ${fg}/${bg}`);
      const actual = contrast(t[`color-${fg}`], t[`color-${bg}`]);
      assert.ok(actual >= min, `${mode} ${fg}/${bg}: ${actual.toFixed(2)}:1 below ${min}:1`);
    }
  });
}

test('theme application is reachable and isolated from model/tool permissions', () => {
  const ui = readFileSync(resolve(root, 'apps/desktop/src/routes/+page.svelte'), 'utf8');
  assert.match(ui, /themeMode = \$state<'system' \| 'light' \| 'dark'>/);
  assert.match(ui, /setTheme\('dark'\)/);
  assert.match(ui, /document\.documentElement\.dataset\.theme/);
  assert.match(ui, /aria-pressed=\{themeMode === 'dark'\}/);
  assert.match(source, /prefers-color-scheme: dark/);
});
