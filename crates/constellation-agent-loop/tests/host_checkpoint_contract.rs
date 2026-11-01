//! Durable host-provided progress, stale async replies, and explicit consent.
//! Only the test's trusted host can grant/approve file operations.
use std::{collections::VecDeque, fs, io::Write};

use constellation_agent_loop::{
    AgentTurn, HostTurnCheckpoint, LoopError, ModelStepSource, ModelStepView, TurnEvent,
    TurnStatus, MAX_MODEL_ATTEMPTS, MAX_TOOL_STEPS,
};
use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{CancellationToken, LocalExecutor, OperationContext, OperationOutcome};

fn principal() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

struct ScriptedProvider {
    replies: VecDeque<Result<String, LoopError>>,
    received: Vec<ModelStepView>,
}

impl ScriptedProvider {
    fn new(replies: impl IntoIterator<Item = Result<String, LoopError>>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
            received: Vec::new(),
        }
    }
}

impl ModelStepSource for ScriptedProvider {
    fn next_response(&mut self, view: ModelStepView) -> Result<String, LoopError> {
        self.received.push(view);
        self.replies.pop_front().expect("unexpected provider call")
    }
}

fn round_trip(turn: &AgentTurn, principal: OperationContext) -> AgentTurn {
    let checkpoint = turn.checkpoint_for_host().unwrap();
    let json = serde_json::to_vec(&checkpoint).unwrap();
    let restored: HostTurnCheckpoint = serde_json::from_slice(&json).unwrap();
    AgentTurn::restore_from_host(principal, restored, checkpoint.revision()).unwrap()
}

#[test]
fn host_synced_checkpoint_reopens_without_refunding_failed_model_attempts() {
    let workspace = tempfile::tempdir().unwrap();
    let private_host = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 8192, 8192).unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    assert_eq!(
        turn.submit_model("invalid-json", &executor).err(),
        Some(LoopError::InvalidProposal)
    );
    let prior_revision = turn.revision();
    let checkpoint = turn.checkpoint_for_host().unwrap();
    let stored = private_host.path().join("trusted-checkpoint.json");
    let mut handle = fs::File::create(&stored).unwrap();
    handle
        .write_all(&serde_json::to_vec(&checkpoint).unwrap())
        .unwrap();
    handle.sync_all().unwrap();
    drop(handle);
    drop(turn);
    // This fixture represents trusted host storage; production must also
    // authenticate checkpoint bytes and separately persist its revision floor.
    let checkpoint: HostTurnCheckpoint =
        serde_json::from_slice(&fs::read(&stored).unwrap()).unwrap();
    let mut restored = AgentTurn::restore_from_host(context, checkpoint, prior_revision).unwrap();
    assert_eq!(restored.model_attempts(), 1);
    assert_eq!(restored.revision(), 1);
    assert_eq!(
        restored
            .submit_model_at_revision(0, r#"{"type":"final","text":"stale"}"#, &executor,)
            .err(),
        Some(LoopError::StaleModelStep)
    );
    assert!(matches!(
        restored.submit_model_at_revision(
            restored.revision(),
            r#"{"type":"final","text":"recovered"}"#,
            &executor,
        ).unwrap(),
        TurnEvent::Finished(ref text) if text == "recovered"
    ));
    assert_eq!(restored.model_attempts(), 2);
}

#[test]
fn denial_then_restored_model_step_still_requires_independent_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        audit.path().join("journal.jsonl"),
        [0x51; 32],
    )
    .unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let mut provider = ScriptedProvider::new([
        Ok(r#"{"type":"tool","action":"create_file","path":"denied.rs","content":"no"}"#.into()),
        Ok(r#"{"type":"tool","action":"create_file","path":"approved.rs","content":"pub fn ok() {}"}"#.into()),
        Ok(r#"{"type":"final","text":"Approved artifact written"}"#.into()),
    ]);
    let initial_revision = turn.revision();
    let denied_id = match turn
        .advance_from_source(initial_revision, &mut provider, &executor)
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => panic!("tool must require approval"),
    };
    assert_eq!(
        turn.checkpoint_for_host().err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    assert!(!workspace.path().join("denied.rs").exists());
    turn.deny(&executor, denied_id).unwrap();
    let denial_revision = turn.revision();
    turn = round_trip(&turn, context);
    assert_eq!(turn.model_attempts(), 1);
    assert_eq!(turn.tool_steps(), 1);
    assert_eq!(
        turn.submit_model_at_revision(
            initial_revision,
            r#"{"type":"final","text":"late provider reply"}"#,
            &executor
        )
        .err(),
        Some(LoopError::StaleModelStep)
    );
    assert_eq!(turn.revision(), denial_revision);

    let operation_id = match turn
        .advance_from_source(turn.revision(), &mut provider, &executor)
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => panic!("tool must require approval"),
    };
    assert!(!workspace.path().join("approved.rs").exists());
    let (_, prepared) = turn.pending_for_trusted_host().unwrap();
    let mut gate = PermissionGate::new();
    assert!(executor.approve(&gate, operation_id, prepared).is_err());
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor.approve(&gate, operation_id, prepared).unwrap();
    assert_eq!(
        turn.checkpoint_for_host().err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    let (receipt, _) = turn
        .dispatch_approved(&executor, &gate, &approval, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert_eq!(
        fs::read(workspace.path().join("approved.rs")).unwrap(),
        b"pub fn ok() {}"
    );
    assert_eq!(
        turn.dispatch_approved(&executor, &gate, &approval, &CancellationToken::new())
            .err(),
        Some(LoopError::WrongState)
    );
    turn = round_trip(&turn, context);
    assert_eq!(turn.tool_steps(), 2);
    assert_eq!(turn.model_attempts(), 2);
    assert!(matches!(
        turn.advance_from_source(turn.revision(), &mut provider, &executor).unwrap(),
        TurnEvent::Finished(ref text) if text == "Approved artifact written"
    ));
    assert_eq!(turn.status(), TurnStatus::Complete);
    assert_eq!(provider.received.len(), 3);
    assert_eq!(executor.receipt(operation_id), Some(receipt));
    assert!(!workspace.path().join("denied.rs").exists());
    let done = round_trip(&turn, context);
    assert_eq!(done.status(), TurnStatus::Complete);
    assert_eq!(done.model_attempts(), 3);
}

#[test]
fn checkpoint_rejects_forged_principal_rollback_pending_approval_and_budget() {
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let checkpoint = turn.checkpoint_for_host().unwrap();
    assert!(AgentTurn::restore_from_host(principal(), checkpoint.clone(), 0).is_err());
    assert_eq!(
        AgentTurn::restore_from_host(context, checkpoint.clone(), 1).err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    let mut json = serde_json::to_value(&checkpoint).unwrap();
    json["status"] = serde_json::json!("awaiting_approval");
    let invalid: HostTurnCheckpoint = serde_json::from_value(json.clone()).unwrap();
    assert!(AgentTurn::restore_from_host(context, invalid, 0).is_err());
    json["status"] = serde_json::json!("awaiting_model");
    json["model_attempts"] = serde_json::json!(MAX_MODEL_ATTEMPTS + 1);
    let invalid: HostTurnCheckpoint = serde_json::from_value(json.clone()).unwrap();
    assert!(AgentTurn::restore_from_host(context, invalid, 0).is_err());
    json["model_attempts"] = serde_json::json!(0);
    json["tool_steps"] = serde_json::json!(MAX_TOOL_STEPS + 1);
    let invalid: HostTurnCheckpoint = serde_json::from_value(json.clone()).unwrap();
    assert!(AgentTurn::restore_from_host(context, invalid, 0).is_err());
    json["tool_steps"] = serde_json::json!(0);
    json["version"] = serde_json::json!(42);
    let invalid: HostTurnCheckpoint = serde_json::from_value(json.clone()).unwrap();
    assert!(AgentTurn::restore_from_host(context, invalid, 0).is_err());
    json["version"] = serde_json::json!(1);
    json["arbitrary_authorization"] = serde_json::json!(true);
    assert!(serde_json::from_value::<HostTurnCheckpoint>(json).is_err());
    // The restored state is intentionally unprivileged; a fabricated
    // checkpoint is never accepted as an executor approval.
    assert_eq!(turn.model_attempts(), 0);
    turn.cancel(&LocalExecutor::new(tempfile::tempdir().unwrap().path(), 8192, 8192).unwrap())
        .unwrap();
    let cancelled = round_trip(&turn, context);
    assert_eq!(cancelled.status(), TurnStatus::Cancelled);
}

#[test]
fn provider_failures_and_bad_json_consume_budget_even_across_restarts() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 8192, 8192).unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let mut provider = ScriptedProvider::new((0..MAX_MODEL_ATTEMPTS).map(|i| {
        if i % 2 == 0 {
            Err(LoopError::Provider)
        } else {
            Ok("broken-json".into())
        }
    }));
    for attempt in 0..MAX_MODEL_ATTEMPTS {
        let revision = turn.revision();
        assert_eq!(
            turn.advance_from_source(revision, &mut provider, &executor)
                .err(),
            Some(if attempt % 2 == 0 {
                LoopError::Provider
            } else {
                LoopError::InvalidProposal
            })
        );
        turn = round_trip(&turn, context);
        assert_eq!(turn.model_attempts(), attempt + 1);
        assert_eq!(turn.tool_steps(), 0);
        assert_eq!(turn.revision(), u64::from(attempt + 1));
    }
    let original_calls = provider.received.len();
    assert_eq!(
        turn.advance_from_source(turn.revision(), &mut provider, &executor)
            .err(),
        Some(LoopError::StepLimit)
    );
    assert_eq!(provider.received.len(), original_calls);
    assert_eq!(turn.status(), TurnStatus::Cancelled);
    let recovered = round_trip(&turn, context);
    assert_eq!(recovered.model_attempts(), MAX_MODEL_ATTEMPTS);
    assert_eq!(recovered.status(), TurnStatus::Cancelled);
    assert_eq!(
        turn.submit_model_at_revision(0, r#"{"type":"final","text":"bypass"}"#, &executor)
            .err(),
        Some(LoopError::StaleModelStep)
    );
    assert!(fs::read_dir(workspace.path()).unwrap().next().is_none());
}

#[test]
fn cancelled_approved_operation_is_terminal_across_restore() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 8192, 8192).unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let id = match turn
        .submit_model(
            r#"{"type":"tool","action":"create_file","path":"never.rs","content":"never"}"#,
            &executor,
        )
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => unreachable!(),
    };
    let (_, prepared) = turn.pending_for_trusted_host().unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor.approve(&gate, id, prepared).unwrap();
    assert_eq!(
        turn.cancel(&executor).err(),
        Some(LoopError::ApprovedCancellationRequired)
    );
    assert_eq!(
        turn.checkpoint_for_host().err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    let cancelled = turn.cancel_approved(&executor, &gate, &approval).unwrap();
    assert_eq!(cancelled.outcome, OperationOutcome::Cancelled);
    let mut restored = round_trip(&turn, context);
    assert_eq!(restored.status(), TurnStatus::Cancelled);
    assert!(restored.pending_for_trusted_host().is_none());
    assert_eq!(
        restored
            .submit_model_at_revision(
                restored.revision(),
                r#"{"type":"final","text":"continue"}"#,
                &executor,
            )
            .err(),
        Some(LoopError::WrongState)
    );
    assert!(!workspace.path().join("never.rs").exists());
    assert_eq!(executor.receipt(id), Some(cancelled));
    assert_eq!(
        restored.cancel(&executor).err(),
        Some(LoopError::WrongState)
    );
    // No model-generated OperationId can revive the consumed approval.
    assert_ne!(id, OperationId::new());
}
