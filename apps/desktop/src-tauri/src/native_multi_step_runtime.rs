//! Native-only, user-stepped model/tool host. A model reply can propose an
//! operation, but only the OS consent proof can authorize its one-use dispatch.
//! The renderer supplies a task/provider selection, never operation authority.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use constellation_agent_loop::{
    MultiStepBudget, MultiStepPhase, MultiStepRun, TrustedToolObservation, TurnEvent, TurnStatus,
};
use constellation_core::{
    ChatEntry, ChatRole, OperationId, PermissionGate, ProviderId, ProviderProfile, TaskId,
    TaskState,
};
use constellation_tools::{
    CancellationToken, ExecutionOutput, ExecutionReceipt, LocalExecutor, OperationContext,
    OperationDetails, OperationOutcome,
};
use futures_util::{
    future::{Either, select},
    pin_mut,
};
use serde_json::{Value, json};
use tauri::State;

use crate::{
    StudioState, file_proposal_history, model_proposal_budget, model_text_reply, native_consent,
    native_multi_step_checkpoint::{NativeMultiStepCheckpointStore, NativeMultiStepRecovery},
    provider_credential, register_task_request, tool_host,
};

const CHECKPOINT_FILE: &str = "native-multi-step-checkpoints.sqlite3";

fn refused() -> String {
    "Native multi-step request cannot be authenticated or was already consumed".into()
}

/// Session registry is native application state. Its executor owns the single
/// OS-anchored effects journal for ALL tasks and must remain open until exit.
#[derive(Default)]
pub(crate) struct NativeMultiStepSessionRegistry {
    sessions: Mutex<HashMap<TaskId, Session>>,
    executor: Mutex<Option<LocalExecutor>>,
    // This mutex is independent of the task gate, executor, and sessions.
    // Core cancellation signals it before waiting on the task gate.
    cancellations: Mutex<HashMap<TaskId, CancellationToken>>,
}

struct Session {
    context: OperationContext,
    provider: ProviderProfile,
    run: MultiStepRun,
    sequence: u64,
    budget: MultiStepBudget,
    review: Option<Review>,
    reviewing: bool,
    observations: Vec<String>,
    executed_steps: u8,
}

struct Review {
    operation_id: OperationId,
    path: String,
    read: bool,
}

impl NativeMultiStepSessionRegistry {
    /// Called by `studio_cancel_task` BEFORE taking `task_tool_boundary`.
    /// A poisoned map must not silently discard a cancellation signal.
    pub(crate) fn cancel_task(&self, task_id: TaskId) {
        let mut cancellations = self
            .cancellations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Latch cancellation even if Core asks to cancel while the new
        // session is still initializing under the task gate. Registering a
        // fresh token afterwards must never erase this earlier user decision.
        cancellations.entry(task_id).or_default().cancel();
    }

    fn cancellation_for(&self, task_id: TaskId) -> Result<CancellationToken, String> {
        self.cancellations
            .lock()
            .map_err(|_| refused())?
            .get(&task_id)
            .cloned()
            .ok_or_else(refused)
    }

    fn with_executor<T>(
        &self,
        state: &StudioState,
        action: impl FnOnce(&LocalExecutor) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut lock = self.executor.lock().map_err(|_| refused())?;
        if lock.is_none() {
            let key = tool_host::load_or_create_journal_key(
                &state.file_tool_journals,
                &state.file_tool_workspace,
            )?;
            *lock = Some(tool_host::open_anchored_multi_step_executor(
                &state.file_tool_workspace,
                &state.file_tool_journals,
                key,
            )?);
        }
        action(lock.as_ref().ok_or_else(refused)?)
    }
}

fn checkpoint_store(state: &StudioState) -> Result<NativeMultiStepCheckpointStore, String> {
    NativeMultiStepCheckpointStore::open_os_vault(&state.file_tool_journals.join(CHECKPOINT_FILE))
}

fn signed_budget(run: &MultiStepRun) -> Result<MultiStepBudget, String> {
    serde_json::from_value(
        serde_json::to_value(run.checkpoint_for_host())
            .map_err(|_| refused())?
            .get("budget")
            .cloned()
            .ok_or_else(refused)?,
    )
    .map_err(|_| refused())
}

fn inactive_task_status(state: TaskState) -> Option<Value> {
    // After cancellation, a previously live pending review must not reveal
    // a path/action or suggest it is still actionable in the renderer.
    match state {
        TaskState::Pending => None,
        TaskState::Succeeded => Some(json!({"status":"complete","actions_available":false})),
        TaskState::Cancelled => Some(json!({"status":"cancelled","actions_available":false})),
        _ => Some(json!({"status":"quarantined","actions_available":false})),
    }
}

fn require_pending(state: &StudioState, context: OperationContext) -> Result<(), String> {
    let snapshot = state.store.snapshot().map_err(|_| refused())?;
    if !snapshot.tasks.iter().any(|task| {
        task.id == context.task_id
            && task.room_id == context.room_id
            && task.agent_id == context.agent_id
            && task.state == TaskState::Pending
    }) {
        return Err(refused());
    }
    Ok(())
}

/// An in-flight checkpoint intentionally contains the LAST settled turn,
/// never the unapproved pending proposal. Receiving a model tool request
/// advances the live turn exactly once; a valid native review therefore has
/// revision = saved_settled_revision + 1, not equality. Require that exact
/// transition together with the charged attempt count; do not accept a
/// delayed, forged or silently replayed second model response.
fn authenticated_native_review_revision(
    run: &MultiStepRun,
    saved_settled_revision: u64,
    saved_reservations: u8,
) -> bool {
    run.phase() == MultiStepPhase::NativeReviewRequired
        && run.status() == TurnStatus::AwaitingApproval
        && saved_settled_revision.checked_add(1) == Some(run.revision())
        && saved_reservations == run.provider_reservations()
        && saved_reservations == run.model_attempts()
}

/// Reauthenticate the OS-pinned head before and after an independent native
/// dialog. A live in-memory pending proposal must never be enough to bypass
/// an altered, deleted or rolled-back checkpoint or request counter.
fn verify_review_checkpoint(state: &StudioState, session: &Session) -> Result<(), String> {
    if state
        .multi_step_runtime
        .cancellation_for(session.context.task_id)?
        .is_cancelled()
    {
        return Err(refused());
    }
    require_pending(state, session.context)?;
    let checkpoint = checkpoint_store(state)?;
    match checkpoint.restore_pending_task(&state.store, session.context, &session.provider)? {
        Some(NativeMultiStepRecovery::Quarantined {
            phase: MultiStepPhase::NativeReviewRequired,
            sequence,
            revision,
            reservations,
        }) if sequence == session.sequence
            && authenticated_native_review_revision(&session.run, revision, reservations) => {}
        _ => return Err(refused()),
    }
    let usage = model_proposal_budget::request_usage(
        &state.database_path,
        &session.context.task_id.to_string(),
        &session.provider.id.to_string(),
    )?;
    if usage.task_used != i64::from(session.run.provider_reservations())
        || usage.provider_used < usage.task_used
    {
        return Err(refused());
    }
    Ok(())
}

fn status_of(session: &Session) -> Value {
    if session.reviewing && session.run.phase() == MultiStepPhase::NativeReviewRequired {
        return json!({ "status": "quarantined", "native_review_busy": true,
            "actions_available": false, "revision": session.run.revision() });
    }
    let phase = match session.run.phase() {
        MultiStepPhase::Settled => "settled",
        MultiStepPhase::ProviderInFlight => "provider_in_flight",
        MultiStepPhase::NativeReviewRequired => "native_review_required",
    };
    let status = match session.run.status() {
        TurnStatus::AwaitingModel => "awaiting_model",
        TurnStatus::AwaitingApproval => "awaiting_native_review",
        TurnStatus::Complete => "complete",
        TurnStatus::Cancelled => "cancelled",
    };
    json!({
        "status": status, "phase": phase, "revision": session.run.revision(),
        "provider_id": session.provider.id, "model_reservations": session.run.provider_reservations(),
        "tool_steps": session.run.tool_steps(), "model_limit": session.budget.model_attempts,
        "executed_tool_count": session.executed_steps,
        "tool_limit": session.budget.tool_steps, "native_review_busy": session.reviewing,
        "action": session.review.as_ref().map(|review| if review.read { "read" } else { "create_new" }),
        "relative_path": session.review.as_ref().map(|review| review.path.as_str()),
    })
}

/// User-selected single next provider step. The reserve checkpoint and the
/// durable budget are committed before a billable request is permitted.
/// No model call retries automatically; each subsequent step needs a new UI
/// invocation after an independently settled native observation.
#[tauri::command]
pub(crate) async fn studio_native_multi_step_next(
    state: State<'_, StudioState>,
    task_id: String,
    provider_id: String,
    model_attempts: u8,
    tool_steps: u8,
) -> Result<Value, String> {
    let task_id: TaskId = task_id.parse().map_err(|_| refused())?;
    let provider_id: ProviderId = provider_id.parse().map_err(|_| refused())?;
    let requested_budget =
        MultiStepBudget::new(model_attempts, tool_steps).map_err(|_| refused())?;
    let (profile, credential, history, ticket, context, _lease, cancellation) = {
        let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
        let snapshot = state.store.snapshot().map_err(|_| refused())?;
        let task = snapshot
            .tasks
            .iter()
            .find(|t| t.id == task_id && t.state == TaskState::Pending)
            .ok_or_else(refused)?;
        let agent = snapshot
            .agents
            .iter()
            .find(|a| a.id == task.agent_id)
            .ok_or_else(refused)?;
        let profile = snapshot
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(refused)?
            .clone();
        let credential = provider_credential(&state, &profile)?;
        let context = OperationContext {
            task_id,
            room_id: task.room_id,
            agent_id: task.agent_id,
        };
        let mut history = file_proposal_history(task, agent);
        // Build and validate the entire request before reservation where
        // possible. Network DNS pinning still occurs after the reservation.
        crate::prepare_provider_chat(&profile, &history).map_err(|_| refused())?;
        constellation_provider_probe::completion_endpoint(&profile.base_url)
            .map_err(|_| refused())?;
        let (lease, cancellation) = register_task_request(&state.task_cancellations, task_id)?;
        let checkpoint = checkpoint_store(&state)?;
        let registry = &state.multi_step_runtime;
        registry.with_executor(&state, |_| Ok(()))?; // Authenticate independent effects head.
        let mut sessions = registry.sessions.lock().map_err(|_| refused())?;
        if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(task_id) {
            let recovered = if state.file_tool_journals.join(CHECKPOINT_FILE).exists() {
                checkpoint.restore_pending_task(&state.store, context, &profile)?
            } else {
                None
            };
            let (run, sequence) = match recovered {
                Some(NativeMultiStepRecovery::Settled { run, sequence }) => {
                    // An ephemeral observation cannot be reconstructed from
                    // a settled checkpoint. After a restart, a prior tool
                    // outcome needs separate journal reconciliation; never
                    // silently omit it from the next model input.
                    if run.tool_steps() != 0 {
                        return Err(refused());
                    }
                    (*run, sequence)
                }
                Some(NativeMultiStepRecovery::Quarantined { .. }) => return Err(refused()),
                None => {
                    let run = MultiStepRun::new(context, requested_budget);
                    let sequence = checkpoint.persist_pending_task(
                        &state.store,
                        context,
                        &profile,
                        &run.checkpoint_for_host(),
                        None,
                    )?;
                    (run, sequence)
                }
            };
            // Restored budgets are immutable and checked against this user
            // request, even if the renderer changes the numerical controls.
            let stored_budget = signed_budget(&run)?;
            if stored_budget != requested_budget {
                return Err(refused());
            }
            entry.insert(Session {
                context,
                provider: profile.clone(),
                run,
                sequence,
                budget: stored_budget,
                review: None,
                reviewing: false,
                observations: Vec::new(),
                executed_steps: 0,
            });
        }
        {
            let mut tokens = registry.cancellations.lock().map_err(|_| refused())?;
            tokens.entry(task_id).or_insert_with(CancellationToken::new);
            if tokens
                .get(&task_id)
                .is_some_and(CancellationToken::is_cancelled)
            {
                return Err(refused());
            }
        }
        let session = sessions.get_mut(&task_id).ok_or_else(refused)?;
        if session.context != context
            || session.provider != profile
            || session.budget != requested_budget
            || session.reviewing
            || session.review.is_some()
            || session.run.phase() != MultiStepPhase::Settled
            || session.run.status() != TurnStatus::AwaitingModel
        {
            return Err(refused());
        }
        let usage = model_proposal_budget::request_usage(
            &state.database_path,
            &task_id.to_string(),
            &provider_id.to_string(),
        )?;
        if usage.task_used != i64::from(session.run.provider_reservations())
            || usage.task_used >= usage.task_limit
            || usage.provider_used >= usage.provider_limit
        {
            return Err(refused());
        }
        // Only fixed, host-derived observations enter the continuation; the
        // bounded history preserves causality across multiple tool steps.
        for observation in &session.observations {
            history.push(ChatEntry {
                role: ChatRole::User,
                content: observation.clone(),
            });
        }
        let (ticket, marker) = session
            .run
            .reserve_model(session.run.revision())
            .map_err(|_| refused())?;
        session.sequence = checkpoint.persist_pending_task(
            &state.store,
            context,
            &profile,
            &marker,
            Some(session.sequence),
        )?;
        // A crash/denial between these independent commits leaves the native
        // session quarantined. Never refund or repair an uncertain attempt.
        model_proposal_budget::reserve_request(
            &state.database_path,
            &task_id.to_string(),
            &provider_id.to_string(),
        )?;
        (
            profile,
            credential,
            history,
            ticket,
            context,
            lease,
            cancellation,
        )
    };
    let native_token = state.multi_step_runtime.cancellation_for(task_id)?;
    let cancelled = async {
        loop {
            if native_token.is_cancelled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let request = crate::await_cancellable_task_request(
        model_text_reply(&profile, credential.as_deref().map(String::as_str), history),
        cancellation,
    );
    // Check native cancellation BEFORE polling the provider future. This
    // signal is set before Core's task mutex, even when HTTP remains blocked.
    pin_mut!(cancelled, request);
    let answer = match select(cancelled, request).await {
        Either::Left(((), _)) => Err(refused()),
        Either::Right((reply, _)) => reply,
    };
    let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
    if state
        .multi_step_runtime
        .cancellation_for(task_id)?
        .is_cancelled()
    {
        // A cancellation signaled before obtaining the Core gate must win
        // even if the task transition is still waiting on this same gate.
        return Err(refused());
    }
    require_pending(&state, context)?;
    let checkpoint = checkpoint_store(&state)?;
    let registry = &state.multi_step_runtime;
    let mut sessions = registry.sessions.lock().map_err(|_| refused())?;
    let session = sessions.get_mut(&task_id).ok_or_else(refused)?;
    if session.context != context
        || session.provider != profile
        || session.run.phase() != MultiStepPhase::ProviderInFlight
    {
        return Err(refused());
    }
    let event = registry.with_executor(&state, |executor| match answer {
        Ok(ref response) => match session.run.receive_model(ticket, response, executor) {
            Ok(event) => Ok(Some(event)),
            Err(_) => {
                // Invalid output or preparation error consumes the request.
                // A second provider request after uncertainty is forbidden.
                if session.run.status() == TurnStatus::AwaitingModel {
                    session.run.cancel(executor).map_err(|_| refused())?;
                }
                Err(refused())
            }
        },
        Err(_) => {
            session.run.provider_failed(ticket).map_err(|_| refused())?;
            session.run.cancel(executor).map_err(|_| refused())?;
            Ok(None)
        }
    });
    // Persist the resulting settled OR pending-review marker before exposing
    // any model reply, approval request or allowing another HTTP attempt.
    session.sequence = checkpoint.persist_pending_task(
        &state.store,
        context,
        &profile,
        &session.run.checkpoint_for_host(),
        Some(session.sequence),
    )?;
    match event? {
        Some(TurnEvent::Finished(text)) => {
            let executed_count = session.executed_steps;
            let (task, message) =
                crate::commit_model_proposal_final_text(&state.store, task_id, text)?;
            Ok(
                json!({ "status": "complete", "task": task, "result_message_id": message.id,
                "text": message.content, "tools_executed": executed_count > 0,
                "executed_tool_count": executed_count,
                "tool_steps_proposed": session.run.tool_steps(),
                "tool_execution_provenance": if executed_count > 0 {
                    "native_os_consent_authenticated_journal"
                } else { "none" } }),
            )
        }
        Some(TurnEvent::ApprovalRequired {
            operation_id,
            preview,
        }) => {
            session.review = Some(Review {
                operation_id,
                path: preview.relative_path,
                read: matches!(
                    preview.action,
                    constellation_agent_loop::ProposedAction::ReadFile
                ),
            });
            Ok(status_of(session))
        }
        None => Ok(
            json!({ "status": "provider_failed", "attempt_charged": true,
            "retry_automatic": false, "revision": session.run.revision() }),
        ),
    }
}

/// After an OS refusal, durably deny the one exact proposed operation. The
/// observation has fixed vocabulary and no file content or provider text.
fn deny_locked(
    state: &StudioState,
    task_id: TaskId,
    allow_reviewing: bool,
) -> Result<Value, String> {
    let checkpoint = checkpoint_store(state)?;
    let registry = &state.multi_step_runtime;
    let mut sessions = registry.sessions.lock().map_err(|_| refused())?;
    let session = sessions.get_mut(&task_id).ok_or_else(refused)?;
    if (session.reviewing && !allow_reviewing)
        || session.run.phase() != MultiStepPhase::NativeReviewRequired
    {
        return Err(refused());
    }
    verify_review_checkpoint(state, session)?;
    let review = session.review.as_ref().ok_or_else(refused)?;
    let observation = registry.with_executor(state, |executor| {
        session
            .run
            .deny_with_observation(executor, review.operation_id)
            .map_err(|_| refused())
    })?;
    session.sequence = checkpoint.persist_pending_task(
        &state.store,
        session.context,
        &session.provider,
        &session.run.checkpoint_for_host(),
        Some(session.sequence),
    )?;
    session.observations.push(
        observation
            .model_json_for(session.context)
            .map_err(|_| refused())?,
    );
    session.review = None;
    session.reviewing = false;
    Ok(json!({ "status": "denied", "executed": false, "revision": session.run.revision() }))
}

#[tauri::command]
pub(crate) fn studio_native_multi_step_deny(
    state: State<'_, StudioState>,
    task_id: String,
) -> Result<Value, String> {
    let task_id: TaskId = task_id.parse().map_err(|_| refused())?;
    let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
    let result = deny_locked(&state, task_id, false)?;
    Ok(result)
}

/// This is called after releasing `task_tool_boundary`. Cancellation can be
/// signaled by Core before executor approval, between approval and dispatch,
/// or during a potentially blocking file operation. The same token crosses
/// all three stages; no disconnected per-operation token is ever constructed.
/// `after_approval` is empty in production and a deterministic barrier in
/// adversarial tests, providing a reproducible race at the critical boundary.
fn dispatch_with_native_proof(
    session: &mut Session,
    executor: &LocalExecutor,
    operation_id: OperationId,
    digest: &str,
    proof: &native_consent::NativeConsentProof,
    cancellation: &CancellationToken,
    after_approval: impl FnOnce(),
) -> Result<
    (
        ExecutionReceipt,
        Option<ExecutionOutput>,
        Option<TrustedToolObservation>,
    ),
    String,
> {
    if cancellation.is_cancelled() {
        return Err(refused());
    }
    let (id, prepared) = session.run.pending_for_trusted_host().ok_or_else(refused)?;
    if id != operation_id
        || prepared.digest() != digest
        || !proof.verify(&operation_id.to_string(), prepared.digest())
    {
        return Err(refused());
    }
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone())
        .map_err(|_| refused())?;
    let approval = executor
        .approve(&gate, id, prepared)
        .map_err(|_| refused())?;
    after_approval();
    // The LocalExecutor checks the SAME shared token under its own one-use
    // journal protocol immediately before executing the file operation.
    let result = session
        .run
        .dispatch_approved_with_observation(executor, &gate, &approval, cancellation)
        .map_err(|_| refused())?;
    if result.0.outcome != OperationOutcome::Cancelled {
        session.executed_steps = session.executed_steps.checked_add(1).ok_or_else(refused)?;
    }
    Ok(result)
}

#[tauri::command]
pub(crate) fn studio_native_multi_step_review(
    state: State<'_, StudioState>,
    window: tauri::Window,
    task_id: String,
) -> Result<Value, String> {
    let task_id: TaskId = task_id.parse().map_err(|_| refused())?;
    #[cfg(windows)]
    let hwnd = window.hwnd().map_err(|_| refused())?.0 as isize;
    #[cfg(not(windows))]
    let hwnd = {
        let _ = window;
        0
    };
    let (context, operation_id, read_path) = {
        let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
        let mut sessions = state
            .multi_step_runtime
            .sessions
            .lock()
            .map_err(|_| refused())?;
        let session = sessions.get_mut(&task_id).ok_or_else(refused)?;
        verify_review_checkpoint(&state, session)?;
        if session.reviewing || session.run.phase() != MultiStepPhase::NativeReviewRequired {
            return Err(refused());
        }
        let review = session.review.as_ref().ok_or_else(refused)?;
        let values = (
            session.context,
            review.operation_id,
            review.read.then(|| review.path.clone()),
        );
        session.reviewing = true; // Native dialog is outside every lock.
        values
    };
    if let Some(relative) = read_path.as_deref()
        && native_consent::confirm_native_read_intent(hwnd, &task_id.to_string(), relative)
            != Ok(true)
    {
        let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
        return deny_locked(&state, task_id, true);
    }
    let (path, digest, sha256, bytes, action, create_content) = {
        let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
        require_pending(&state, context)?;
        let mut sessions = state
            .multi_step_runtime
            .sessions
            .lock()
            .map_err(|_| refused())?;
        let session = sessions.get_mut(&task_id).ok_or_else(refused)?;
        verify_review_checkpoint(&state, session)?;
        if !session.reviewing
            || session.context != context
            || session.run.phase() != MultiStepPhase::NativeReviewRequired
            || session.review.as_ref().map(|r| r.operation_id) != Some(operation_id)
        {
            return Err(refused());
        }
        let (preview, content) = state.multi_step_runtime.with_executor(&state, |executor| {
            if read_path.is_some() {
                session
                    .run
                    .prepare_pending_read_for_native_review(executor)
                    .map_err(|_| refused())?;
            }
            let (id, prepared) = session.run.pending_for_trusted_host().ok_or_else(refused)?;
            if id != operation_id {
                return Err(refused());
            }
            let preview = prepared.preview();
            let content = session
                .run
                .pending_create_content_for_native_review()
                .map(str::to_owned);
            Ok((preview, content))
        })?;
        let (path, sha256, bytes, action) = match preview.details {
            OperationDetails::Read {
                canonical_file,
                sha256,
                bytes,
            } => (
                canonical_file,
                Some(sha256),
                bytes,
                native_consent::LocalFileAction::Read,
            ),
            OperationDetails::Write {
                canonical_file,
                sha256,
                bytes,
                ..
            } => (
                canonical_file,
                Some(sha256),
                bytes,
                native_consent::LocalFileAction::CreateNew,
            ),
            OperationDetails::Command { .. } => return Err(refused()),
        };
        (path, preview.digest, sha256, bytes, action, content)
    };
    let operation_str = operation_id.to_string();
    let task_str = task_id.to_string();
    let request = native_consent::NativeConsentRequest {
        operation_id: &operation_str,
        task_id: &task_str,
        canonical_path: &path,
        action,
        sha256: sha256.as_deref(),
        byte_count: bytes,
        operation_digest: &digest,
        create_content: create_content.as_deref(),
    };
    let proof = match native_consent::confirm_native_consent(hwnd, &request) {
        Ok(Some(proof)) if proof.verify(&operation_str, &digest) => proof,
        _ => {
            let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
            return deny_locked(&state, task_id, true);
        }
    };
    let (mut session, cancellation) = {
        // Admission and exact immutable binding checks are serialized with
        // Core task cancellation. The actual executor may block, so the gate
        // and the registry mutex MUST be released before approval/dispatch.
        let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
        require_pending(&state, context)?;
        let cancellation = state.multi_step_runtime.cancellation_for(task_id)?;
        if cancellation.is_cancelled() {
            return Err(refused());
        }
        let mut sessions = state
            .multi_step_runtime
            .sessions
            .lock()
            .map_err(|_| refused())?;
        let session = sessions.get(&task_id).ok_or_else(refused)?;
        verify_review_checkpoint(&state, session)?;
        if !session.reviewing
            || session.context != context
            || session.review.as_ref().map(|r| r.operation_id) != Some(operation_id)
            || session.run.phase() != MultiStepPhase::NativeReviewRequired
        {
            return Err(refused());
        }
        // The signed checkpoint remains NativeReviewRequired while executing.
        // Removing the live run makes concurrent status/review/next attempts
        // report quarantine; the original run is reinserted only if the
        // executor journal and Core state both reconcile successfully.
        let session = sessions.remove(&task_id).ok_or_else(refused)?;
        (session, cancellation)
    };
    let result = state.multi_step_runtime.with_executor(&state, |executor| {
        dispatch_with_native_proof(
            &mut session,
            executor,
            operation_id,
            &digest,
            &proof,
            &cancellation,
            || {},
        )
    });
    // After uncertain dispatch, a pending native-review marker remains on
    // disk and cannot recover into an executable approval after restart.
    // An independent Core cancel can now acquire the task gate while the
    // executor is running; reconciliation must re-check both signals.
    let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
    if cancellation.is_cancelled() {
        return Err(refused());
    }
    require_pending(&state, context)?;
    let (receipt, _output, observation) = result?;
    let checkpoint = checkpoint_store(&state)?;
    session.sequence = checkpoint.persist_pending_task(
        &state.store,
        context,
        &session.provider,
        &session.run.checkpoint_for_host(),
        Some(session.sequence),
    )?;
    if let Some(observation) = observation {
        session
            .observations
            .push(observation.model_json_for(context).map_err(|_| refused())?);
    }
    session.review = None;
    session.reviewing = false;
    let revision = session.run.revision();
    let executed_tool_count = session.executed_steps;
    state
        .multi_step_runtime
        .sessions
        .lock()
        .map_err(|_| refused())?
        .insert(task_id, session);
    Ok(
        json!({ "status": "executed", "outcome": match receipt.outcome {
        OperationOutcome::Succeeded => "succeeded", OperationOutcome::Failed => "failed",
        OperationOutcome::TimedOut => "timed_out", OperationOutcome::OutputLimit => "output_limit",
        OperationOutcome::Cancelled => "cancelled",
    }, "revision": revision, "operation_id": operation_str,
        "executed_tool_count": executed_tool_count,
        "next_model_requires_explicit_user_action": true }),
    )
}

/// Read-only, including across restarts. Neither a missing DB nor a pending
/// marker can construct a model ticket, OS approval, or executor operation.
#[tauri::command]
pub(crate) fn studio_native_multi_step_status(
    state: State<'_, StudioState>,
    task_id: String,
    provider_id: String,
) -> Result<Value, String> {
    let task_id: TaskId = task_id.parse().map_err(|_| refused())?;
    let provider_id: ProviderId = provider_id.parse().map_err(|_| refused())?;
    let _gate = state.task_tool_boundary.lock().map_err(|_| refused())?;
    let snapshot = state.store.snapshot().map_err(|_| refused())?;
    let task = snapshot
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(refused)?;
    if let Some(status) = inactive_task_status(task.state) {
        return Ok(status);
    }
    let profile = snapshot
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(refused)?;
    let context = OperationContext {
        task_id,
        room_id: task.room_id,
        agent_id: task.agent_id,
    };
    if state
        .multi_step_runtime
        .cancellation_for(task_id)
        .is_ok_and(|token| token.is_cancelled())
    {
        return inactive_task_status(TaskState::Cancelled).ok_or_else(refused);
    }
    let sessions = state
        .multi_step_runtime
        .sessions
        .lock()
        .map_err(|_| refused())?;
    if let Some(session) = sessions.get(&task_id) {
        if session.context != context || &session.provider != profile {
            return Err(refused());
        }
        return Ok(status_of(session));
    }
    if !state.file_tool_journals.join(CHECKPOINT_FILE).exists() {
        return Ok(json!({ "status": "not_started" }));
    }
    let checkpoint = checkpoint_store(&state)?;
    match checkpoint.restore_pending_task(&state.store, context, profile)? {
        Some(NativeMultiStepRecovery::Quarantined {
            phase,
            revision,
            reservations,
            ..
        }) => Ok(json!({ "status": "quarantined", "phase": match phase {
                MultiStepPhase::ProviderInFlight => "provider_in_flight",
                MultiStepPhase::NativeReviewRequired => "native_review_required",
                MultiStepPhase::Settled => "settled",
            }, "revision": revision, "model_reservations": reservations })),
        Some(NativeMultiStepRecovery::Settled { run, .. }) => {
            let budget = signed_budget(&run)?;
            Ok(
                json!({ "status": if run.tool_steps() != 0 { "quarantined" } else { match run.status() {
                TurnStatus::AwaitingModel => "awaiting_model",
                TurnStatus::AwaitingApproval => "quarantined",
                TurnStatus::Complete => "complete",
                TurnStatus::Cancelled => "cancelled",
            } }, "revision": run.revision(), "model_reservations": run.provider_reservations(),
                "tool_steps": run.tool_steps(), "model_limit": budget.model_attempts,
                "tool_limit": budget.tool_steps }),
            )
        }
        None => Ok(json!({ "status": "not_started" })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_multi_step_checkpoint::MultiStepAnchor;
    use constellation_core::{AgentId, CredentialSource, ProviderKind, RoomId, StudioStore};
    use constellation_tools::JournalOperationState;
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };

    #[derive(Clone, Default)]
    struct FakeHead(Arc<Mutex<HashMap<TaskId, String>>>);

    impl MultiStepAnchor for FakeHead {
        fn read(&self, task: TaskId) -> Result<Option<String>, String> {
            Ok(self.0.lock().map_err(|_| refused())?.get(&task).cloned())
        }

        fn pin(&self, task: TaskId, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .map_err(|_| refused())?
                .insert(task, value.into());
            Ok(())
        }
    }

    #[test]
    fn fake_provider_read_is_quarantined_without_native_inspection_authority() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("private.txt"), "private document").unwrap();
        let core = StudioStore::open(dir.path().join("project.sqlite3")).unwrap();
        let room = core.create_room("Room").unwrap();
        let agent = core.add_agent("Agent", "Role").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Review a local document")
            .unwrap();
        let context = OperationContext {
            task_id: task.id,
            room_id: room.id,
            agent_id: agent.id,
        };
        let provider = ProviderProfile {
            id: ProviderId::new(),
            display_name: "Test fixture".into(),
            model_id: "no-network".into(),
            base_url: "https://example.com/v1".into(),
            kind: ProviderKind::OpenaiChat,
            credentials: CredentialSource::None,
        };
        core.save_provider_profile(provider.clone()).unwrap();
        let path = dir.path().join("checkpoints.sqlite3");
        let head = FakeHead::default();
        let store =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x67; 32], head.clone()).unwrap();
        let mut run = MultiStepRun::new(context, MultiStepBudget::new(2, 1).unwrap());
        let first = store
            .persist_pending_task(&core, context, &provider, &run.checkpoint_for_host(), None)
            .unwrap();
        let (ticket, pending) = run.reserve_model(0).unwrap();
        let reserved = store
            .persist_pending_task(&core, context, &provider, &pending, Some(first))
            .unwrap();
        let executor = LocalExecutor::new(dir.path(), 8192, 8192).unwrap();
        let fake_response = r#"{"type":"tool","action":"read_file","path":"private.txt"}"#;
        let event = run.receive_model(ticket, fake_response, &executor).unwrap();
        assert!(matches!(event, TurnEvent::ApprovalRequired { .. }));
        assert_eq!(run.phase(), MultiStepPhase::NativeReviewRequired);
        assert!(
            run.pending_for_trusted_host().is_none(),
            "a model-proposed read must not inspect any file before independent native consent"
        );
        store
            .persist_pending_task(
                &core,
                context,
                &provider,
                &run.checkpoint_for_host(),
                Some(reserved),
            )
            .unwrap();
        let reopened =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x67; 32], head).unwrap();
        assert!(matches!(
            reopened
                .restore_pending_task(&core, context, &provider)
                .unwrap(),
            Some(NativeMultiStepRecovery::Quarantined {
                phase: MultiStepPhase::NativeReviewRequired,
                ..
            })
        ));
        // Forged task/principal cannot obtain a review or resume authority.
        let wrong_context = OperationContext {
            task_id: TaskId::new(),
            ..context
        };
        assert!(
            reopened
                .restore_pending_task(&core, wrong_context, &provider)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("private.txt")).unwrap(),
            "private document"
        );
    }

    #[test]
    fn native_review_requires_exact_prior_signed_revision_and_denial_settles() {
        let dir = tempfile::tempdir().unwrap();
        let core = StudioStore::open(dir.path().join("project.sqlite3")).unwrap();
        let room = core.create_room("Native step").unwrap();
        let agent = core.add_agent("Writer", "Role").unwrap();
        let task = core
            .create_task(room.id, agent.id, "A locally simulated file proposal")
            .unwrap();
        let context = OperationContext {
            task_id: task.id,
            room_id: room.id,
            agent_id: agent.id,
        };
        let provider = ProviderProfile {
            id: ProviderId::new(),
            display_name: "Fixture".into(),
            model_id: "no-network".into(),
            base_url: "https://example.com/v1".into(),
            kind: ProviderKind::OpenaiChat,
            credentials: CredentialSource::None,
        };
        core.save_provider_profile(provider.clone()).unwrap();
        let store = NativeMultiStepCheckpointStore::with_anchor(
            &dir.path().join("checkpoints.sqlite3"),
            [0x51; 32],
            FakeHead::default(),
        )
        .unwrap();
        let mut run = MultiStepRun::new(context, MultiStepBudget::new(2, 2).unwrap());
        let first = store
            .persist_pending_task(&core, context, &provider, &run.checkpoint_for_host(), None)
            .unwrap();
        let (ticket, reserved) = run.reserve_model(0).unwrap();
        let second = store
            .persist_pending_task(&core, context, &provider, &reserved, Some(first))
            .unwrap();
        let executor = LocalExecutor::new(dir.path(), 8_192, 8_192).unwrap();
        let event = run
            .receive_model(
                ticket,
                r#"{"type":"tool","action":"create_file","path":"not-created.txt","content":"fixture bytes"}"#,
                &executor,
            )
            .unwrap();
        let operation_id = match event {
            TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
            TurnEvent::Finished(_) => panic!("tool proposal must require a new native approval"),
        };
        let pending = run.checkpoint_for_host();
        // Signed NativeReviewRequired keeps only the LAST settled revision.
        // That is deliberately one less than the active, in-RAM proposal.
        assert_eq!(pending.revision(), 0);
        assert_eq!(run.revision(), 1);
        assert_eq!(pending.provider_reservations(), 1);
        assert!(authenticated_native_review_revision(&run, 0, 1));
        assert!(!authenticated_native_review_revision(&run, 1, 1));
        assert!(!authenticated_native_review_revision(&run, 0, 2));
        assert!(!authenticated_native_review_revision(&run, u64::MAX, 1));
        assert!(!dir.path().join("not-created.txt").exists());

        let review_sequence = store
            .persist_pending_task(&core, context, &provider, &pending, Some(second))
            .unwrap();
        assert!(matches!(
            store.restore_pending_task(&core, context, &provider).unwrap(),
            Some(NativeMultiStepRecovery::Quarantined {
                sequence,
                phase: MultiStepPhase::NativeReviewRequired,
                revision: 0,
                reservations: 1,
            }) if sequence == review_sequence
        ));
        let observation = run.deny_with_observation(&executor, operation_id).unwrap();
        assert!(observation.model_json_for(context).is_ok());
        assert_eq!(run.phase(), MultiStepPhase::Settled);
        assert_eq!(run.status(), TurnStatus::AwaitingModel);
        assert_eq!(run.revision(), 2);
        assert_eq!(run.checkpoint_for_host().revision(), 2);
        assert!(!authenticated_native_review_revision(&run, 0, 1));
        let settled_sequence = store
            .persist_pending_task(
                &core,
                context,
                &provider,
                &run.checkpoint_for_host(),
                Some(review_sequence),
            )
            .unwrap();
        assert_eq!(settled_sequence, review_sequence + 1);
        assert!(matches!(
            store.restore_pending_task(&core, context, &provider).unwrap(),
            Some(NativeMultiStepRecovery::Settled { run, sequence })
                if sequence == settled_sequence
                    && run.revision() == 2
                    && run.provider_reservations() == 1
                    && run.status() == TurnStatus::AwaitingModel
        ));
        assert!(!dir.path().join("not-created.txt").exists());
    }

    #[test]
    fn cancel_signal_before_task_gate_prevents_approved_effect_and_journals_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let private_journal = dir.path().join("private-journal.jsonl");
        std::fs::create_dir(&workspace).unwrap();
        let executor = LocalExecutor::with_authenticated_journal(
            &workspace,
            8_192,
            8_192,
            &private_journal,
            [0x55; 32],
        )
        .unwrap();
        let context = OperationContext {
            task_id: TaskId::new(),
            room_id: RoomId::new(),
            agent_id: AgentId::new(),
        };
        let mut run = MultiStepRun::new(context, MultiStepBudget::new(2, 2).unwrap());
        let (ticket, _) = run.reserve_model(0).unwrap();
        let event = run.receive_model(
            ticket,
            r#"{"type":"tool","action":"create_file","path":"must-not-exist.txt","content":"secret"}"#,
            &executor,
        ).unwrap();
        let operation_id = match event {
            TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
            TurnEvent::Finished(_) => panic!("expected tool request"),
        };
        let digest = run
            .pending_for_trusted_host()
            .unwrap()
            .1
            .digest()
            .to_owned();
        let proof = native_consent::NativeConsentProof::fixture_for_test(
            &operation_id.to_string(),
            &digest,
        );
        let session = Session {
            context,
            provider: ProviderProfile {
                id: ProviderId::new(),
                display_name: "fake".into(),
                model_id: "fake".into(),
                base_url: "https://example.com/v1".into(),
                kind: ProviderKind::OpenaiChat,
                credentials: CredentialSource::None,
            },
            run,
            sequence: 2,
            budget: MultiStepBudget::new(2, 2).unwrap(),
            review: None,
            reviewing: true,
            observations: Vec::new(),
            executed_steps: 0,
        };
        let mut session = session;
        let registry = Arc::new(NativeMultiStepSessionRegistry::default());
        registry
            .cancellations
            .lock()
            .unwrap()
            .insert(context.task_id, CancellationToken::new());
        let token = registry.cancellation_for(context.task_id).unwrap();
        // Simulate Core cancellation waiting for task_tool_boundary. The
        // cancellation signal must reach LocalExecutor even while another
        // thread holds that boundary and the OS approval was issued.
        let task_gate = Arc::new(Mutex::new(()));
        let held_gate = task_gate.lock().unwrap();
        let (sent, received) = mpsc::sync_channel::<()>(1);
        let worker_registry = Arc::clone(&registry);
        let worker_gate = Arc::clone(&task_gate);
        let worker = std::thread::spawn(move || {
            worker_registry.cancel_task(context.task_id);
            sent.send(()).unwrap();
            let _pending_core_cancellation = worker_gate.lock().unwrap();
        });
        let (receipt, output, observation) = dispatch_with_native_proof(
            &mut session,
            &executor,
            operation_id,
            &digest,
            &proof,
            &token,
            || received.recv_timeout(Duration::from_secs(2)).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
        assert!(output.is_none());
        assert!(observation.is_none());
        assert!(token.is_cancelled());
        assert_eq!(session.executed_steps, 0);
        assert!(!workspace.join("must-not-exist.txt").exists());
        assert!(matches!(
            executor.journal_state(operation_id).unwrap(),
            JournalOperationState::Completed {
                outcome: OperationOutcome::Cancelled,
                ..
            }
        ));
        drop(held_gate);
        worker.join().unwrap();
        // A stale second dispatch cannot reuse the same signed approval.
        assert!(
            dispatch_with_native_proof(
                &mut session,
                &executor,
                operation_id,
                &digest,
                &proof,
                &token,
                || {},
            )
            .is_err()
        );
    }

    #[test]
    fn only_one_pending_step_and_same_revision_may_reserve_model() {
        let context = OperationContext {
            task_id: TaskId::new(),
            room_id: RoomId::new(),
            agent_id: AgentId::new(),
        };
        let mut run = MultiStepRun::new(context, MultiStepBudget::new(2, 1).unwrap());
        let (ticket, marker) = run.reserve_model(0).unwrap();
        assert_eq!(marker.phase(), MultiStepPhase::ProviderInFlight);
        assert_eq!(ticket.ordinal(), 1);
        assert!(run.reserve_model(0).is_err());
        let executor = LocalExecutor::new(std::env::temp_dir(), 8192, 8192).unwrap();
        run.provider_failed(ticket).unwrap();
        assert!(
            run.receive_model(ticket, r#"{"type":"final","text":"forged"}"#, &executor)
                .is_err()
        );
        let (second, _) = run.reserve_model(run.revision()).unwrap();
        assert_eq!(second.ordinal(), 2);
        assert!(run.reserve_model(run.revision()).is_err());
    }

    #[test]
    fn registry_and_status_disclose_no_approval_or_document_bytes() {
        let registry = NativeMultiStepSessionRegistry::default();
        assert!(registry.sessions.lock().unwrap().is_empty());
        assert!(registry.executor.lock().unwrap().is_none());
        assert!(MultiStepBudget::new(0, 1).is_err());
        assert!(MultiStepBudget::new(1, 0).is_err());
    }

    #[test]
    fn cancelled_task_never_exposes_stale_native_review_scope() {
        assert!(inactive_task_status(TaskState::Pending).is_none());
        for state in [
            TaskState::Cancelled,
            TaskState::Running,
            TaskState::Queued,
            TaskState::Failed,
            TaskState::Succeeded,
        ] {
            let result = inactive_task_status(state).expect("not pending");
            assert_eq!(result["actions_available"], false);
            assert!(result.get("relative_path").is_none());
            assert!(result.get("operation_id").is_none());
            assert!(result.get("digest").is_none());
            assert_ne!(result["status"], "awaiting_native_review");
        }
        assert_eq!(
            inactive_task_status(TaskState::Cancelled).unwrap()["status"],
            "cancelled"
        );
    }

    #[test]
    fn cancellation_preceding_session_registration_is_a_permanent_tombstone() {
        let registry = NativeMultiStepSessionRegistry::default();
        let pending = TaskId::new();
        registry.cancel_task(pending);
        assert!(registry.cancellation_for(pending).unwrap().is_cancelled());
        registry
            .cancellations
            .lock()
            .unwrap()
            .entry(pending)
            .or_insert_with(CancellationToken::new);
        assert!(registry.cancellation_for(pending).unwrap().is_cancelled());
        // Unrelated tasks have independently scoped signals.
        let unrelated = TaskId::new();
        registry
            .cancellations
            .lock()
            .unwrap()
            .entry(unrelated)
            .or_insert_with(CancellationToken::new);
        assert!(!registry.cancellation_for(unrelated).unwrap().is_cancelled());
    }
}
