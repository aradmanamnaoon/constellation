import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const android = path.join(root, 'apps/desktop/src-tauri/gen/android');
const read = (relative) => readFileSync(path.join(android, relative), 'utf8');

test('versioned native Android project has a consistent Tauri identifier and entry point', () => {
  const config = JSON.parse(readFileSync(path.join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'));
  const packageName = config.identifier;
  const gradle = read('app/build.gradle.kts');
  const manifest = read('app/src/main/AndroidManifest.xml');
  const activityPath = 'app/src/main/java/' + packageName.replaceAll('.', '/') + '/MainActivity.kt';
  assert.equal(packageName, 'app.constellation.studio');
  assert.ok(existsSync(path.join(android, activityPath)));
  assert.match(gradle, /applicationId\s*=\s*"app\.constellation\.studio"/);
  assert.match(manifest, /android:exported="true"/);
  assert.match(manifest, /android:name="\.MainActivity"/);
  assert.match(manifest, /android:usesCleartextTraffic="\$\{usesCleartextTraffic\}"/);
});

test('mobile project keeps network permissions narrow and contains no signing credentials', () => {
  const manifest = read('app/src/main/AndroidManifest.xml');
  const gradle = read('app/build.gradle.kts');
  const ignore = read('.gitignore');
  const permissions = [...manifest.matchAll(/<uses-permission\s+android:name="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(permissions, ['android.permission.INTERNET']);
  assert.doesNotMatch(manifest, /android\.permission\.(?:READ_SMS|READ_CONTACTS|RECORD_AUDIO|ACCESS_FINE_LOCATION)/);
  assert.match(gradle, /manifestPlaceholders\["usesCleartextTraffic"\]\s*=\s*"false"/);
  assert.match(ignore, /keystore\.properties/);
  assert.match(ignore, /key\.properties/);
  assert.ok(!existsSync(path.join(android, 'key.properties')));
  assert.ok(!existsSync(path.join(android, 'keystore.properties')));
  assert.ok(!existsSync(path.join(android, 'app/release.keystore')));
  const paths = read('app/src/main/res/xml/file_paths.xml');
  assert.doesNotMatch(paths, /<external-path\b/);
  assert.match(paths, /<external-files-path\b/);
});

test('the generated Gradle wrapper is checksum pinned to the reviewed scaffold', () => {
  const wrapper = readFileSync(path.join(android, 'gradle/wrapper/gradle-wrapper.jar'));
  const digest = createHash('sha256').update(wrapper).digest('hex');
  assert.equal(digest, '497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7');
  assert.match(read('gradle/wrapper/gradle-wrapper.properties'), /distributionUrl=https\\:\/\/services\.gradle\.org\/distributions\/gradle-9\.6\.1-bin\.zip/);
  assert.match(read('gradle/wrapper/gradle-wrapper.properties'), /distributionSha256Sum=9c0f7faeeb306cb14e4279a3e084ca6b596894089a0638e68a07c945a32c9e14/);
});

test('mutated Android identifier or unexpectedly broadened permissions are rejected', () => {
  const manifest = read('app/src/main/AndroidManifest.xml');
  const injected = manifest.replace('android.permission.INTERNET', 'android.permission.READ_CONTACTS');
  const permissions = [...injected.matchAll(/<uses-permission\s+android:name="([^"]+)"/g)].map((m) => m[1]);
  assert.notDeepEqual(permissions, ['android.permission.INTERNET']);
  const gradle = read('app/build.gradle.kts').replace(
    'applicationId = "app.constellation.studio"',
    'applicationId = "app.untrusted.app"',
  );
  assert.doesNotMatch(gradle, /applicationId\s*=\s*"app\.constellation\.studio"/);
});

test('Android debug APK is explicitly nonrelease and requires its debug signer to be verified', () => {
  const workflow = readFileSync(path.join(root, '.github/workflows/acceptance.yml'), 'utf8');
  assert.match(workflow, /android-arm64-debug-apk-nonrelease-uninstalled-unverified/);
  assert.match(workflow, /apksigner" verify --verbose --print-certs/);
  assert.match(workflow, /CN=Android Debug/);
  assert.match(workflow, /Refuse production Android release without installed-device acceptance and release signing/);
  assert.match(workflow, /No signing credentials were provided; refusing to mark the Android family passed/);
});

test('Android hosted build pins exact SDK, build tools and NDK rather than selecting an arbitrary latest toolchain', () => {
  const workflow = readFileSync(path.join(root, '.github/workflows/acceptance.yml'), 'utf8');
  const androidJob = workflow.slice(workflow.indexOf('\n  android-build:'), workflow.indexOf('\n  docs-links:'));
  assert.match(androidJob, /NDK_VERSION=28\.2\.13676358/);
  assert.match(androidJob, /"ndk;\$NDK_VERSION"/);
  assert.match(androidJob, /'platforms;android-37' 'build-tools;36\.0\.0'/);
  assert.match(androidJob, /javac -version/);
  assert.doesNotMatch(androidJob, /sort -V|tail -n 1/);
  assert.match(androidJob, /test -d "\$NDK_HOME\/toolchains\/llvm\/prebuilt"/);
});
