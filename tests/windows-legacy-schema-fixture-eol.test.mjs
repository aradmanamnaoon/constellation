import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import { test } from 'node:test';

const storeSource = readFileSync(new URL('../crates/constellation-core/src/store.rs', import.meta.url), 'utf8');
const startupHarness = readFileSync(new URL('./windows-private-legacy-startup-smoke-20261101.ps1', import.meta.url), 'utf8');
const marker = 'const STORE_LEGACY_SCHEMA_DDL: &str = "';

function extractHistoricalDdl(source) {
  const normalized = source.replace(/\r\n/g, '\n');
  assert.equal(normalized.split(marker).length, 2);
  return normalized.split(marker)[1].split('";')[0];
}

function schemaFor(ddl) {
  const db = new DatabaseSync(':memory:');
  try {
    db.exec(ddl);
    return db.prepare("SELECT type, name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name").all();
  } finally {
    db.close();
  }
}

test('legacy DDL from either Windows CRLF or Linux LF produces the same strict sqlite_master layout', () => {
  const lfSource = storeSource.replace(/\r\n/g, '\n');
  const crlfSource = lfSource.replace(/\n/g, '\r\n');
  const expected = schemaFor(extractHistoricalDdl(lfSource));
  assert.ok(expected.length >= 7);
  assert.deepEqual(schemaFor(extractHistoricalDdl(crlfSource)), expected);

  // Demonstrate the original regression: without normalization the same
  // CREATE statements retain different SQL bytes in sqlite_master.sql.
  const rawDdl = crlfSource.split(marker)[1].split('";')[0];
  assert.notDeepEqual(schemaFor(rawDdl), expected);
});

test('the private real-Windows restart harness normalizes DDL before creating its synthetic profile', () => {
  assert.match(startupHarness, /readFileSync\(source,"utf8"\)\.replace\(\/\\r\\n\/g,"\\n"\)/);
  assert.match(startupHarness, /MIGRATED_USER_VERSION=/);
  assert.match(startupHarness, /version!==1\|\|integrity!=="ok"/);
  assert.match(startupHarness, /RedirectStandardError=\$true/);
});
