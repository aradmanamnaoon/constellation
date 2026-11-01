//! The native host may reserve one final, text-only model request only after
//! separately authenticated real ToolHost completion. This library test
//! exercises the *reservation* fence, not the OS-vault or provider network.
use constellation_agent_loop::{AgentTurn, LoopError, TurnEvent, TurnStatus};
use constellation_core::{AgentId, OperationId, RoomId, TaskId};
use constellation_tools::{LocalExecutor, OperationContext};

fn fixture() -> (
    tempfile::TempDir,
    LocalExecutor,
    OperationContext,
    AgentTurn,
) {
    let root = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(root.path(), 16_384, 16_384).unwrap();
    let context = OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    };
    let turn = AgentTurn::new(context);
    (root, executor, context, turn)
}

#[test]
fn a_settled_native_proposal_can_consume_only_one_final_followup_attempt() {
    let (_root, executor, context, mut turn) = fixture();
    let proposal = r#"{"type":"tool","action":"read_file","path":"not-inspected.txt"}"#;
    let id = match turn.submit_model(proposal, &executor).unwrap() {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        TurnEvent::Finished(_) => panic!("unexpected final"),
    };
    // Original model proposal is durably denied. The actual independently
    // reviewed ToolHost action has a different ID and is verified outside
    // this library before any reservation can occur.
    turn.deny(&executor, id).unwrap();
    assert_eq!(
        (turn.revision(), turn.model_attempts(), turn.tool_steps()),
        (2, 1, 1)
    );
    assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    assert_eq!(
        turn.reserve_single_final_followup(1),
        Err(LoopError::StaleModelStep)
    );
    assert_eq!(turn.reserve_single_final_followup(2).unwrap(), 3);
    assert_eq!(
        (turn.revision(), turn.model_attempts(), turn.tool_steps()),
        (3, 2, 1)
    );
    let sealed = turn.checkpoint_for_host().unwrap();
    let mut reopened = AgentTurn::restore_from_host(context, sealed, 3).unwrap();
    assert_eq!(
        reopened.reserve_single_final_followup(3),
        Err(LoopError::WrongState)
    );
    assert_eq!(
        reopened.reserve_single_final_followup(2),
        Err(LoopError::StaleModelStep)
    );
    assert_eq!(reopened.model_attempts(), 2);
    // A forged/unknown operation cannot be used as an executor approval.
    assert!(executor.receipt(OperationId::new()).is_none());
}

#[test]
fn no_model_followup_is_reserved_without_the_first_exact_settled_tool_step() {
    let (_root, executor, _context, mut turn) = fixture();
    assert_eq!(
        turn.reserve_single_final_followup(0),
        Err(LoopError::WrongState)
    );
    let id = match turn
        .submit_model(
            r#"{"type":"tool","action":"read_file","path":"unknown.txt"}"#,
            &executor,
        )
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        TurnEvent::Finished(_) => unreachable!(),
    };
    assert_eq!(
        turn.reserve_single_final_followup(1),
        Err(LoopError::WrongState)
    );
    turn.cancel(&executor).unwrap();
    assert_eq!(
        turn.reserve_single_final_followup(turn.revision()),
        Err(LoopError::WrongState)
    );
    assert_eq!(turn.deny(&executor, id), Err(LoopError::WrongState));
}
