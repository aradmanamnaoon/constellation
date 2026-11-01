//! Read-only recovery lookup over a fully authenticated journal.
use std::{fs, path::Path};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, JournalOperationState, LocalExecutor, OperationContext, OperationOutcome,
    OperationRequest, ToolError, WriteMode,
};

const KEY: [u8; 32] = [0xa5; 32];

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn open(root: &Path, log: &Path) -> LocalExecutor {
    LocalExecutor::with_authenticated_journal(root, 4096, 4096, log, KEY).unwrap()
}

fn write(bytes: &[u8], path: &str) -> OperationRequest {
    OperationRequest::Write {
        path: path.into(),
        bytes: bytes.to_vec(),
        mode: WriteMode::CreateNew,
    }
}

fn lookup(
    root: &Path,
    log: &Path,
    id: OperationId,
    digest: &str,
    context: OperationContext,
) -> constellation_tools::Result<JournalOperationState> {
    LocalExecutor::lookup_authenticated_journal(root, log, KEY, id, digest, context, None)
}

#[test]
fn lookup_live_writer_and_reopened_journal_identify_all_states_without_content() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("receipts.jsonl");
    let principal = context();
    let tool = open(workspace.path(), &log);
    assert_eq!(
        tool.journal_state(OperationId::new()).unwrap(),
        JournalOperationState::Unknown
    );
    let mut gate = PermissionGate::new();
    let approved_id = OperationId::new();
    let approved = tool
        .prepare(principal, write(b"SECRET_APPROVED_BYTES", "pending.txt"))
        .unwrap();
    gate.grant(approved.scope().clone()).unwrap();
    tool.approve(&gate, approved_id, &approved).unwrap();
    let approved_digest = approved.digest().to_string();
    assert!(matches!(
        tool.journal_state_bound(approved_id, &approved_digest, principal)
            .unwrap(),
        JournalOperationState::ApprovedUnstarted { .. }
    ));

    let denied_id = OperationId::new();
    let denied = tool
        .prepare(principal, write(b"PRIVATE_DENIED", "deny.txt"))
        .unwrap();
    let denied_digest = denied.digest().to_string();
    tool.deny(denied_id, &denied).unwrap();
    assert!(matches!(
        lookup(workspace.path(), &log, denied_id, &denied_digest, principal).unwrap(),
        JournalOperationState::Denied { .. }
    ));

    let success_id = OperationId::new();
    let success = tool
        .prepare(principal, write(b"VERY_PRIVATE_SECRET", "success.txt"))
        .unwrap();
    gate.grant(success.scope().clone()).unwrap();
    let success_digest = success.digest().to_string();
    let approval = tool.approve(&gate, success_id, &success).unwrap();
    tool.execute(&gate, &approval, success, &CancellationToken::new())
        .unwrap();

    let cancelled_id = OperationId::new();
    let cancelled = tool
        .prepare(principal, write(b"PRIVATE_CANCELLED", "cancelled.txt"))
        .unwrap();
    gate.grant(cancelled.scope().clone()).unwrap();
    let cancelled_digest = cancelled.digest().to_string();
    let approval = tool.approve(&gate, cancelled_id, &cancelled).unwrap();
    let token = CancellationToken::new();
    token.cancel();
    tool.execute(&gate, &approval, cancelled, &token).unwrap();

    let head = tool.authenticated_journal_head().unwrap();
    assert_eq!(head.record_count(), 8);
    assert_eq!(head.last_tag().len(), 64);
    let status = LocalExecutor::lookup_authenticated_journal(
        workspace.path(),
        &log,
        KEY,
        success_id,
        &success_digest,
        principal,
        Some(&head),
    )
    .unwrap();
    assert_eq!(
        status,
        JournalOperationState::Completed {
            digest: success_digest.clone(),
            context: principal,
            outcome: OperationOutcome::Succeeded
        }
    );
    let printed = format!("{status:?}");
    assert!(!printed.contains("PRIVATE"));
    assert!(!printed.contains("success.txt"));
    assert!(!printed.contains("failure_reason"));
    assert!(matches!(
        lookup(
            workspace.path(),
            &log,
            cancelled_id,
            &cancelled_digest,
            principal
        )
        .unwrap(),
        JournalOperationState::Completed {
            outcome: OperationOutcome::Cancelled,
            ..
        }
    ));
    assert!(matches!(
        tool.journal_state_bound(success_id, &approved_digest, principal),
        Err(ToolError::ApprovalMismatch)
    ));
    let wrong_principal = context();
    assert!(matches!(
        lookup(
            workspace.path(),
            &log,
            success_id,
            &success_digest,
            wrong_principal
        ),
        Err(ToolError::ApprovalMismatch)
    ));
    let before = fs::read(&log).unwrap();
    drop(tool);
    let reopened = open(workspace.path(), &log);
    assert_eq!(reopened.journal_state(success_id).unwrap(), status);
    assert_eq!(fs::read(&log).unwrap(), before);
    let prepared = reopened
        .prepare(principal, write(b"VERY_PRIVATE_SECRET", "again.txt"))
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    assert!(matches!(
        reopened.approve(&gate, success_id, &prepared),
        Err(ToolError::OperationReplay)
    ));
}

#[test]
fn a_trusted_chain_head_detects_complete_suffix_removal() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("truncation.jsonl");
    let principal = context();
    let id = OperationId::new();
    let (digest, head) = {
        let tool = open(workspace.path(), &log);
        let prepared = tool.prepare(principal, write(b"done", "done.txt")).unwrap();
        let digest = prepared.digest().to_string();
        let mut gate = PermissionGate::new();
        gate.grant(prepared.scope().clone()).unwrap();
        let approval = tool.approve(&gate, id, &prepared).unwrap();
        tool.execute(&gate, &approval, prepared, &CancellationToken::new())
            .unwrap();
        (digest, tool.authenticated_journal_head().unwrap())
    };
    let bytes = fs::read_to_string(&log).unwrap();
    let mut lines = bytes.lines();
    let first = lines.next().unwrap();
    let second = lines.next().unwrap();
    fs::write(&log, format!("{first}\n{second}\n")).unwrap();
    assert!(matches!(
        lookup(workspace.path(), &log, id, &digest, principal).unwrap(),
        JournalOperationState::StartedUnfinished { .. }
    ));
    assert!(matches!(
        LocalExecutor::lookup_authenticated_journal(
            workspace.path(),
            &log,
            KEY,
            id,
            &digest,
            principal,
            Some(&head)
        ),
        Err(ToolError::InvalidJournal)
    ));
}

#[test]
fn forged_mac_wrong_key_partial_tail_and_duplicate_events_fail_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("tamper.jsonl");
    let principal = context();
    let id = OperationId::new();
    let digest = {
        let tool = open(workspace.path(), &log);
        let prepared = tool
            .prepare(principal, write(b"never created", "denied.txt"))
            .unwrap();
        let digest = prepared.digest().to_string();
        tool.deny(id, &prepared).unwrap();
        digest
    };
    let original = fs::read_to_string(&log).unwrap();
    assert!(matches!(
        LocalExecutor::lookup_authenticated_journal(
            workspace.path(),
            &log,
            [0xa6; 32],
            id,
            &digest,
            principal,
            None,
        ),
        Err(ToolError::InvalidJournal)
    ));
    let forged = original.replace("\"denied\"", "\"succeeded\"");
    // Replace a real signed field while leaving its HMAC unchanged.
    let forged = if forged == original {
        original.replace("deny_scoped", "approve_scoped")
    } else {
        forged
    };
    fs::write(&log, forged).unwrap();
    assert!(matches!(
        lookup(workspace.path(), &log, id, &digest, principal),
        Err(ToolError::InvalidJournal)
    ));
    fs::write(&log, original.trim_end_matches('\n')).unwrap();
    assert!(matches!(
        lookup(workspace.path(), &log, id, &digest, principal),
        Err(ToolError::InvalidJournal)
    ));
    fs::write(&log, format!("{original}{original}")).unwrap();
    assert!(matches!(
        lookup(workspace.path(), &log, id, &digest, principal),
        Err(ToolError::InvalidJournal)
    ));
    fs::write(&log, &original).unwrap();
    assert!(matches!(
        lookup(workspace.path(), &log, id, &digest, principal).unwrap(),
        JournalOperationState::Denied { .. }
    ));
}

#[test]
fn missing_journal_never_creates_a_file_or_writer_lock_during_lookup() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let log = private.path().join("missing.jsonl");
    assert!(matches!(
        lookup(
            workspace.path(),
            &log,
            OperationId::new(),
            &"f".repeat(64),
            context()
        ),
        Err(ToolError::InvalidJournal)
    ));
    assert!(!log.exists());
    assert!(!private.path().join("missing.jsonl.lock").exists());
}
