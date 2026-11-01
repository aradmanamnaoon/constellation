//! A native host may feed the model only bounded, provenance-bound outcomes
//! from real executor actions, never a fabricated tool message or file body.
use std::fs;

use constellation_agent_loop::{AgentTurn, LoopError, ObservedToolOutcome, TurnEvent, TurnStatus};
use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{CancellationToken, LocalExecutor, OperationContext, OperationOutcome};

fn principal() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn proposed_id(turn: &mut AgentTurn, executor: &LocalExecutor, json: &str) -> OperationId {
    match turn.submit_model(json, executor).unwrap() {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        TurnEvent::Finished(_) => panic!("tool proposal did not require host review"),
    }
}

#[test]
fn native_approved_create_yields_only_bounded_fixed_vocabulary() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let secret = "PRIVATE_CONFIDENTIAL_SENTINEL_DO_NOT_SEND_TO_MODEL";
    let proposal = serde_json::json!({
        "type":"tool","action":"create_file","path":"private/secret.txt", "content":secret
    });
    fs::create_dir(workspace.path().join("private")).unwrap();
    let id = proposed_id(&mut turn, &executor, &proposal.to_string());
    let (pending_id, pending) = turn.pending_for_trusted_host().unwrap();
    assert_eq!(id, pending_id);
    let mut gate = PermissionGate::new();
    gate.grant(pending.scope().clone()).unwrap();
    let approval = executor.approve(&gate, id, pending).unwrap();
    let (receipt, output, observation) = turn
        .dispatch_approved_with_observation(&executor, &gate, &approval, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert!(output.is_some());
    assert_eq!(
        fs::read_to_string(workspace.path().join("private/secret.txt")).unwrap(),
        secret
    );
    assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    let observation = observation.unwrap();
    assert_eq!(observation.outcome(), ObservedToolOutcome::Succeeded);
    assert_eq!(observation.revision(), turn.revision());
    let safe = observation.model_json_for(context).unwrap();
    assert!(safe.len() <= 256);
    let object: serde_json::Value = serde_json::from_str(&safe).unwrap();
    assert_eq!(object["type"], "trusted_tool_observation");
    assert_eq!(object["action"], "create_file");
    assert_eq!(object["outcome"], "succeeded");
    assert_eq!(object["operation_id"], id.to_string());
    for forbidden in [
        secret,
        "private/secret.txt",
        "sha256",
        "failure_reason",
        "digest",
        "scope",
    ] {
        assert!(
            !safe.contains(forbidden),
            "sensitive key/content leaked: {forbidden}"
        );
    }
    assert_eq!(
        observation.model_json_for(OperationContext {
            agent_id: AgentId::new(),
            ..context
        }),
        Err(LoopError::UnsafeCheckpoint)
    );
    assert_eq!(
        turn.dispatch_approved_with_observation(
            &executor,
            &gate,
            &approval,
            &CancellationToken::new()
        )
        .err(),
        Some(LoopError::WrongState)
    );
}

#[test]
fn authenticated_denial_produces_no_file_metadata_and_cannot_be_duplicated() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        audit.path().join("model-denials.jsonl"),
        [0x69; 32],
    )
    .unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let missing = "nonexistent-confidential-object.txt";
    let proposal = serde_json::json!({"type":"tool","action":"read_file","path":missing});
    let id = proposed_id(&mut turn, &executor, &proposal.to_string());
    assert!(turn.pending_for_trusted_host().is_none());
    assert_eq!(
        turn.deny_with_observation(&executor, OperationId::new())
            .err(),
        Some(LoopError::WrongState)
    );
    let observation = turn.deny_with_observation(&executor, id).unwrap();
    assert_eq!(observation.outcome(), ObservedToolOutcome::Denied);
    assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    let json = observation.model_json_for(context).unwrap();
    assert!(json.contains("denied"));
    assert!(json.contains("read_file"));
    assert!(!json.contains(missing));
    assert_eq!(
        turn.deny_with_observation(&executor, id).err(),
        Some(LoopError::WrongState)
    );
    assert!(!workspace.path().join(missing).exists());
    let audit_text = fs::read_to_string(audit.path().join("model-denials.jsonl")).unwrap();
    assert!(!audit_text.contains(missing));
}

#[test]
fn precancelled_approved_dispatch_stops_without_any_model_observation() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let id = proposed_id(
        &mut turn,
        &executor,
        r#"{"type":"tool","action":"create_file","path":"cancelled.txt","content":"never"}"#,
    );
    let mut gate = PermissionGate::new();
    let (pending_id, prepared) = turn.pending_for_trusted_host().unwrap();
    assert_eq!(id, pending_id);
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor.approve(&gate, id, prepared).unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (receipt, output, observation) = turn
        .dispatch_approved_with_observation(&executor, &gate, &approval, &cancelled)
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(output.is_none());
    assert!(observation.is_none());
    assert_eq!(turn.status(), TurnStatus::Cancelled);
    assert!(!workspace.path().join("cancelled.txt").exists());
}

#[test]
fn real_write_collision_can_reenter_model_only_with_nonsecret_failure_status() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();
    let secret = "UNCHANGED_EXISTING_PRIVATE_CONTENT";
    fs::write(workspace.path().join("existing.txt"), secret).unwrap();
    let context = principal();
    let mut turn = AgentTurn::new(context);
    let id = proposed_id(
        &mut turn,
        &executor,
        r#"{"type":"tool","action":"create_file","path":"existing.txt","content":"new"}"#,
    );
    let mut gate = PermissionGate::new();
    let (_, pending) = turn.pending_for_trusted_host().unwrap();
    gate.grant(pending.scope().clone()).unwrap();
    let approval = executor.approve(&gate, id, pending).unwrap();
    let (receipt, _output, observation) = turn
        .dispatch_approved_with_observation(&executor, &gate, &approval, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    let safe = observation.unwrap().model_json_for(context).unwrap();
    assert!(safe.contains("failed"));
    assert!(!safe.contains(secret));
    assert!(!safe.contains("existing.txt"));
    assert_eq!(
        fs::read_to_string(workspace.path().join("existing.txt")).unwrap(),
        secret
    );
}
