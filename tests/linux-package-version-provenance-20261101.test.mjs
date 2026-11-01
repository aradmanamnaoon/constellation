import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');

test('Windows, frontend and Linux package versions agree with frozen Tauri version', () => {
  const version = JSON.parse(read('apps/desktop/src-tauri/tauri.conf.json')).version;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  assert.equal(JSON.parse(read('apps/desktop/package.json')).version, version);
  assert.match(
    read('apps/desktop/src-tauri/Cargo.toml'),
    new RegExp(`^version = "${version.replaceAll('.', '\\.') }"$`, 'm'),
  );
  for (const script of [
    'tests/linux-package-offline.sh',
    'tests/linux-appimage-offline.sh',
    'tests/linux-gui-xvfb.sh',
    'tests/linux-install-isolated.sh',
  ]) {
    const source = read(script);
    assert.match(source, /source\/apps\/desktop\/src-tauri\/tauri\.conf\.json|snapshot\/apps\/desktop\/src-tauri\/tauri\.conf\.json/);
    assert.match(source, /version="\$\(node -p /);
    assert.match(source, /\[\[ "\$version" =~ \^\[0-9\]\+/);
    assert.doesNotMatch(source, /constellation-studio_0\.1\.0_amd64\.deb|Constellation_0\.1\.0_x86_64\.AppImage|Version: 0\.1\.0/);
  }
  const deb = read('tests/linux-package-offline.sh');
  assert.ok(deb.includes('Version: ${version}'));
  assert.ok(deb.includes('constellation-studio_${version}_amd64.deb'));
  assert.ok(read('tests/linux-appimage-offline.sh').includes('Constellation_${version}_x86_64.AppImage'));
});
