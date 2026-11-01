//! An untrusted model can propose a read, but a rejected path must be
//! journaled without observing whether the target exists or what it contains.
use std::fs;

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    LocalExecutor, OperationContext, OperationRequest, ToolError, WriteMode,
};

fn principal() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

#[test]
fn deferred_denial_is_authenticated_and_one_use_without_probing_read_target() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    let journal_path = audit.path().join("read-denials.jsonl");
    let secret = workspace.path().join("existing-secret.txt");
    fs::write(&secret, b"Never read for a denial").unwrap();

    let context = principal();
    let nonexistent_id = OperationId::new();
    let existing_id = OperationId::new();
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16 * 1024,
        16 * 1024,
        &journal_path,
        [0x7f; 32],
    )
    .unwrap();
    assert!(matches!(
        executor.deny_unprepared_read(OperationId::new(), context, "../outside"),
        Err(ToolError::InvalidRelativePath)
    ));
    assert!(matches!(
        executor.deny_unprepared_read(OperationId::new(), context, "bidi-\u{202e}-path"),
        Err(ToolError::InvalidRelativePath)
    ));

    // Equal success for an absent target and a present private target proves
    // neither lookup is needed merely to journal an independent user denial.
    executor
        .deny_unprepared_read(nonexistent_id, context, "missing-file.txt")
        .unwrap();
    executor
        .deny_unprepared_read(existing_id, context, "existing-secret.txt")
        .unwrap();
    assert_eq!(fs::read(&secret).unwrap(), b"Never read for a denial");
    assert!(!workspace.path().join("missing-file.txt").exists());
    assert!(matches!(
        executor.deny_unprepared_read(nonexistent_id, context, "missing-file.txt"),
        Err(ToolError::OperationReplay)
    ));

    let journal_text = fs::read_to_string(&journal_path).unwrap();
    assert_eq!(journal_text.matches("\"event\":\"deny_scoped\"").count(), 2);
    assert!(!journal_text.contains("existing-secret.txt"));
    assert!(!journal_text.contains("Never read for a denial"));

    let unrelated_prepared = executor
        .prepare(
            context,
            OperationRequest::Write {
                path: "new-ordinary-file.txt".into(),
                bytes: b"safe fixture".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(unrelated_prepared.scope().clone()).unwrap();
    assert!(matches!(
        executor.approve(&gate, existing_id, &unrelated_prepared),
        Err(ToolError::OperationReplay)
    ));
    drop(executor);

    let reopened = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16 * 1024,
        16 * 1024,
        &journal_path,
        [0x7f; 32],
    )
    .unwrap();
    assert!(matches!(
        reopened.approve(&gate, nonexistent_id, &unrelated_prepared),
        Err(ToolError::OperationReplay)
    ));
    assert!(matches!(
        reopened.approve(&gate, existing_id, &unrelated_prepared),
        Err(ToolError::OperationReplay)
    ));
    drop(reopened);

    // Authentication fails closed when the persisted denial is modified.
    let altered = journal_text.replacen("\"event\":\"deny_scoped\"", "\"event\":\"start\"", 1);
    assert_ne!(altered, journal_text);
    fs::write(&journal_path, altered).unwrap();
    assert!(LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16 * 1024,
        16 * 1024,
        &journal_path,
        [0x7f; 32],
    )
    .is_err());
}
