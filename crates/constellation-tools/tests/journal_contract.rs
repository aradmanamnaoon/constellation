//! Durable approval/receipt regression checks without outbound effects.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, LocalExecutor, OperationContext, OperationOutcome, OperationRequest,
    ToolError, WriteMode,
};

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn durable(root: &Path, log: &Path) -> LocalExecutor {
    LocalExecutor::with_journal(root, 16384, 16384, log).unwrap()
}

fn write_request(path: &str) -> OperationRequest {
    OperationRequest::Write {
        path: path.into(),
        bytes: b"approved data".to_vec(),
        mode: WriteMode::CreateNew,
    }
}

#[test]
fn completed_receipt_survives_restart_and_reused_id_is_denied() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("attempts.jsonl");
    let context = context();
    let id = OperationId::new();
    let receipt = {
        let tool = durable(workspace.path(), &log);
        let prepared = tool.prepare(context, write_request("created.txt")).unwrap();
        let mut gate = PermissionGate::new();
        gate.grant(prepared.scope().clone()).unwrap();
        let approval = tool.approve(&gate, id, &prepared).unwrap();
        let (receipt, _) = tool
            .execute(&gate, &approval, prepared, &CancellationToken::new())
            .unwrap();
        assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
        assert_eq!(tool.receipt(id), Some(receipt.clone()));
        receipt
    };
    assert_eq!(
        fs::read(workspace.path().join("created.txt")).unwrap(),
        b"approved data"
    );
    let tool = durable(workspace.path(), &log);
    assert_eq!(tool.receipt(id), Some(receipt));
    let prepared = tool.prepare(context, write_request("created.txt")).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    assert!(matches!(
        tool.approve(&gate, id, &prepared),
        Err(ToolError::OperationReplay)
    ));
}

#[test]
fn interrupted_attempt_cannot_be_dispatched_after_restart() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("attempts.jsonl");
    let tool = durable(workspace.path(), &log);
    let prepared = tool
        .prepare(context(), write_request("unwritten.txt"))
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let digest = prepared.digest().to_owned();
    drop(tool);

    // Reproduce a crash after the start record was synced but before receipt.
    let event = serde_json::json!({
        "event": "start", "id": approval.operation_id, "digest": digest
    });
    let mut file = OpenOptions::new().append(true).open(&log).unwrap();
    writeln!(file, "{event}").unwrap();
    file.sync_all().unwrap();
    drop(file);

    let tool = durable(workspace.path(), &log);
    assert!(tool.receipt(approval.operation_id).is_none());
    assert!(matches!(
        tool.execute(&gate, &approval, prepared, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
    assert!(!workspace.path().join("unwritten.txt").exists());
}

#[test]
fn workspace_journal_concurrent_owner_and_corrupt_log_fail_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("attempts.jsonl");
    assert!(matches!(
        LocalExecutor::with_journal(workspace.path(), 1024, 1024, workspace.path().join("log")),
        Err(ToolError::InvalidJournal)
    ));
    let tool = durable(workspace.path(), &log);
    assert!(matches!(
        LocalExecutor::with_journal(workspace.path(), 1024, 1024, &log),
        Err(ToolError::InvalidJournal)
    ));
    drop(tool);
    fs::write(&log, b"{malformed\n").unwrap();
    assert!(matches!(
        LocalExecutor::with_journal(workspace.path(), 1024, 1024, &log),
        Err(ToolError::InvalidJournal)
    ));
    assert!(!workspace.path().join("log").exists());
}

#[test]
fn cancelled_attempt_receipt_is_persisted() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("attempts.jsonl");
    let id = OperationId::new();
    {
        let tool = durable(workspace.path(), &log);
        let prepared = tool
            .prepare(context(), write_request("cancelled.txt"))
            .unwrap();
        let mut gate = PermissionGate::new();
        gate.grant(prepared.scope().clone()).unwrap();
        let approval = tool.approve(&gate, id, &prepared).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        let (receipt, _) = tool.execute(&gate, &approval, prepared, &token).unwrap();
        assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    }
    assert_eq!(
        durable(workspace.path(), &log).receipt(id).unwrap().outcome,
        OperationOutcome::Cancelled
    );
    assert!(!workspace.path().join("cancelled.txt").exists());
}
