/**
 * Independent OFFLINE Windows NSIS payload identity probe and adversarial tests.
 * Node-only: no installer launch, no extraction directory, no registry writes.
 *
 * Node's test runner: node --test tests/windows-nsis-payload-provenance-20261101.test.mjs
 * Read-only proof CLI: node tests/windows-nsis-payload-provenance-20261101.test.mjs
 *   --installer ABS --installer-sha256 HEX --direct ABS --direct-sha256 HEX
 *   --nsis ABS --nsis-sha256 HEX --sevenzip ABS --sevenzip-sha256 HEX
 *
 * All four immutable identities must be independently pinned by the caller;
 * calculating a digest of an untrusted candidate alone is not provenance.
 * Even an accepted byte comparison NEVER represents installed-app acceptance.
 */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, realpathSync, statSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';

const MAX_EXE = 50 * 1024 * 1024;
const MAX_SETUP = 128 * 1024 * 1024;
const MAX_LIST = 1024 * 1024;
const MAX_SCRIPT = 1024 * 1024;
const MAX_EXTRACTOR = 30 * 1024 * 1024;
const EXPECTED_EXE = 'constellation-desktop.exe';
const MARKER = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_', 'ascii');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
class ProbeError extends Error {}
const fail = (reason) => { throw new ProbeError(reason); };
const hexDigest = (value) => typeof value === 'string' && /^[a-f\d]{64}$/i.test(value);

function readPinnedFile(filename, expectedDigest, sizeCap, extension) {
  if (!path.isAbsolute(filename) || !hexDigest(expectedDigest)) fail('Absolute path and pinned SHA-256 are required');
  const before = lstatSync(filename);
  if (!before.isFile() || before.isSymbolicLink() || before.size < 1 || before.size > sizeCap ||
      path.extname(filename).toLowerCase() !== extension) fail('Input type, size or extension rejected');
  // Symlink/mutable inputs are not immutable release evidence.
  if (path.normalize(realpathSync.native(filename)).toLowerCase() !== path.normalize(filename).toLowerCase()) {
    fail('Aliases or reparse paths refused');
  }
  const bytes = readFileSync(filename);
  const after = statSync(filename);
  if (before.size !== after.size || before.mtimeMs !== after.mtimeMs ||
      before.ino !== after.ino || before.dev !== after.dev) fail('Input changed during verification');
  const digest = sha256(bytes);
  if (digest !== expectedDigest.toLowerCase()) fail('Pinned source SHA-256 mismatch');
  return { bytes, digest, path: filename };
}

function checkNsisSource(nsis, directPath) {
  const content = nsis.toString('utf8');
  if (content.includes('\0')) fail('Malformed NSIS source');
  const source = [...content.matchAll(/^\s*!define\s+MAINBINARYSRCPATH\s+"([^"]+)"\s*$/gm)];
  const copies = [...content.matchAll(/^\s*File\s+"\$\{MAINBINARYSRCPATH\}"\s*$/gm)];
  const mode = [...content.matchAll(/^\s*!define\s+INSTALLMODE\s+"([^"]+)"\s*$/gm)];
  const webview = [...content.matchAll(/^\s*!define\s+INSTALLWEBVIEW2MODE\s+"([^"]*)"\s*$/gm)];
  if (source.length !== 1 || copies.length !== 1 || mode.length !== 1 || webview.length !== 1) {
    fail('Ambiguous or missing generated NSIS binary copy and mode');
  }
  const src = path.win32.normalize(source[0][1]);
  const expected = path.win32.normalize(directPath);
  if (!path.win32.isAbsolute(src) || src.toLowerCase() !== expected.toLowerCase()) {
    fail('NSIS MAINBINARYSRCPATH does not point to pinned direct EXE');
  }
  if (mode[0][1] !== 'currentUser') fail('Unexpected NSIS install mode');
  // WebView2 modes are observed and never executed in this read-only proof.
  return { install_mode: mode[0][1], webview_mode: webview[0][1] || 'skip_not_verified' };
}

function parseSevenZipListing(raw) {
  const text = typeof raw === 'string' ? raw : raw.toString('utf8');
  if (Buffer.byteLength(text, 'utf8') > MAX_LIST || text.includes('\0')) fail('Unbounded archive listing');
  const split = text.match(/\r?\n-{10,}\r?\n/);
  if (!split) fail('Archive entry separator absent');
  const header = text.slice(0, split.index);
  const archiveTypes = [...header.matchAll(/^Type = ([^\r\n]+)$/gm)];
  if (archiveTypes.length !== 1 || archiveTypes[0][1] !== 'Nsis') fail('Archive type is not NSIS');
  const entries = text.slice(split.index + split[0].length)
    .split(/\r?\n\s*\r?\n/).map((block) => {
      const fields = Object.create(null);
      for (const line of block.split(/\r?\n/).filter(Boolean)) {
        const parsed = /^([^=]+?) = ?(.*)$/.exec(line);
        if (!parsed) fail('Malformed 7-Zip listing entry');
        const key = parsed[1].trim();
        if (Object.hasOwn(fields, key)) fail('Duplicate 7-Zip metadata field');
        fields[key] = parsed[2];
      }
      return fields;
    }).filter((entry) => entry.Path !== undefined);
  if (entries.length < 1 || entries.length > 4096) fail('Empty or excessive archive listing');
  const seen = new Set();
  for (const entry of entries) {
    const name = entry.Path;
    const normalized = name.replaceAll('\\', '/');
    const parts = normalized.split('/');
    if (!name || name.includes(':') || name.startsWith('\\') || name.startsWith('/') ||
        parts.some((part) => !part || part === '.' || part === '..' || /[. ]$/.test(part))) {
      fail('Unsafe archive entry path');
    }
    const key = normalized.toLowerCase();
    if (seen.has(key)) fail('Archive has case-folded duplicate entry');
    seen.add(key);
    if (entry.Size && (!/^\d+$/.test(entry.Size) || Number(entry.Size) > MAX_EXE)) {
      // Other resources can be larger; reject an unsafe/unbounded listing.
      fail('Archive entry size exceeds bounded inspection');
    }
  }
  const executable = entries.filter((entry) => {
    const last = entry.Path.replaceAll('\\', '/').split('/').at(-1);
    return last.toLowerCase() === EXPECTED_EXE;
  });
  if (executable.length !== 1) fail('Expected one unambiguous packaged desktop executable');
  if (executable[0].Size && Number(executable[0].Size) > MAX_EXE) fail('Packaged EXE too large');
  if (/^[dls]/i.test(executable[0].Attributes ?? '')) fail('Packaged EXE must be a normal file');
  return { entry: executable[0].Path, count: entries.length };
}

function comparePayload(direct, packaged) {
  if (!Buffer.isBuffer(direct) || !Buffer.isBuffer(packaged) ||
      direct.length === 0 || direct.length > MAX_EXE || packaged.length > MAX_EXE ||
      direct.length !== packaged.length) fail('EXE length or size mismatch');
  if (direct.subarray(0, 2).toString('ascii') !== 'MZ' ||
      packaged.subarray(0, 2).toString('ascii') !== 'MZ') fail('Windows PE header missing');
  const offsets = [];
  for (let i = 0; i < direct.length; i += 1) {
    if (direct[i] !== packaged[i]) {
      if (offsets.length === 4) fail('Too many changed payload bytes');
      offsets.push(i);
    }
  }
  let result;
  if (offsets.length === 0) result = 'exact_match';
  else {
    const i = offsets[0];
    const isMarker = offsets.length === 3 && offsets.every((n, j) => n === i + j) &&
      i >= MARKER.length && direct.subarray(i - MARKER.length, i).equals(MARKER) &&
      direct.subarray(i, i + 3).toString('ascii') === 'UNK' &&
      packaged.subarray(i, i + 3).toString('ascii') === 'NSS';
    if (!isMarker) fail('Packaged bytes differ beyond the permitted Tauri NSIS marker');
    result = 'tauri_marker_only';
  }
  return {
    comparison: result,
    bytes: direct.length,
    differing_bytes: offsets.length,
    marker_offset: offsets[0] ?? null,
    direct_sha256: sha256(direct),
    packaged_sha256: sha256(packaged),
  };
}

function runSevenZip(binary, args, limit) {
  const result = spawnSync(binary, args, {
    windowsHide: true, shell: false, timeout: 20_000,
    maxBuffer: limit,
    encoding: null,
  });
  if (result.error || result.signal || result.status !== 0 || !Buffer.isBuffer(result.stdout) ||
      (result.stderr?.length ?? 0) !== 0) fail('Offline archive reader failed closed');
  return result.stdout;
}

function inspectPinnedArchive(options, extractor = runSevenZip) {
  const installer = readPinnedFile(options.installer, options['installer-sha256'], MAX_SETUP, '.exe');
  const direct = readPinnedFile(options.direct, options['direct-sha256'], MAX_EXE, '.exe');
  const nsis = readPinnedFile(options.nsis, options['nsis-sha256'], MAX_SCRIPT, '.nsi');
  const reader = readPinnedFile(options.sevenzip, options['sevenzip-sha256'], MAX_EXTRACTOR, '.exe');
  if (!/^7z(?:a|z)?\.exe$/i.test(path.basename(reader.path))) fail('Expected a pinned 7-Zip reader');
  const metadata = checkNsisSource(nsis.bytes, direct.path);
  const listing = extractor(reader.path, ['l', '-slt', '--', installer.path], MAX_LIST);
  const selected = parseSevenZipListing(listing);
  const payload = extractor(reader.path, ['x', '-so', '--', installer.path, selected.entry], MAX_EXE + MAX_LIST);
  const comparison = comparePayload(direct.bytes, payload);
  // The archive can change while 7-Zip runs. Refuse stale evidence even if
  // the extracted payload accidentally happens to match the direct binary.
  readPinnedFile(installer.path, installer.digest, MAX_SETUP, '.exe');
  readPinnedFile(direct.path, direct.digest, MAX_EXE, '.exe');
  readPinnedFile(nsis.path, nsis.digest, MAX_SCRIPT, '.nsi');
  readPinnedFile(reader.path, reader.digest, MAX_EXTRACTOR, '.exe');
  return {
    schema: 'constellation.offline_nsis_payload.v1',
    status: 'OFFLINE_PAYLOAD_MATCH_ONLY',
    release_ready: false,
    installed_app_verified: false,
    signing_verified: false,
    installer_sha256: installer.digest,
    nsis_sha256: nsis.digest,
    extractor_sha256: reader.digest,
    archive_type: 'Nsis',
    archive_entries: selected.count,
    selected_entry: selected.entry,
    ...metadata,
    ...comparison,
  };
}

function cliArgs(argv) {
  const required = ['installer', 'installer-sha256', 'direct', 'direct-sha256',
    'nsis', 'nsis-sha256', 'sevenzip', 'sevenzip-sha256'];
  if (argv.length !== required.length * 2) fail('Exactly eight --key value pairs are required');
  const result = {};
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i].startsWith('--') ? argv[i].slice(2) : '';
    if (!required.includes(key) || Object.hasOwn(result, key) || argv[i + 1].startsWith('--')) {
      fail('Unknown/duplicate CLI argument');
    }
    result[key] = argv[i + 1];
  }
  for (const key of required) if (!Object.hasOwn(result, key)) fail('Required CLI pin missing');
  return result;
}

const listing = (paths) => `7-Zip 19.00\n\n--\nPath = C:\\build\\setup.exe\nType = Nsis\nPhysical Size = 9000\n\n----------\n${paths.map((p) => `Path = ${p}\nSize = 148\nAttributes =\nMethod = LZMA:23\n`).join('\n')}`;
const directFixture = Buffer.concat([
  Buffer.from('MZ'), Buffer.alloc(53), MARKER, Buffer.from('UNK'), Buffer.alloc(75),
]);
const packagedFixture = Buffer.from(directFixture);
packagedFixture.write('NSS', 55 + MARKER.length, 'ascii');

if (process.argv.length > 2) {
  try {
    const report = inspectPinnedArchive(cliArgs(process.argv.slice(2)));
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } catch (error) {
    // Never echo untrusted file paths, archive contents or shell diagnostics.
    process.stderr.write(`OFFLINE_NSIS_REJECTED: ${error instanceof ProbeError ? error.message : 'Untrusted input or I/O failure'}\n`);
    process.exitCode = 2;
  }
} else {
  test('fixture exact bytes and only the 3-byte Tauri UNK→NSS replacement pass', () => {
    assert.equal(comparePayload(directFixture, directFixture).comparison, 'exact_match');
    const report = comparePayload(directFixture, packagedFixture);
    assert.equal(report.comparison, 'tauri_marker_only');
    assert.equal(report.differing_bytes, 3);
    assert.equal(report.marker_offset, 55 + MARKER.length);
  });

  test('truncation, extra changes, swapped marker direction, wrong marker and non-PE reject', () => {
    assert.throws(() => comparePayload(directFixture, packagedFixture.subarray(1)), /length/);
    const corrupt = Buffer.from(packagedFixture);
    corrupt[corrupt.length - 1] = 10;
    assert.throws(() => comparePayload(directFixture, corrupt), /permitted/);
    assert.throws(() => comparePayload(packagedFixture, directFixture), /permitted/);
    const spoof = Buffer.from(directFixture);
    spoof.write('NSS', 70, 'ascii');
    assert.throws(() => comparePayload(directFixture, spoof), /permitted/);
    const early = Buffer.from('MZUNK');
    assert.throws(() => comparePayload(early, Buffer.from('MZNSS')), /permitted/);
    assert.throws(() => comparePayload(Buffer.from('zz'), Buffer.from('zz')), /PE/);
  });

  test('only exact one NSIS archive entry is accepted, not path traversal or duplicate aliases', () => {
    assert.equal(parseSevenZipListing(listing(['$PLUGINSDIR\\System.dll', EXPECTED_EXE])).entry, EXPECTED_EXE);
    assert.throws(() => parseSevenZipListing(listing(['x/../constellation-desktop.exe'])), /Unsafe/);
    assert.throws(() => parseSevenZipListing(listing(['x\\constellation-desktop.exe', EXPECTED_EXE])), /unambiguous/);
    assert.throws(() => parseSevenZipListing(listing([EXPECTED_EXE, 'CONSTELLATION-DESKTOP.EXE'])), /duplicate/);
    assert.throws(() => parseSevenZipListing(listing(['other.exe'])), /unambiguous/);
    assert.throws(() => parseSevenZipListing(listing([EXPECTED_EXE]).replace('Type = Nsis', 'Type = 7z')), /NSIS/);
    assert.throws(() => parseSevenZipListing(listing([EXPECTED_EXE]).replace('Size = 148', 'Size = 999999999999')), /size/);
    assert.throws(() => parseSevenZipListing(listing(['C:\\Windows\\constellation-desktop.exe'])), /Unsafe/);
  });

  test('NSIS source must bind ONE direct path and ONE macro File operation', () => {
    const filename = 'C:\\build\\constellation-desktop.exe';
    const script = `!define MAINBINARYSRCPATH "${filename}"\n!define INSTALLMODE "currentUser"\n!define INSTALLWEBVIEW2MODE "downloadBootstrapper"\nFile "\${MAINBINARYSRCPATH}"\n`;
    assert.equal(checkNsisSource(Buffer.from(script), filename).webview_mode, 'downloadBootstrapper');
    assert.throws(() => checkNsisSource(Buffer.from(script.replace(filename, 'C:\\other.exe')), filename), /MAINBINARY/);
    assert.throws(() => checkNsisSource(Buffer.from(`${script}${script}`), filename), /Ambiguous/);
    assert.throws(() => checkNsisSource(Buffer.from(script.replace('currentUser', 'perMachine')), filename), /mode/);
    assert.throws(() => checkNsisSource(Buffer.from(script.replace('File "\${MAINBINARYSRCPATH}"', 'Delete "temp"')), filename), /Ambiguous/);
  });

  test('missing hash pins, duplicated args and ignored CLI switches fail closed', () => {
    assert.throws(() => cliArgs(['--installer', 'foo']), /Exactly/);
    const args = ['installer', 'installer-sha256', 'direct', 'direct-sha256',
      'nsis', 'nsis-sha256', 'sevenzip', 'sevenzip-sha256'].flatMap((x) => [`--${x}`, 'v']);
    assert.equal(cliArgs(args).sevenzip, 'v');
    assert.throws(() => cliArgs([...args.slice(0, -2), '--installer', 'v']), /duplicate/);
    assert.throws(() => cliArgs([...args.slice(0, -2), '--execute', 'v']), /Unknown/);
    assert(!hexDigest('0'.repeat(63)));
    assert(hexDigest('a'.repeat(64)));
  });

  test('malformed lists, pathological sizes and duplicate case aliases are denied without external effects', () => {
    assert.throws(() => parseSevenZipListing('not a 7z listing'), /separator/);
    assert.throws(() => parseSevenZipListing(listing(Array.from({ length: 4097 }, (_, i) => `entry-${i}.txt`))), /Unbounded|excessive/);
    assert.throws(() => parseSevenZipListing(listing(['x\\.\\constellation-desktop.exe'])), /Unsafe/);
    assert.throws(() => parseSevenZipListing(listing(['x\\..\\constellation-desktop.exe'])), /Unsafe/);
    assert.throws(() => parseSevenZipListing(listing([EXPECTED_EXE, 'CONSTELLATION-DESKTOP.EXE'])), /duplicate/);
    assert.throws(() => parseSevenZipListing(listing([EXPECTED_EXE]).replace(`Path = ${EXPECTED_EXE}`, `Path = ${EXPECTED_EXE}\nPath = other.exe`)), /Duplicate 7-Zip/);
    assert.throws(() => runSevenZip('nonexistent-offline-reader.exe', ['l', '-slt'], MAX_LIST), /failed closed/);
  });

  test('contract never claims installed app, signature or public release from offline payload bytes', () => {
    const source = readFileSync(new URL(import.meta.url), 'utf8');
    for (const guard of ['release_ready: false', 'installed_app_verified: false',
      'signing_verified: false', "status: 'OFFLINE_PAYLOAD_MATCH_ONLY'"]) {
      assert(source.includes(guard), `Missing honest status ${guard}`);
    }
    assert.doesNotMatch(source, /spawnSync\([^,]+,\s*\[\s*'\/S'/);
  });
}
