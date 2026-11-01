// Non-invasive source-contract guards; does not launch Tauri, contact models,
// grant file permissions, or claim a screen-reader accessibility audit.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const root = resolve(import.meta.dirname, '..');
const source = readFileSync(resolve(root, 'apps/desktop/src/routes/+page.svelte'), 'utf8');
const css = readFileSync(resolve(root, 'apps/desktop/src/routes/studio.css'), 'utf8');

test('all existing evidence import/search/save/load/delete/export native command contracts remain', () => {
  for (const command of [
    'studio_extract_local_document',
    'studio_search_evidence', 'studio_search_saved_evidence',
    'studio_save_evidence', 'studio_list_saved_evidence',
    'studio_load_saved_evidence', 'studio_delete_saved_evidence',
    'studio_export_saved_evidence_markdown',
  ]) {
    assert.match(source, new RegExp(`['"]${command}['"]`), command);
  }
  assert.match(source, /rightsAcknowledged: true/);
  assert.match(source, /original_binary_stored !== false/);
  assert.match(source, /expectedSha256: source\.source_sha256/);
  assert.match(source, /decoder\.decode\(bytes\.slice\(hit\.start_byte, hit\.end_byte\)\) !== hit\.quote/);
  assert.match(source, /evidenceSavedResults = true/);
  assert.match(source, /evidenceImportOriginalSha = ''/);
});

test('all task/workflow and file-approval command contracts remain independently callable', () => {
  for (const command of [
    'studio_create_task', 'studio_run_task', 'studio_cancel_task',
    'studio_preview_workflow', 'studio_create_workflow',
    'studio_list_workflows', 'studio_cancel_workflow', 'studio_run_workflow',
    'studio_run_workflow_step', 'studio_retry_workflow_step',
    'studio_export_workflow_markdown',
  ]) assert.match(source, new RegExp(`['"]${command}['"]`), command);
  assert.match(source, /function approveFileTool\(/);
  assert.match(source, /function rejectFileTool\(/);
  assert.match(source, /permission_granted: false/);
  assert.match(source, /fileToolPreview\.action === 'read'/);
  assert.match(source, /model_proposal/);
});

test('opening a saved source never discards the current draft on the first click', () => {
  assert.match(source, /evidenceText\.trim\(\) && \(evidenceLoadConfirmation\?\.sourceId !== source\.source_id/);
  assert.match(source, /evidenceLoadConfirmation = \{sourceId: source\.source_id, sha256: source\.source_sha256\}/);
  assert.match(source, /loaded\.source_sha256 !== source\.source_sha256/);
  assert.match(source, /loaded\.relative_path !== source\.relative_path/);
  assert.match(source, /Replace current draft/);
  assert.match(source, /Keep my draft/);
  assert.match(source, /Unsaved edits and temporary search results will be lost/);
});

test('selected PDF/DOCX import also waits for explicit draft replacement consent', () => {
  assert.match(source, /async function importEvidenceDocument\(file: File \| undefined, replaceDraft = false\)/);
  assert.match(source, /if \(evidenceText\.trim\(\) && !replaceDraft\)/);
  assert.match(source, /evidencePendingImport = file/);
  assert.match(source, /Replace draft and import/);
  assert.match(source, /The selected file has not been read or stored yet/);
  assert.match(source, /Keep current draft/);
  assert.match(source, /document\.getElementById\('evidence-document'\)\?\.focus\(\)/);
});

test('deletion and cancellation require explicit separate human confirmation', () => {
  assert.match(source, /evidenceDeleteConfirmation\?\.sha256 !== source\.source_sha256/);
  assert.match(source, /'studio_delete_saved_evidence', \{sourceId: source\.source_id, expectedSha256: source\.source_sha256\}/);
  assert.match(source, /Confirm remove/);
  assert.match(source, /Keep source/);
  assert.match(source, /if \(taskCancelConfirmation !== taskId\)/);
  assert.match(source, /if \(workflowCancelConfirmation !== record\.definition\.workflow_id\)/);
  assert.match(source, /Keep task/);
  assert.match(source, /Keep workflow/);
  assert.match(source, /already have received a request/);
  assert.match(source, /document\.getElementById\('evidence-saved-section'\)\?\.focus\(\)/);
});

test('evidence provenance and clipboard consequences are explicitly disclosed', () => {
  assert.match(source, /Original file SHA-256:/);
  assert.match(source, /The original PDF\/DOCX is not stored/);
  assert.match(source, /page\/paragraph mapping is not saved with extracted text/);
  assert.match(source, /Source identity and verification/);
  assert.match(source, /other apps may read them/);
  assert.match(source, /A matching passage does not independently validate its claims/);
  assert.match(source, /Saved sources could not be verified/);
  assert.match(source, /role="alert"/);
  assert.match(source, /aria-live="polite"/);
});

test('progressive disclosure preserves keyboard access and native approval boundaries', () => {
  assert.match(source, /class="evidence-journey"/);
  assert.match(source, /href="#evidence-import-section"/);
  assert.match(source, /href="#evidence-draft-search"/);
  assert.match(source, /href="#evidence-saved-section"/);
  assert.match(source, /<details class="task-extra">/);
  assert.match(source, /<summary>Optional file actions · approval required<\/summary>/);
  assert.match(source, /<details class="workflow-more-actions">/);
  assert.match(source, /Approve this exact file action/);
  assert.match(source, /Reject without executing/);
});

test('mobile, focus, forced-colors and reduced-motion styles remain accessible', () => {
  for (const pattern of [
    /\.studio \.evidence-journey a:focus-visible/,
    /\.studio \.evidence-provenance summary:focus-visible/,
    /\.studio \.task-extra summary:focus-visible/,
    /\.studio \.workflow-more-actions summary:focus-visible/,
    /\.studio \.inline-confirmation/,
    /\.studio \.evidence-source-list/,
    /@media \(max-width:520px\)/,
    /@media \(prefers-reduced-motion:reduce\)/,
    /@media \(forced-colors:active\)/,
  ]) assert.match(css, pattern);
  assert.doesNotMatch(css, /\.studio \.evidence-provenance summary\s*\{[^}]*outline\s*:\s*none/);
});
