import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const backend = read('../apps/desktop/src-tauri/src/evidence_library.rs');
const native = read('../apps/desktop/src-tauri/src/lib.rs');
const page = read('../apps/desktop/src/routes/+page.svelte');

test('saved source results use bounded ranked pages, never uncontrolled retrieval', () => {
  assert.match(backend, /pub\(crate\) fn search\(path: &Path, query: &str, offset: usize\)/);
  assert.match(backend, /offset > 990 \|\| !offset\.is_multiple_of\(PAGE_SIZE\)/);
  assert.match(backend, /\.search\(query, \(offset \+ PAGE_SIZE \+ 1\)\.min\(1_000\)\)/);
  assert.match(backend, /\.skip\(offset\)/);
  assert.match(backend, /\.take\(PAGE_SIZE\)/);
  assert.match(native, /evidence_library::search\(&state\.database_path, &query, offset\.unwrap_or\(0\)\)/);
  assert.match(backend, /saved_evidence_ranked_pages_do_not_hide_matches_after_first_ten/);
});

test('saved evidence pages preserve the existing exact-source quote verification', () => {
  assert.match(page, /searchSavedEvidence\(offset = 0\)/);
  assert.match(page, /studio_search_saved_evidence', \{ query, offset \}/);
  assert.match(page, /response\.offset !== offset/);
  assert.match(page, /source\.source_sha256 !== hit\.source_sha256/);
  assert.match(page, /decoder\.decode\(bytes\.slice\(hit\.start_byte, hit\.end_byte\)\) !== hit\.quote/);
  assert.match(page, /Saved evidence result pages/);
  assert.match(page, /Previous evidence/);
  assert.match(page, /Next evidence/);
  assert.match(page, /Top 1,000 ranked matches maximum/);
});
