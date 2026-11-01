import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const runtime = readFileSync(
  resolve(import.meta.dirname, '../apps/desktop/src-tauri/src/native_multi_step_runtime.rs'),
  'utf8',
);
const helper = readFileSync(
  resolve(import.meta.dirname, 'helper-native-consent.ps1'),
  'utf8',
);

function readReviewSequenceIsSafe(source) {
  const review = source.indexOf('pub(crate) fn studio_native_multi_step_review(');
  const next = source.indexOf('pub(crate) fn studio_native_multi_step_deny(', review);
  const body = source.slice(review, next >= 0 ? next : undefined);
  const check = body.indexOf('verify_review_checkpoint(&state, session)?;');
  const initial = body.indexOf('confirm_native_read_intent(');
  const revalidation = body.indexOf('verify_review_checkpoint(&state, session)?;', initial);
  const preparation = body.indexOf('prepare_pending_read_for_native_review(executor)');
  const proof = body.indexOf('confirm_native_consent(hwnd, &request)');
  const finalFence = body.indexOf('verify_review_checkpoint(&state, session)?;', proof);
  const dispatch = body.indexOf('dispatch_with_native_proof(');
  return review >= 0 && body.length > 100
    && check >= 0 && initial > check
    && revalidation > initial && preparation > revalidation
    && proof > preparation && finalFence > proof && dispatch > finalFence;
}

test('guided read requires native metadata consent before file preparation and exact operation approval before execution', () => {
  assert.equal(readReviewSequenceIsSafe(runtime), true);
});

test('mutations that bypass initial OS read permission or prepare before it fail the gate', () => {
  assert.equal(
    readReviewSequenceIsSafe(runtime.replace('confirm_native_read_intent(', 'insecure_renderer_permission(')),
    false,
  );
  assert.equal(
    readReviewSequenceIsSafe(runtime.replace(
      'prepare_pending_read_for_native_review(executor)',
      'unapproved_read_prepare(executor)',
    )),
    false,
  );
  assert.equal(
    readReviewSequenceIsSafe(runtime.replace(
      'confirm_native_consent(hwnd, &request)',
      'renderer_supplied_approval(hwnd, &request)',
    )),
    false,
  );
});

test('private Windows GUI harness checks deny/approve at both independent native boundaries', () => {
  for (const [file, expected] of [
    ['windows-native-multistep-read-intent-deny-e2e-20261101.ps1', 'pass_no_effect_after_denied_first_read_inspection'],
    ['windows-native-multistep-read-final-deny-e2e-20261101.ps1', 'pass_no_effect_after_denied_second_native_read'],
    ['windows-native-multistep-read-two-consents-e2e-20261101.ps1', 'pass_independent_two_native_read_permissions_exact_sha_one_effect'],
  ]) {
    const text = readFileSync(resolve(import.meta.dirname, file), 'utf8');
    assert.ok(text.includes(expected), file);
    assert.match(text, /ExpectedFixtureSha256 = \$fileSha/);
    assert.match(text, /Get-FileHash/);
    assert.match(text, /RendererReviewLabel = 'Review exact operation in native OS dialog'/);
  }
  assert.match(helper, /ExpectedRelativePath/);
  assert.match(helper, /verified_no_file_metadata_in_native_prompt/);
  assert.match(helper, /RendererApprovalLabel/);
  assert.match(helper, /ExistingNativeReviewInProgress/);
});
