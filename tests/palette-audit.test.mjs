import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

// Dependency-free static contract tests; no browser, network or disk writes.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const palette = readFileSync(resolve(root, 'apps/desktop/src/routes/palette.css'), 'utf8');
const studio = readFileSync(resolve(root, 'apps/desktop/src/routes/studio.css'), 'utf8');

function blockAfter(source, marker) {
  const start = source.indexOf(marker);
  assert.ok(start >= 0, `Missing CSS block: ${marker}`);
  const opening = marker.endsWith('{')
    ? start + marker.length - 1
    : source.indexOf('{', start + marker.length);
  assert.ok(opening >= 0, `Missing opening brace: ${marker}`);
  let depth = 1;
  for (let i = opening + 1; i < source.length; i++) {
    if (source[i] === '{') depth++;
    if (source[i] === '}' && --depth === 0) return source.slice(opening + 1, i);
  }
  assert.fail(`Unclosed CSS block: ${marker}`);
}

function parseTokens(source) {
  return Object.fromEntries([...source.matchAll(/(--color-[a-z-]+)\s*:\s*([^;{}]+);/g)]
    .map(([, name, value]) => [name, value.trim()]));
}

const light = parseTokens(blockAfter(palette, ':root {'));
const dark = parseTokens(blockAfter(palette, ":root[data-theme='dark'] {"));
const systemQuery = blockAfter(palette, '@media (prefers-color-scheme: dark)');
const system = parseTokens(blockAfter(systemQuery, ":root:not([data-theme='light']):not([data-theme='dark']) {"));

function relativeLuminance(hex) {
  assert.match(hex, /^#[0-9a-f]{6}$/i, `Contrast input should be a 6-digit hex color: ${hex}`);
  const channels = [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255);
  const [r, g, b] = channels.map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return r * 0.2126 + g * 0.7152 + b * 0.0722;
}

function contrast(a, b) {
  const [high, low] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (high + 0.05) / (low + 0.05);
}

function verifyTokenCoverage(css, modes = [light, dark, system]) {
  const references = [...css.matchAll(/var\(\s*(--color-[a-z-]+)/g)].map((match) => match[1]);
  for (const [index, tokens] of modes.entries()) {
    for (const name of references) {
      assert.ok(Object.hasOwn(tokens, name), `Unresolved ${name} in theme ${index}`);
    }
  }
  return references;
}

function rejectUnscopedColorLiterals(css) {
  assert.doesNotMatch(css, /#[\da-f]{3,8}\b/i, 'Raw colors belong in palette.css, not component CSS');
  assert.doesNotMatch(css, /(?:^|[;{])\s*(?:color|background(?:-color)?|border(?:-color)?)\s*:\s*(?:white|black|red|green|blue)\s*[;}]/im);
  return true;
}

function verifyStatusRoles(css) {
  assert.match(css, /\.toast,\.error-toast\s*\{[^}]*color:var\(--color-success\);\s*background:var\(--color-success-soft\)/);
  assert.match(css, /\.error-toast\s*\{[^}]*color:var\(--color-danger\);\s*background:var\(--color-danger-soft\)/);
  assert.match(css, /\.task-state\[data-state="running"\][^{]*\{[^}]*var\(--color-warning-soft\)[^}]*var\(--color-warning\)/);
  assert.match(css, /\.task-state\[data-state="cancelled"\][^{]*\{[^}]*var\(--color-danger-soft\)[^}]*var\(--color-danger\)/);
  assert.match(css, /\.workflow-saved-item p\s*\{[^}]*color:var\(--color-text-muted\)/);
  assert.match(css, /\.send-button\s*\{[^}]*background:var\(--color-action\)[^}]*color:var\(--color-action-text\)/);
}

function balancedCss(source) {
  // Comments can contain documentation braces but should not alter structure.
  const text = source.replace(/\/\*[\s\S]*?\*\//g, '');
  let nesting = 0;
  for (const ch of text) {
    if (ch === '{') nesting++;
    if (ch === '}') {
      nesting--;
      assert.ok(nesting >= 0, 'CSS has an unmatched closing brace');
    }
  }
  assert.equal(nesting, 0, 'CSS has an unclosed rule');
}

test('light, explicit dark, and system dark provide identical complete token contracts', () => {
  assert.deepEqual(Object.keys(dark).sort(), Object.keys(light).sort());
  assert.deepEqual(system, dark, 'OS-dark preference must match explicit dark tokens exactly');
  assert.match(palette, /:root\s*\{\s*color-scheme:\s*light/);
  assert.match(palette, /:root\[data-theme='dark'\]\s*\{\s*color-scheme:\s*dark/);
  assert.match(systemQuery, /color-scheme:\s*dark/);
  assert.ok(Object.keys(light).length >= 30, 'Missing component-level semantic tokens');
});

test('every component token exists in all three themes and no legacy literal leaks', () => {
  assert.ok(verifyTokenCoverage(studio).length >= 150, 'Component styles lack semantic coverage');
  rejectUnscopedColorLiterals(studio);
  balancedCss(palette);
  balancedCss(studio);
});

for (const [mode, tokens] of [['light', light], ['dark', dark]]) {
  test(`${mode} core text/action/status and focus meet WCAG AA contrasts`, () => {
    const pairs = [
      ['text', 'surface', 4.5],
      ['text-muted', 'surface', 4.5],
      ['text', 'surface-alt', 4.5],
      ['text', 'surface-selected', 4.5],
      ['text', 'surface-raised', 4.5],
      ['text-muted', 'surface-tint', 4.5],
      ['action-text', 'action', 4.5],
      ['action-text', 'action-hover', 4.5],
      ['rail-text', 'rail', 4.5],
      ['rail-muted', 'rail', 4.5],
      ['warning', 'warning-soft', 4.5],
      ['danger', 'danger-soft', 4.5],
      ['success', 'success-soft', 4.5],
      ['avatar-text', 'avatar', 4.5],
      ['focus', 'surface', 3],
      ['icon', 'surface', 3],
    ];
    for (const [foreground, background, required] of pairs) {
      const name = (role) => tokens[`--color-${role}`];
      assert.ok(name(foreground) && name(background), `Missing ${foreground}/${background}`);
      const actual = contrast(name(foreground), name(background));
      assert.ok(actual >= required,
        `${mode} ${foreground}/${background}: ${actual.toFixed(2)} < ${required}:1`);
    }
  });
}

test('keyboard outline and focused composer remain available through form-specific selectors', () => {
  assert.match(studio, /\.studio\s+:is\([^)]*button[^)]*input[^)]*textarea[^)]*select[^)]*summary[^)]*\):focus-visible\s*\{\s*outline:3px solid var\(--color-focus\)/);
  assert.match(studio, /\.studio \.composer:focus-within\s*\{[^}]*border-color:var\(--color-focus\)/);
  assert.match(studio, /\.studio \.composer textarea:focus-visible\s*\{[^}]*outline:2px solid var\(--color-focus\)/);
  assert.match(studio, /\.sidebar:not\(\.open\)\s*\{\s*visibility:hidden/);
});

test('status severity and selected states keep their specific semantic roles', () => {
  verifyStatusRoles(studio);
});

test('forced-colors, reduced motion, RTL and responsive treatment preserve usable controls', () => {
  assert.match(studio, /@media\s*\(forced-colors:active\)/);
  assert.match(studio, /--color-focus:Highlight/);
  assert.match(studio, /outline:3px solid Highlight/);
  assert.match(studio, /@media\s*\(prefers-reduced-motion:reduce\)/);
  assert.match(studio, /\.studio \.sidebar,\.studio \.rail button\s*\{\s*transition:none/);
  assert.match(studio, /:root\[dir='rtl'\] \.studio \.sidebar/);
  assert.match(studio, /inset-inline-start:57px/);
  assert.match(studio, /inset-inline-start:48px/);
  assert.match(studio, /@media\s*\(max-width:520px\)/);
  assert.match(studio, /\.studio \.rail button\s*\{\s*width:40px; height:40px/);
  assert.match(studio, /\.studio \.theme-picker\s*\{\s*flex-wrap:wrap/);
});

test('adversarial regression: missing token, rogue hex, and unmatched CSS are detected', () => {
  assert.throws(() => verifyTokenCoverage(studio + '.unsafe{color:var(--color-not-in-palette)}'), /Unresolved/);
  assert.throws(() => rejectUnscopedColorLiterals(studio + '.unsafe{background:#ffffff;}'), /Raw colors/);
  assert.throws(() => balancedCss(studio + '.broken { color:var(--color-text);'), /unclosed/);
  assert.throws(() => balancedCss(studio + '}'), /unmatched/);
  assert.throws(() => verifyStatusRoles(studio.replace(
    'color:var(--color-success); background:var(--color-success-soft);',
    'color:var(--color-danger); background:var(--color-danger-soft);',
  )), (error) => error.code === 'ERR_ASSERTION');
  assert.ok(contrast('#ffffff', '#ffffff') < 4.5,
    'A simulated white-on-white control must fail contrast');
});
