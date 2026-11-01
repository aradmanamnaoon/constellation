//! No direct model-native authority: all OS effects require a distinct,
//! independently approved executor token for the exact pending operation.
use std::fs;

use constellation_agent_loop::{
    LoopError, MultiStepBudget, MultiStepPhase, MultiStepRun, TurnEvent, TurnStatus,
    MAX_MODEL_ATTEMPTS, MAX_TOOL_STEPS,
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
fn executor(path: &std::path::Path) -> LocalExecutor {
    LocalExecutor::new(path, 32_768, 32_768).unwrap()
}
fn budget() -> MultiStepBudget {
    MultiStepBudget::new(6, 3).unwrap()
}
fn propose(
    session: &mut MultiStepRun,
    executor: &LocalExecutor,
    name: &str,
    content: &str,
) -> OperationId {
    let (ticket, marker) = session.reserve_model(session.revision()).unwrap();
    assert_eq!(marker.phase(), MultiStepPhase::ProviderInFlight);
    assert_eq!(
        MultiStepRun::restore_from_host(principal(), &marker.to_bytes().unwrap(), 0).err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    let body = serde_json::json!({
        "type":"tool", "action":"create_file", "path":name, "content":content,
    });
    match session
        .receive_model(ticket, &body.to_string(), executor)
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => panic!("missing independent native approval pause"),
    }
}

#[test]
fn two_actual_executor_steps_each_need_new_approval_and_exact_fence() {
    let dir = tempfile::tempdir().unwrap();
    let host = executor(dir.path());
    let context = principal();
    let mut session = MultiStepRun::new(context, budget());
    let (ticket, marker) = session.reserve_model(0).unwrap();
    assert_eq!(marker.phase(), MultiStepPhase::ProviderInFlight);
    assert!(MultiStepRun::restore_from_host(context, &marker.to_bytes().unwrap(), 0).is_err());
    assert_eq!(session.reserve_model(0).err(), Some(LoopError::WrongState));
    let secret = "SECRET_FILE_BYTES_NEVER_TO_MODEL";
    let first = serde_json::json!({
        "type":"tool", "action":"create_file", "path":"first.txt", "content":secret,
    })
    .to_string();
    let id1 = match session.receive_model(ticket, &first, &host).unwrap() {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => panic!("tool proposal bypassed review"),
    };
    assert!(!dir.path().join("first.txt").exists());
    assert_eq!(session.phase(), MultiStepPhase::NativeReviewRequired);
    assert!(MultiStepRun::restore_from_host(
        context,
        &session.checkpoint_for_host().to_bytes().unwrap(),
        0
    )
    .is_err());
    assert!(session.pending_create_content_for_native_review().is_some());
    let (_, prepared1) = session.pending_for_trusted_host().unwrap();
    let mut gate = PermissionGate::new();
    assert!(host.approve(&gate, id1, prepared1).is_err());
    gate.grant(prepared1.scope().clone()).unwrap();
    let approval1 = host.approve(&gate, id1, prepared1).unwrap();
    let (receipt1, _, observation1) = session
        .dispatch_approved_with_observation(&host, &gate, &approval1, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt1.outcome, OperationOutcome::Succeeded);
    assert_eq!(
        fs::read_to_string(dir.path().join("first.txt")).unwrap(),
        secret
    );
    let safe = observation1.unwrap().model_json_for(context).unwrap();
    assert!(!safe.contains(secret));
    assert!(!safe.contains("first.txt"));
    assert!(!safe.contains("digest"));
    assert_eq!(session.phase(), MultiStepPhase::Settled);
    assert_eq!(session.tool_steps(), 1);
    let checkpoint = session.checkpoint_for_host();
    let prior_revision = checkpoint.revision();
    let mut session =
        MultiStepRun::restore_from_host(context, &checkpoint.to_bytes().unwrap(), prior_revision)
            .unwrap();
    assert_eq!(session.status(), TurnStatus::AwaitingModel);
    assert_eq!(session.model_attempts(), 1);

    let id2 = propose(&mut session, &host, "second.txt", "second");
    assert_ne!(id1, id2);
    assert_eq!(
        session
            .dispatch_approved_with_observation(&host, &gate, &approval1, &CancellationToken::new())
            .err(),
        Some(LoopError::WrongState)
    );
    assert!(!dir.path().join("second.txt").exists());
    let (_, prepared2) = session.pending_for_trusted_host().unwrap();
    let mut independent_gate = PermissionGate::new();
    assert!(host.approve(&independent_gate, id2, prepared2).is_err());
    independent_gate.grant(prepared2.scope().clone()).unwrap();
    let approval2 = host.approve(&independent_gate, id2, prepared2).unwrap();
    let (receipt2, _, observation2) = session
        .dispatch_approved_with_observation(
            &host,
            &independent_gate,
            &approval2,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(receipt2.outcome, OperationOutcome::Succeeded);
    assert!(observation2.is_some());
    assert_eq!(
        fs::read_to_string(dir.path().join("second.txt")).unwrap(),
        "second"
    );
    assert_eq!(session.tool_steps(), 2);
    assert_eq!(session.model_attempts(), 2);
    let finished = session.checkpoint_for_host();
    let mut restored = MultiStepRun::restore_from_host(
        context,
        &finished.to_bytes().unwrap(),
        finished.revision(),
    )
    .unwrap();
    let (last, _) = restored.reserve_model(restored.revision()).unwrap();
    assert!(matches!(
        restored.receive_model(
            last,
            r#"{"type":"final","text":"Two separately approved files created"}"#,
            &host
        ),
        Ok(TurnEvent::Finished(_))
    ));
    assert_eq!(restored.status(), TurnStatus::Complete);
    assert_eq!(
        restored.reserve_model(restored.revision()).err(),
        Some(LoopError::WrongState)
    );
}

#[test]
fn duplicate_model_responses_invalid_or_oversize_count_once_and_stale_fails() {
    let dir = tempfile::tempdir().unwrap();
    let host = executor(dir.path());
    let context = principal();
    let mut session = MultiStepRun::new(context, MultiStepBudget::new(4, 1).unwrap());
    let (ticket, _) = session.reserve_model(0).unwrap();
    assert_eq!(
        session.receive_model(ticket, "{broken", &host).err(),
        Some(LoopError::InvalidProposal)
    );
    assert_eq!(session.model_attempts(), 1);
    assert_eq!(session.provider_reservations(), 1);
    assert_eq!(
        session.receive_model(ticket, "{}", &host).err(),
        Some(LoopError::StaleModelStep)
    );
    let (next, _) = session.reserve_model(session.revision()).unwrap();
    let oversized = "x".repeat(16 * 1024 + 1);
    assert_eq!(
        session.receive_model(next, &oversized, &host).err(),
        Some(LoopError::Oversized)
    );
    assert_eq!(session.model_attempts(), 2);
    assert_eq!(session.provider_reservations(), 2);
    let stable = session.checkpoint_for_host();
    let recovered =
        MultiStepRun::restore_from_host(context, &stable.to_bytes().unwrap(), stable.revision())
            .unwrap();
    assert_eq!(recovered.provider_reservations(), 2);
    assert_eq!(recovered.model_attempts(), 2);
    assert!(!dir.path().join("unapproved").exists());
}

#[test]
fn per_session_tool_budget_rejects_second_effect_but_allows_final_answer() {
    let dir = tempfile::tempdir().unwrap();
    let host = executor(dir.path());
    let context = principal();
    let mut session = MultiStepRun::new(context, MultiStepBudget::new(3, 1).unwrap());
    let id = propose(&mut session, &host, "first.txt", "ok");
    session.deny_with_observation(&host, id).unwrap();
    let (second, _) = session.reserve_model(session.revision()).unwrap();
    assert_eq!(
        session
            .receive_model(
                second,
                r#"{"type":"tool","action":"create_file","path":"unsafe.txt","content":"never"}"#,
                &host
            )
            .err(),
        Some(LoopError::StepLimit)
    );
    assert_eq!(session.tool_steps(), 1);
    assert_eq!(session.model_attempts(), 2);
    assert!(!dir.path().join("unsafe.txt").exists());
    let (third, _) = session.reserve_model(session.revision()).unwrap();
    assert!(matches!(
        session.receive_model(third, r#"{"type":"final","text":"No more tools"}"#, &host),
        Ok(TurnEvent::Finished(_))
    ));
}

#[test]
fn cancellation_and_uncertain_crash_must_not_replay_provider_or_approved_action() {
    let dir = tempfile::tempdir().unwrap();
    let host = executor(dir.path());
    let context = principal();
    let mut run = MultiStepRun::new(context, budget());
    let (ticket, pending) = run.reserve_model(0).unwrap();
    assert_eq!(pending.phase(), MultiStepPhase::ProviderInFlight);
    assert_eq!(
        MultiStepRun::restore_from_host(context, &pending.to_bytes().unwrap(), 0).err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    run.cancel(&host).unwrap();
    assert_eq!(run.status(), TurnStatus::Cancelled);
    assert_eq!(run.provider_reservations(), 1);
    assert_eq!(run.model_attempts(), 0);
    let terminal = run.checkpoint_for_host();
    let recovered = MultiStepRun::restore_from_host(
        context,
        &terminal.to_bytes().unwrap(),
        terminal.revision(),
    )
    .unwrap();
    assert_eq!(recovered.status(), TurnStatus::Cancelled);
    assert_eq!(
        run.receive_model(ticket, r#"{"type":"final","text":"late"}"#, &host)
            .err(),
        Some(LoopError::StaleModelStep)
    );

    let mut another = MultiStepRun::new(context, budget());
    let id = propose(&mut another, &host, "cancel.txt", "never");
    let (oid, prepared) = another.pending_for_trusted_host().unwrap();
    assert_eq!(id, oid);
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = host.approve(&gate, id, prepared).unwrap();
    assert_eq!(
        another.cancel(&host).err(),
        Some(LoopError::ApprovedCancellationRequired)
    );
    assert!(MultiStepRun::restore_from_host(
        context,
        &another.checkpoint_for_host().to_bytes().unwrap(),
        0
    )
    .is_err());
    let receipt = another.cancel_approved(&host, &gate, &approval).unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(!dir.path().join("cancel.txt").exists());
    assert_eq!(another.status(), TurnStatus::Cancelled);
    assert!(MultiStepRun::restore_from_host(
        context,
        &another.checkpoint_for_host().to_bytes().unwrap(),
        another.revision()
    )
    .is_ok());
}

#[test]
fn failed_provider_is_charged_and_checkpoints_reject_tamper_rollback_cross_principal() {
    let context = principal();
    let mut session = MultiStepRun::new(context, MultiStepBudget::new(2, 2).unwrap());
    let (ticket, _) = session.reserve_model(0).unwrap();
    session.provider_failed(ticket).unwrap();
    assert_eq!(session.provider_reservations(), 1);
    assert_eq!(session.model_attempts(), 1);
    assert_eq!(
        session.provider_failed(ticket).err(),
        Some(LoopError::StaleModelStep)
    );
    let stable = session.checkpoint_for_host();
    let bytes = stable.to_bytes().unwrap();
    assert_eq!(
        MultiStepRun::restore_from_host(principal(), &bytes, 0).err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    assert_eq!(
        MultiStepRun::restore_from_host(context, &bytes, stable.revision() + 1).err(),
        Some(LoopError::UnsafeCheckpoint)
    );
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["reservations"] = serde_json::json!(0);
    assert!(
        MultiStepRun::restore_from_host(context, &serde_json::to_vec(&json).unwrap(), 0).is_err()
    );
    json["reservations"] = serde_json::json!(1);
    json["phase"] = serde_json::json!("native_review_required");
    assert!(
        MultiStepRun::restore_from_host(context, &serde_json::to_vec(&json).unwrap(), 0).is_err()
    );
    json["phase"] = serde_json::json!("settled");
    json["extra_permission"] = serde_json::json!(true);
    assert!(
        MultiStepRun::restore_from_host(context, &serde_json::to_vec(&json).unwrap(), 0).is_err()
    );
    assert!(MultiStepRun::restore_from_host(context, &vec![b'x'; 2049], 0).is_err());
    assert_eq!(
        MultiStepBudget::new(MAX_MODEL_ATTEMPTS + 1, 1).err(),
        Some(LoopError::StepLimit)
    );
    assert_eq!(
        MultiStepBudget::new(1, MAX_TOOL_STEPS + 1).err(),
        Some(LoopError::StepLimit)
    );
    assert_eq!(MultiStepBudget::new(0, 0).err(), Some(LoopError::StepLimit));
}

#[test]
fn every_failed_provider_attempt_spends_budget_even_after_authenticated_restore() {
    let context = principal();
    let mut run = MultiStepRun::new(context, MultiStepBudget::new(2, 1).unwrap());
    let (first, pending) = run.reserve_model(run.revision()).unwrap();
    assert_eq!(pending.provider_reservations(), 1);
    assert!(MultiStepRun::restore_from_host(context, &pending.to_bytes().unwrap(), 0).is_err());
    run.provider_failed(first).unwrap();
    let anchor = run.checkpoint_for_host();
    let mut run =
        MultiStepRun::restore_from_host(context, &anchor.to_bytes().unwrap(), anchor.revision())
            .unwrap();
    let (second, _) = run.reserve_model(run.revision()).unwrap();
    run.provider_failed(second).unwrap();
    assert_eq!(run.provider_reservations(), 2);
    assert_eq!(run.model_attempts(), 2);
    assert_eq!(
        run.reserve_model(run.revision()).err(),
        Some(LoopError::StepLimit)
    );
    let final_anchor = run.checkpoint_for_host();
    let mut run = MultiStepRun::restore_from_host(
        context,
        &final_anchor.to_bytes().unwrap(),
        final_anchor.revision(),
    )
    .unwrap();
    assert_eq!(
        run.reserve_model(run.revision()).err(),
        Some(LoopError::StepLimit)
    );
}
