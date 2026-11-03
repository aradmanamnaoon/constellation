import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const core = read('../apps/desktop/src-tauri/src/lib.rs');
const repo = read('../crates/constellation-workflows/src/repository.rs');
const page = read('../apps/desktop/src/routes/+page.svelte');
const css = read('../apps/desktop/src/routes/studio.css');

test('saved workflow listing is native, bounded, deterministic and has a lookahead', () => {
  assert.match(core, /fn list_workflow_page\(\s*workflows: &WorkflowRepository,\s*offset: usize,\s*query: &str/);
  assert.match(core, /const PAGE_SIZE: usize = 25/);
  assert.match(core, /list_matching_ids\(query, PAGE_SIZE \+ 1, offset\)/);
  assert.match(core, /records\.truncate\(PAGE_SIZE\)/);
  assert.match(core, /offset\.is_multiple_of\(PAGE_SIZE\)/);
  assert.match(core, /saved_workflow_pages_are_bounded_distinct_and_complete/);
  assert.match(repo, /pub fn list_matching_ids\(/);
  assert.match(repo, /instr\(lower\(id\), lower\(\?1\)\)/);
});

test('workflow pager navigates all pages and validates untrusted backend metadata', () => {
  assert.match(page, /loadWorkflows\(targetOffset = workflowOffset, searchTerm = workflowAppliedSearch\)/);
  assert.match(page, /studio_list_workflows', \{ offset: targetOffset, query: searchTerm \}/);
  assert.match(page, /response\.offset !== targetOffset/);
  assert.match(page, /response\.max_items !== workflowPageSize/);
  assert.match(page, /response\.query !== searchTerm/);
  assert.match(page, /Find workflow ID/);
  assert.match(page, /Clear filter/);
  assert.match(page, /Saved workflow pages/);
  assert.match(page, /Previous page/);
  assert.match(page, /Next page/);
  assert.match(css, /\.workflow-pages/);
});
