import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const page = readFileSync(resolve(root, 'apps/desktop/src/routes/+page.svelte'), 'utf8');
const host = readFileSync(resolve(root, 'apps/desktop/src-tauri/src/native_multi_step_runtime.rs'), 'utf8');
const rootHost = readFileSync(resolve(root, 'apps/desktop/src-tauri/src/lib.rs'), 'utf8');

function assertBoundary(source) {
  const step = source.indexOf('pub(crate) async fn studio_native_multi_step_next(');
  const reservation = source.indexOf('checkpoint.persist_pending_task(', step);
  const budget = source.indexOf('model_proposal_budget::reserve_request(', step);
  const provider = source.indexOf('model_text_reply(', step);
  assert.ok(step >= 0 && reservation > step && budget > reservation && provider > budget,
    'signed ProviderInFlight checkpoint and nonrefundable budget must precede outbound model request');

  const nativeReview = source.indexOf('pub(crate) fn studio_native_multi_step_review(');
  const readIntent = source.indexOf('confirm_native_read_intent(', nativeReview);
  const readMetadata = source.indexOf('prepare_pending_read_for_native_review(', nativeReview);
  const finalConsent = source.indexOf('confirm_native_consent(', nativeReview);
  const proof = source.indexOf('proof.verify(', finalConsent);
  const nativeDispatch = source.indexOf('dispatch_with_native_proof(', finalConsent);
  const helper = source.indexOf('fn dispatch_with_native_proof(');
  const helperCancel = source.indexOf('cancellation.is_cancelled()', helper);
  const helperProof = source.indexOf('proof.verify(', helper);
  const approvalMatch = source.slice(helper, nativeReview).match(/executor\s*\.approve\(&gate,/);
  const executorApprove = approvalMatch ? helper + approvalMatch.index : -1;
  const actualDispatch = source.indexOf('.dispatch_approved_with_observation(', helper);
  assert.ok(nativeReview >= 0 && readIntent > nativeReview &&
    readMetadata > readIntent && finalConsent > readMetadata &&
    proof > finalConsent && nativeDispatch > proof &&
    helper >= 0 && helperCancel > helper && helperProof > helperCancel &&
    executorApprove > helperProof && actualDispatch > executorApprove,
  'read inspection must precede metadata and OS consent; shared cancellation and exact proof must precede one-use executor approval/dispatch');
}

test('production native host reserves authenticated checkpoint and spend before model HTTP', () => {
  assertBoundary(host);
});

test('adversarial mutations of the native gate cannot masquerade as a passing inspection', () => {
  for (const broken of [
    host.replaceAll('confirm_native_consent(', 'removed_native_consent('),
    host.replaceAll('confirm_native_read_intent(', 'removed_native_read_inspection('),
    host.replaceAll('model_proposal_budget::reserve_request(', 'disabled_budget('),
    host.replaceAll('proof.verify(', 'forged_approval('),
  ]) {
    assert.throws(() => assertBoundary(broken));
  }
});

test('Tauri registers every separately user-triggered native command', () => {
  assert.match(rootHost, /mod native_multi_step_runtime;/);
  assert.match(rootHost, /multi_step_runtime:\s*native_multi_step_runtime::NativeMultiStepSessionRegistry/);
  for (const command of [
    'studio_native_multi_step_next', 'studio_native_multi_step_review',
    'studio_native_multi_step_deny', 'studio_native_multi_step_status',
  ]) {
    assert.ok(rootHost.includes('native_multi_step_runtime::' + command),
      'missing production invocation: ' + command);
  }
});

test('renderer cannot represent native consent and never automatically chains steps', () => {
  assert.match(page, /async function requestNextMultiStep\(task: StudioTask\)/);
  assert.match(page, /async function resolveMultiStep\(task: StudioTask, decision: 'review' \| 'deny'\)/);
  assert.match(page, /onclick=\{\(\) => void requestNextMultiStep\(task\)\}/);
  assert.match(page, /onclick=\{\(\) => void resolveMultiStep\(task, 'review'\)\}/);
  assert.match(page, /onclick=\{\(\) => void resolveMultiStep\(task, 'deny'\)\}/);
  assert.match(page, /onclick=\{\(\) => void inspectMultiStep\(task\)\}/);
  assert.doesNotMatch(page, /(?:permission_granted|approval|approved)\s*:\s*true/);
  assert.match(page, /No additional model request or device operation was automatically initiated/);
  assert.match(page, /multiStepStates\[task.id\]\.status === 'quarantined'/);
});

test('native source refuses mutable-route checkpoint replay after restart', () => {
  assert.match(host, /NativeMultiStepCheckpointStore::open_os_vault/);
  assert.match(host, /NativeMultiStepRecovery::Quarantined/);
  assert.match(host, /session\.run\.phase\(\) != MultiStepPhase::Settled/);
  assert.match(host, /session\.provider != profile/);
  assert.match(host, /register_task_request\(&state\.task_cancellations, task_id\)/);
  assert.match(page, /status: 'quarantined'/);
});
