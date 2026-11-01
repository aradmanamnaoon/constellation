//! A rejected model read is journaled without looking up its target.
//! The trusted host has deliberately not requested metadata inspection.

use std::fs;

use constellation_agent_loop::{AgentTurn, LoopError, ProposedAction, TurnEvent, TurnStatus};
use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{LocalExecutor, OperationContext, OperationRequest, ToolError};

const TEST_KEY: [u8; 32] = [0x29; 32]; // Only used for offline test journals.

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn pending_read(turn: &mut AgentTurn, executor: &LocalExecutor, path: &str) -> OperationId {
    let proposal = serde_json::json!({
        "type": "tool", "action": "read_file", "path": path
    });
    match turn.submit_model(&proposal.to_string(), executor).unwrap() {
        TurnEvent::ApprovalRequired {
            operation_id,
            preview,
        } => {
            assert_eq!(preview.action, ProposedAction::ReadFile);
            assert_eq!(preview.relative_path, path);
            assert_eq!(preview.bytes, None);
            assert_eq!(preview.sha256, None);
            assert!(turn.pending_for_trusted_host().is_none());
            operation_id
        }
        TurnEvent::Finished(_) => panic!("a read must require independent host review"),
    }
}

#[test]
fn denying_existing_and_missing_reads_is_durable_without_metadata_inspection() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    let journal = audit.path().join("denied-reads.jsonl");
    fs::write(
        workspace.path().join("private.txt"),
        b"confidential contents",
    )
    .unwrap();
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        &journal,
        TEST_KEY,
    )
    .unwrap();
    let principal = context();

    let mut existing = AgentTurn::new(principal);
    let existing_id = pending_read(&mut existing, &executor, "private.txt");
    assert_eq!(
        existing.deny(&executor, OperationId::new()),
        Err(LoopError::WrongState)
    );
    existing.deny(&executor, existing_id).unwrap();
    assert_eq!(existing.status(), TurnStatus::AwaitingModel);

    let mut missing = AgentTurn::new(principal);
    let missing_id = pending_read(&mut missing, &executor, "missing.txt");
    missing.deny(&executor, missing_id).unwrap();
    assert_eq!(missing.status(), TurnStatus::AwaitingModel);
    assert!(!workspace.path().join("missing.txt").exists());
    assert_eq!(
        fs::read(workspace.path().join("private.txt")).unwrap(),
        b"confidential contents"
    );
    assert_eq!(executor.receipt(existing_id), None);
    assert_eq!(executor.receipt(missing_id), None);

    let raw = fs::read_to_string(&journal).unwrap();
    let records: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    // Scoped denial records now authenticate the exact principal, so legacy
    // unscoped `deny` must never substitute for a trustworthy native audit.
    assert!(records.iter().all(|row| {
        row["record"]["event"] == "deny_scoped"
            && row["record"]["context"]["task_id"] == principal.task_id.to_string()
            && row["record"]["context"]["agent_id"] == principal.agent_id.to_string()
            && row["record"]["context"]["room_id"] == principal.room_id.to_string()
    }));
    assert!(!raw.contains("private.txt"));
    assert!(!raw.contains("missing.txt"));
    assert!(!raw.contains("confidential contents"));
    drop(executor);

    // The missing path becomes an ordinary file AFTER the denial; its old
    // one-use operation ID still cannot be turned into an approval.
    fs::write(workspace.path().join("missing.txt"), b"created later").unwrap();
    let reopened = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        &journal,
        TEST_KEY,
    )
    .unwrap();
    let mut gate = PermissionGate::new();
    for (id, path) in [(existing_id, "private.txt"), (missing_id, "missing.txt")] {
        let prepared = reopened
            .prepare(principal, OperationRequest::Read { path: path.into() })
            .unwrap();
        gate.grant(prepared.scope().clone()).unwrap();
        assert!(matches!(
            reopened.approve(&gate, id, &prepared),
            Err(ToolError::OperationReplay)
        ));
    }
}

#[test]
fn cancelling_an_unprepared_read_writes_a_durable_deny_and_ends_turn() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    let journal = audit.path().join("cancel-read.jsonl");
    let principal = context();
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        &journal,
        TEST_KEY,
    )
    .unwrap();
    let mut turn = AgentTurn::new(principal);
    let id = pending_read(&mut turn, &executor, "missing.txt");
    turn.cancel(&executor).unwrap();
    assert_eq!(turn.status(), TurnStatus::Cancelled);
    assert_eq!(executor.receipt(id), None);
    assert!(!workspace.path().join("missing.txt").exists());
    assert_eq!(turn.cancel(&executor), Err(LoopError::WrongState));

    let raw = fs::read_to_string(&journal).unwrap();
    let rows: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["record"]["event"], "deny_scoped");
    assert_eq!(
        rows[0]["record"]["context"]["task_id"],
        principal.task_id.to_string()
    );
    drop(executor);

    fs::write(workspace.path().join("missing.txt"), b"later").unwrap();
    let reopened = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        &journal,
        TEST_KEY,
    )
    .unwrap();
    let prepared = reopened
        .prepare(
            principal,
            OperationRequest::Read {
                path: "missing.txt".into(),
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    assert!(matches!(
        reopened.approve(&gate, id, &prepared),
        Err(ToolError::OperationReplay)
    ));
}
