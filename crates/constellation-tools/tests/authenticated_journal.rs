//! No network or process dispatch: check authenticated persistence and consent.

use std::{fs, path::Path};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, LocalExecutor, OperationApproval, OperationContext, OperationOutcome,
    OperationRequest, ToolError, WriteMode,
};

const KEY: [u8; 32] = [0x71; 32];

fn open(root: &Path, log: &Path, key: [u8; 32]) -> Result<LocalExecutor, ToolError> {
    LocalExecutor::with_authenticated_journal(root, 4096, 4096, log, key)
}

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn request(name: &str) -> OperationRequest {
    OperationRequest::Write {
        path: name.into(),
        bytes: b"user approved bytes".to_vec(),
        mode: WriteMode::CreateNew,
    }
}

#[test]
fn approved_receipt_is_authenticated_and_replay_is_denied_after_restart() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("auth.jsonl");
    let principal = context();
    let operation_id = OperationId::new();
    let receipt = {
        let tool = open(workspace.path(), &log, KEY).unwrap();
        let prepared = tool.prepare(principal, request("created.txt")).unwrap();
        let mut gate = PermissionGate::new();
        gate.grant(prepared.scope().clone()).unwrap();
        let approval = tool.approve(&gate, operation_id, &prepared).unwrap();
        let (receipt, _) = tool
            .execute(&gate, &approval, prepared, &CancellationToken::new())
            .unwrap();
        assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
        receipt
    };
    let tool = open(workspace.path(), &log, KEY).unwrap();
    assert_eq!(tool.receipt(operation_id), Some(receipt));
    let prepared = tool.prepare(principal, request("created.txt")).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    assert!(matches!(
        tool.approve(&gate, operation_id, &prepared),
        Err(ToolError::OperationReplay)
    ));
    assert_eq!(
        fs::read(workspace.path().join("created.txt")).unwrap(),
        b"user approved bytes"
    );
    drop(tool);
    assert!(matches!(
        open(workspace.path(), &log, [0x72; 32]),
        Err(ToolError::InvalidJournal)
    ));
    // No fallback parsing an authenticated journal as unsealed events.
    assert!(matches!(
        LocalExecutor::with_journal(workspace.path(), 4096, 4096, &log),
        Err(ToolError::InvalidJournal)
    ));
}

#[test]
fn denial_survives_restart_and_cannot_be_forged_into_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("denials.jsonl");
    let principal = context();
    let operation_id = OperationId::new();
    let digest = {
        let tool = open(workspace.path(), &log, KEY).unwrap();
        let prepared = tool.prepare(principal, request("denied.txt")).unwrap();
        let digest = prepared.digest().to_owned();
        tool.deny(operation_id, &prepared).unwrap();
        assert!(matches!(
            tool.deny(operation_id, &prepared),
            Err(ToolError::OperationReplay)
        ));
        digest
    };
    let tool = open(workspace.path(), &log, KEY).unwrap();
    let prepared = tool.prepare(principal, request("denied.txt")).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    assert!(matches!(
        tool.approve(&gate, operation_id, &prepared),
        Err(ToolError::OperationReplay)
    ));
    let forged = OperationApproval {
        operation_id,
        digest,
    };
    assert!(matches!(
        tool.execute(&gate, &forged, prepared, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
    assert!(tool.receipt(operation_id).is_none());
    assert!(!workspace.path().join("denied.txt").exists());
}

#[test]
fn valid_json_tampering_and_missing_middle_record_fail_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("tampering.jsonl");
    {
        let tool = open(workspace.path(), &log, KEY).unwrap();
        let prepared = tool.prepare(context(), request("created.txt")).unwrap();
        let mut gate = PermissionGate::new();
        gate.grant(prepared.scope().clone()).unwrap();
        let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
        tool.execute(&gate, &approval, prepared, &CancellationToken::new())
            .unwrap();
    }
    let original = fs::read_to_string(&log).unwrap();
    assert!(original.contains("Succeeded"));
    let changed = original.replace("Succeeded", "Failed");
    assert_ne!(changed, original);
    fs::write(&log, changed).unwrap();
    assert!(matches!(
        open(workspace.path(), &log, KEY),
        Err(ToolError::InvalidJournal)
    ));

    let lines: Vec<_> = original.lines().collect();
    assert_eq!(lines.len(), 3);
    fs::write(&log, format!("{}\n{}\n", lines[0], lines[2])).unwrap();
    assert!(matches!(
        open(workspace.path(), &log, KEY),
        Err(ToolError::InvalidJournal)
    ));
    fs::write(&log, original).unwrap();
    assert!(open(workspace.path(), &log, KEY).is_ok());
}

#[test]
fn mismatched_format_and_wrong_location_are_refused() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("legacy.jsonl");
    let legacy = LocalExecutor::with_journal(workspace.path(), 4096, 4096, &log).unwrap();
    let prepared = legacy.prepare(context(), request("ignored.txt")).unwrap();
    legacy.deny(OperationId::new(), &prepared).unwrap();
    drop(legacy);
    assert!(matches!(
        open(workspace.path(), &log, KEY),
        Err(ToolError::InvalidJournal)
    ));
    assert!(matches!(
        open(
            workspace.path(),
            &workspace.path().join("unsafe.jsonl"),
            KEY
        ),
        Err(ToolError::InvalidJournal)
    ));
}

#[test]
fn copied_authenticated_log_cannot_be_transplanted_to_another_journal_path() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let first = private.path().join("original.jsonl");
    let second = private.path().join("replacement.jsonl");
    {
        let tool = open(workspace.path(), &first, KEY).unwrap();
        let prepared = tool.prepare(context(), request("not_created.txt")).unwrap();
        tool.deny(OperationId::new(), &prepared).unwrap();
    }
    fs::copy(&first, &second).unwrap();
    assert!(matches!(
        open(workspace.path(), &second, KEY),
        Err(ToolError::InvalidJournal)
    ));
    assert!(open(workspace.path(), &first, KEY).is_ok());
}
