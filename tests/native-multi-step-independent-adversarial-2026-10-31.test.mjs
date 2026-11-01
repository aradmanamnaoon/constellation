import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

// Cross-file, dependency-free source contracts. These are static adversarial
// checks, not native Rust, live-provider, OS-dialog, or integration execution.
const root = resolve(import.meta.dirname, '..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const native = read('apps/desktop/src-tauri/src/native_multi_step_runtime.rs');
const checkpoint = read('apps/desktop/src-tauri/src/native_multi_step_checkpoint.rs');
const core = read('apps/desktop/src-tauri/src/lib.rs');
const loop = read('crates/constellation-agent-loop/src/multi_step.rs');
const view = read('apps/desktop/src/routes/+page.svelte');
const host = read('apps/desktop/src-tauri/src/tool_host.rs');

function section(source, start, end) {
  const a = source.indexOf(start);
  const b = source.indexOf(end, a + start.length);
  assert(a >= 0 && b > a, `Source boundary missing: ${start} → ${end}`);
  return source.slice(a, b);
}

const next = section(native, 'pub(crate) async fn studio_native_multi_step_next(', '/// After an OS refusal');
const denial = section(native, 'fn deny_locked(', '#[tauri::command]\npub(crate) fn studio_native_multi_step_deny(');
const dispatchHelper = section(native, 'fn dispatch_with_native_proof(', '#[tauri::command]\npub(crate) fn studio_native_multi_step_review(');
const review = section(native, 'pub(crate) fn studio_native_multi_step_review(', '/// Read-only, including across restarts.');
const status = section(native, 'pub(crate) fn studio_native_multi_step_status(', '#[cfg(test)]');
const cancel = section(core, 'fn studio_cancel_task(', '/// A process restart invalidates previous in-flight calls.');

function reviewViolations(source) {
  const problems = [];
  const intent = source.indexOf('native_consent::confirm_native_read_intent(');
  const prepare = source.indexOf('.prepare_pending_read_for_native_review(');
  const consent = source.indexOf('native_consent::confirm_native_consent(');
  const approve = dispatchHelper.indexOf('.approve(&gate, id, prepared)');
  const dispatch = dispatchHelper.indexOf('.dispatch_approved_with_observation(');
  const rechecks = [...source.matchAll(/verify_review_checkpoint\(&state, session\)\?/g)];
  if (intent < 0 || prepare < 0 || intent >= prepare) problems.push('READ_INSPECTED_BEFORE_OS_INTENT');
  const helperCall = source.indexOf('dispatch_with_native_proof(');
  if (consent < 0 || approve < 0 || dispatch < 0 || helperCall <= consent ||
      !(approve < dispatch) || !dispatchHelper.includes('proof.verify(')) {
    problems.push('TOOL_APPROVED_BEFORE_NATIVE_CONSENT');
  }
  if (rechecks.length < 3 || rechecks.at(-1).index <= consent) {
    problems.push('NO_POST_DIALOG_CHECKPOINT_REAUTH');
  }
  if (source.indexOf('require_pending(&state, context)?;', consent) < 0) {
    problems.push('MISSING_CORE_CANCEL_FENCE_AFTER_DIALOG');
  }
  if (!/session\.reviewing\s*=\s*true/.test(source) || !/if\s*!session\.reviewing/.test(source)) {
    problems.push('DUPLICATE_REVIEW_ADMISSION_UNFENCED');
  }
  if (!/session\.review\.as_ref\(\)\.map\(\|r\| r\.operation_id\) != Some\(operation_id\)/.test(source)) {
    problems.push('OPERATION_ID_SUBSTITUTION_UNFENCED');
  }
  return problems;
}

test('Tauri exposes exactly the native commands used by the guided Task renderer', () => {
  for (const cmd of ['next', 'review', 'deny', 'status']) {
    assert.match(core, new RegExp(`native_multi_step_runtime::studio_native_multi_step_${cmd},`));
    assert.match(native, new RegExp(`pub\\(crate\\) (?:async )?fn studio_native_multi_step_${cmd}\\(`));
    assert.match(view, new RegExp(`studio_native_multi_step_${cmd}`));
  }
  assert.match(core, /multi_step_runtime:\s*native_multi_step_runtime::NativeMultiStepSessionRegistry::default\(\)/);
});

test('OS review orders the first read consent, preparation, exact approval and post-dialog reauthentication', () => {
  assert.deepEqual(reviewViolations(review), []);
  assert.match(denial, /verify_review_checkpoint\(state, session\)\?/);
  assert.match(denial, /session\.reviewing && !allow_reviewing/);
});

test('adversarial mutations removing read-intent, native proof or post-dialog fence fail the review contract', () => {
  const withoutIntent = review.replace('native_consent::confirm_native_read_intent(', 'removed_native_read_intent(');
  assert(reviewViolations(withoutIntent).includes('READ_INSPECTED_BEFORE_OS_INTENT'));
  const withoutConsent = review.replace('native_consent::confirm_native_consent(', 'untrusted_renderer_consent(');
  assert(reviewViolations(withoutConsent).includes('TOOL_APPROVED_BEFORE_NATIVE_CONSENT'));
  const withoutPostDialogFence = review.replaceAll('verify_review_checkpoint(&state, session)?;', 'removed_reauthentication()?;');
  assert(reviewViolations(withoutPostDialogFence).includes('NO_POST_DIALOG_CHECKPOINT_REAUTH'));
  const withoutReviewFlag = review.replace('session.reviewing = true;', 'session.reviewing = false;');
  assert(reviewViolations(withoutReviewFlag).includes('DUPLICATE_REVIEW_ADMISSION_UNFENCED'));
});

test('provider and task identities are validated through current Core data and authenticated checkpoint', () => {
  assert.match(next, /session\.context != context\s*\|\|\s*session\.provider != profile/);
  assert.match(status, /session\.context != context \|\| &session\.provider != profile/);
  assert.match(review, /verify_review_checkpoint\(&state, session\)\?/);
  assert.match(checkpoint, /if !snapshot\.providers\.iter\(\)\.any\(\|p\| p == provider\)/);
  assert.match(checkpoint, /provider_profile_sha256/);
  assert.match(checkpoint, /task_context_sha256/);
  assert.match(checkpoint, /hmac::verify\(/);
  assert.match(checkpoint, /Self::verify_pin\(&saved, sequence, &mac\)/);
});

test('provider attempts are durably reserved and cancelled response cannot become a final task', () => {
  const pinned = next.indexOf('session.sequence = checkpoint.persist_pending_task(', next.indexOf('reserve_model('));
  const counted = next.indexOf('model_proposal_budget::reserve_request(');
  const request = next.indexOf('model_text_reply(&profile, credential.as_deref()');
  assert(pinned >= 0 && counted > pinned && request > counted,
    'checkpoint and budget must precede any provider request');
  assert(next.indexOf('require_pending(&state, context)?;', request) > request,
    'Core cancellation must win before model reply is accepted');
  assert(next.indexOf('checkpoint.persist_pending_task(', request) < next.indexOf('commit_model_proposal_final_text('),
    'settled checkpoint precedes final saved message');
  assert.match(loop, /cp\.phase != MultiStepPhase::Settled/);
  assert.match(next, /Some\(NativeMultiStepRecovery::Quarantined \{ \.\. \}\) => return Err\(refused\(\)\)/);
});

test('one-use review cannot replay after concurrent IPC denial or after a process restart', () => {
  assert.match(review, /session\.reviewing = true/);
  assert.match(review, /if session\.reviewing \|\| session\.run\.phase\(\) != MultiStepPhase::NativeReviewRequired/);
  assert.match(denial, /if \(session\.reviewing && !allow_reviewing\)/);
  assert.match(status, /NativeMultiStepRecovery::Quarantined/);
  assert.match(next, /if run\.tool_steps\(\) != 0/);
  assert.match(host, /open_anchored_multi_step_executor/);
});

test('P1 gate: Core cancel can interrupt an approved native tool execution without waiting for the task mutex', () => {
  assert.match(cancel, /task_tool_boundary\s*\.lock\(\)/);
  // An independent, task-scoped cancellation channel/token must be wired into
  // dispatch. A newly constructed local token is permanently false and cannot
  // be signalled by studio_cancel_task; this gate intentionally fails until fixed.
  assert.doesNotMatch(review, /let cancellation = CancellationToken::new\(\);/,
    'P1: native review dispatch allocates an uncancellable token');
  assert.match(review, /cancellation_for\(task_id\)\?/);
  assert.match(review, /dispatch_with_native_proof\(/);
  assert.match(dispatchHelper, /dispatch_approved_with_observation\(executor, &gate, &approval, cancellation\)/);
  assert.match(cancel, /multi_step_runtime\.cancel_task\(task_id\)/);
  assert(cancel.indexOf('multi_step_runtime.cancel_task(task_id)') < cancel.indexOf('.task_tool_boundary'),
    'Core must signal the token before waiting on task authorization');
});

test('P2 gate: terminal completion does not misrepresent already executed tools', () => {
  const complete = section(next, 'Some(TurnEvent::Finished(text)) => {', 'Some(TurnEvent::ApprovalRequired {');
  // The first model step can have zero tools, while a later final response can
  // follow several approved read/create operations. Constant false is unsafe.
  assert.doesNotMatch(complete, /"tools_executed"\s*:\s*false/,
    'P2: final task unconditionally reports no tools after earlier approved steps');
});

test('P1 gate: every claimed executed-tool count can increase after a journal-backed native effect', () => {
  const complete = section(next, 'Some(TurnEvent::Finished(text)) => {', 'Some(TurnEvent::ApprovalRequired {');
  assert.match(complete, /executed_count\s*>\s*0/);
  assert.match(dispatchHelper, /session\.executed_steps\s*(?:\+=|=\s*session\.executed_steps)/,
    'P1: executed_steps is read as provenance but is never incremented after native dispatch');
  assert.match(dispatchHelper, /result\.0\.outcome != OperationOutcome::Cancelled/);
});
