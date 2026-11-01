//! Approval-to-dispatch filesystem identity regressions. No network or shell.

use std::{fs, path::Path};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, LocalExecutor, OperationContext, OperationOutcome, OperationRequest,
    WriteMode,
};
use sha2::{Digest, Sha256};

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn executor(workspace: &Path) -> LocalExecutor {
    LocalExecutor::new(workspace, 1_048_576, 32_768).unwrap()
}

fn dispatch(
    tool: &LocalExecutor,
    request: OperationRequest,
    before: impl FnOnce(),
) -> (OperationOutcome, Option<String>) {
    let prepared = tool.prepare(context(), request).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    before();
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert!(output.is_none(), "redirected effect must have no result");
    assert_eq!(tool.receipt(approval.operation_id), Some(receipt.clone()));
    (receipt.outcome, receipt.failure_reason)
}

#[test]
fn approved_create_refuses_replaced_ordinary_directory_even_without_symlinks() {
    let workspace = tempfile::tempdir().unwrap();
    let original = workspace.path().join("drafts");
    let displaced = workspace.path().join("old-drafts");
    fs::create_dir(&original).unwrap();
    let tool = executor(workspace.path());
    let (status, reason) = dispatch(
        &tool,
        OperationRequest::Write {
            path: "drafts/paper.md".into(),
            bytes: b"approved text".to_vec(),
            mode: WriteMode::CreateNew,
        },
        || {
            fs::rename(&original, &displaced).unwrap();
            fs::create_dir(&original).unwrap();
        },
    );
    assert_eq!(status, OperationOutcome::Failed);
    assert_eq!(
        reason.as_deref(),
        Some("path escapes the selected workspace")
    );
    assert!(!original.join("paper.md").exists());
    assert!(!displaced.join("paper.md").exists());
}

#[test]
fn approved_replacement_refuses_replaced_ordinary_directory_with_identical_contents() {
    let workspace = tempfile::tempdir().unwrap();
    let original = workspace.path().join("drafts");
    let displaced = workspace.path().join("old-drafts");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("paper.md"), b"old content").unwrap();
    let tool = executor(workspace.path());
    let (status, reason) = dispatch(
        &tool,
        OperationRequest::Write {
            path: "drafts/paper.md".into(),
            bytes: b"new content".to_vec(),
            mode: WriteMode::ReplaceIfSha256(format!("{:x}", Sha256::digest(b"old content"))),
        },
        || {
            fs::rename(&original, &displaced).unwrap();
            fs::create_dir(&original).unwrap();
            // Exactly matching text alone must not authorize another directory.
            fs::write(original.join("paper.md"), b"old content").unwrap();
        },
    );
    assert_eq!(status, OperationOutcome::Failed);
    assert_eq!(
        reason.as_deref(),
        Some("path escapes the selected workspace")
    );
    assert_eq!(fs::read(original.join("paper.md")).unwrap(), b"old content");
    assert_eq!(
        fs::read(displaced.join("paper.md")).unwrap(),
        b"old content"
    );
}

#[test]
fn approved_create_cannot_clobber_an_external_hardlink_placed_at_target() {
    let workspace = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let private = external.path().join("external.txt");
    fs::write(&private, b"private").unwrap();
    let tool = executor(workspace.path());
    let (status, reason) = dispatch(
        &tool,
        OperationRequest::Write {
            path: "new.txt".into(),
            bytes: b"approved replacement".to_vec(),
            mode: WriteMode::CreateNew,
        },
        || fs::hard_link(&private, workspace.path().join("new.txt")).unwrap(),
    );
    assert_eq!(status, OperationOutcome::Failed);
    assert_eq!(reason.as_deref(), Some("target file already exists"));
    assert_eq!(fs::read(private).unwrap(), b"private");
    assert_eq!(
        fs::read(workspace.path().join("new.txt")).unwrap(),
        b"private"
    );
    assert!(
        fs::read_dir(workspace.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("constellation-")),
        "failed publishing must remove private staging files"
    );
}

#[cfg(any(unix, windows))]
#[test]
fn approved_create_refuses_swapped_parent_symlink() {
    let workspace = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let parent = workspace.path().join("notes");
    fs::create_dir(&parent).unwrap();
    let tool = executor(workspace.path());
    let prepared = tool
        .prepare(
            context(),
            OperationRequest::Write {
                path: "notes/paper.md".into(),
                bytes: b"approved".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    fs::rename(&parent, workspace.path().join("original-notes")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(external.path(), &parent).unwrap();
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(external.path(), &parent).is_err() {
            // Normal Windows users without Developer Mode may lack symlink creation.
            return;
        }
    }
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert!(output.is_none());
    assert!(!external.path().join("paper.md").exists());
    assert!(!workspace.path().join("original-notes/paper.md").exists());
}
