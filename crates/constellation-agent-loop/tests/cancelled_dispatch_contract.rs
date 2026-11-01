//! Direct cancellation of an approved tool dispatch must terminate the turn.
//! The separate `cancel_approved` convenience path cannot prove this invariant
//! because it changes the turn status after dispatch.

use std::fs;

use constellation_agent_loop::{AgentTurn, LoopError, TurnEvent, TurnStatus};
use constellation_core::{AgentId, PermissionGate, RoomId, TaskId};
use constellation_tools::{CancellationToken, LocalExecutor, OperationContext, OperationOutcome};

#[test]
fn precancelled_approved_dispatch_consumes_token_and_ends_turn() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();
    let context = OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    };
    let mut turn = AgentTurn::new(context);
    let operation_id = match turn
        .submit_model(
            r#"{"type":"tool","action":"create_file","path":"cancelled.txt","content":"secret"}"#,
            &executor,
        )
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        TurnEvent::Finished(_) => panic!("file action must require independent consent"),
    };

    let (pending_id, pending) = turn.pending_for_trusted_host().unwrap();
    assert_eq!(pending_id, operation_id);
    let mut gate = PermissionGate::new();
    gate.grant(pending.scope().clone()).unwrap();
    let approval = executor.approve(&gate, operation_id, pending).unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();

    let (receipt, output) = turn
        .dispatch_approved(&executor, &gate, &approval, &cancelled)
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(output.is_none());
    assert_eq!(executor.receipt(operation_id), Some(receipt));
    assert_eq!(turn.status(), TurnStatus::Cancelled);
    assert!(!workspace.path().join("cancelled.txt").exists());
    assert_eq!(fs::read_dir(workspace.path()).unwrap().count(), 0);

    assert_eq!(
        turn.submit_model(r#"{"type":"final","text":"continue"}"#, &executor)
            .err(),
        Some(LoopError::WrongState)
    );
    assert_eq!(
        turn.dispatch_approved(&executor, &gate, &approval, &CancellationToken::new())
            .err(),
        Some(LoopError::WrongState)
    );
}
