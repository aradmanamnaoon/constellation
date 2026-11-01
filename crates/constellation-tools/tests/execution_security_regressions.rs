//! Deterministic failure checks for the experimental, standalone effect runner.
//! No test pushes, contacts a server, or runs an arbitrary user command.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId, ToolAction};
use constellation_tools::{
    CancellationToken, GitAction, LocalExecutor, OperationContext, OperationOutcome,
    OperationRequest, ToolError, WriteMode,
};

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn executor(root: &Path) -> LocalExecutor {
    LocalExecutor::new(root, 1024 * 1024, 32 * 1024).unwrap()
}

fn create_request(path: &str, bytes: &[u8]) -> OperationRequest {
    OperationRequest::Write {
        path: path.into(),
        bytes: bytes.to_vec(),
        mode: WriteMode::CreateNew,
    }
}

fn process_request(executable: PathBuf, working_dir: PathBuf) -> OperationRequest {
    OperationRequest::Process {
        executable,
        working_dir,
        args: vec!["--list".into()],
        timeout: Duration::from_secs(3),
    }
}

#[test]
fn simultaneous_replays_cannot_execute_the_same_approval_twice() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = Arc::new(executor(workspace.path()));
    let context = context();
    let first = tool
        .prepare(context, create_request("only-once.txt", b"one"))
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(first.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &first).unwrap();
    let gate = Arc::new(gate);
    let barrier = Arc::new(Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let tool = Arc::clone(&tool);
        let gate = Arc::clone(&gate);
        let barrier = Arc::clone(&barrier);
        let approval = approval.clone();
        tasks.push(thread::spawn(move || {
            let operation = tool
                .prepare(context, create_request("only-once.txt", b"one"))
                .unwrap();
            barrier.wait();
            tool.execute(&gate, &approval, operation, &CancellationToken::new())
        }));
    }
    let mut success = 0;
    for task in tasks {
        match task.join().unwrap() {
            Ok((receipt, _)) => {
                assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
                success += 1;
            }
            Err(ToolError::OperationReplay) => {}
            Err(other) => panic!("unexpected concurrent result: {other:?}"),
        }
    }
    assert_eq!(success, 1);
    assert_eq!(
        fs::read(workspace.path().join("only-once.txt")).unwrap(),
        b"one"
    );
    assert_eq!(
        tool.receipt(approval.operation_id).unwrap().outcome,
        OperationOutcome::Succeeded
    );
}

#[test]
fn failed_create_new_does_not_replace_an_existing_file_or_authorize_replay() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = executor(workspace.path());
    let operation = tool
        .prepare(context(), create_request("data.txt", b"new"))
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(operation.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    fs::write(workspace.path().join("data.txt"), b"someone-else").unwrap();
    let (receipt, output) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert_eq!(
        receipt.failure_reason.as_deref(),
        Some("target file already exists")
    );
    assert!(output.is_none());
    assert_eq!(
        fs::read(workspace.path().join("data.txt")).unwrap(),
        b"someone-else"
    );
    let replay = tool
        .prepare(context(), create_request("data.txt", b"new"))
        .unwrap();
    // This is a different principal, so permission must reject independently.
    assert!(matches!(
        tool.execute(&gate, &approval, replay, &CancellationToken::new()),
        Err(ToolError::PermissionDenied)
    ));
}

#[test]
fn replacing_an_approved_file_with_an_external_hardlink_discloses_no_content() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let in_root = workspace.path().join("notes.txt");
    let secret = outside.path().join("private-outside.txt");
    fs::write(&in_root, "approved harmless text").unwrap();
    fs::write(&secret, "outside private text").unwrap();

    let tool = executor(workspace.path());
    let prepared = tool
        .prepare(
            context(),
            OperationRequest::Read {
                path: "notes.txt".into(),
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();

    // The user approved the original file, then a different local process
    // replaced the exact same path with a hardlink to outside the workspace.
    fs::remove_file(&in_root).unwrap();
    fs::hard_link(&secret, &in_root).unwrap();
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert_eq!(
        receipt.failure_reason.as_deref(),
        Some("files with multiple hardlinks are not eligible for workspace read approval")
    );
    assert!(output.is_none());
    assert_eq!(fs::read_to_string(&secret).unwrap(), "outside private text");
    assert_eq!(
        fs::read_to_string(&in_root).unwrap(),
        "outside private text"
    );
}

#[test]
fn executable_changed_after_approval_is_rejected_and_receipted() {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    fs::write(&executable, b"first executable bytes").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tool = executor(workspace.path());
    let context = context();
    let first = tool
        .prepare(context, process_request(executable.clone(), PathBuf::new()))
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(first.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &first).unwrap();
    fs::write(&executable, b"replaced executable bytes").unwrap();
    let refreshed = tool
        .prepare(context, process_request(executable.clone(), PathBuf::new()))
        .unwrap();
    assert_ne!(first.digest(), refreshed.digest());
    let (receipt, output) = tool
        .execute(&gate, &approval, first, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert_eq!(
        receipt.failure_reason.as_deref(),
        Some("invalid executable, working directory or arguments")
    );
    assert!(output.is_none());
    assert!(matches!(
        tool.execute(&gate, &approval, refreshed, &CancellationToken::new()),
        Err(ToolError::ApprovalMismatch)
    ));
    assert_eq!(tool.receipt(approval.operation_id), Some(receipt));
}

#[test]
fn invalid_executable_and_working_directory_fail_before_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let real = std::env::current_exe().unwrap();
    let relative = PathBuf::from(real.file_name().unwrap());
    for executable in [relative, workspace.path().to_path_buf()] {
        assert!(tool
            .prepare(ctx, process_request(executable, PathBuf::new()))
            .is_err());
    }
    for working_dir in [
        PathBuf::from("../escape"),
        PathBuf::from("./sub"),
        workspace.path().to_path_buf(),
    ] {
        assert!(matches!(
            tool.prepare(ctx, process_request(real.clone(), working_dir)),
            Err(ToolError::InvalidRelativePath)
        ));
    }
    let script = workspace.path().join(if cfg!(windows) {
        "script.cmd"
    } else {
        "script.sh"
    });
    fs::write(&script, b"echo NOT_EXECUTED").unwrap();
    assert!(tool
        .prepare(ctx, process_request(script, PathBuf::new()))
        .is_err());
}

#[test]
fn malformed_process_args_and_timeout_are_rejected_without_launch() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = executor(workspace.path());
    let exe = std::env::current_exe().unwrap();
    for (args, timeout) in [
        (vec!["bad\0argument".into()], Duration::from_secs(2)),
        (vec![], Duration::ZERO),
        (vec![], Duration::from_secs(121)),
        (vec!["a".repeat(32_769)], Duration::from_secs(2)),
        (vec!["a".into(); 129], Duration::from_secs(2)),
    ] {
        assert!(matches!(
            tool.prepare(
                context(),
                OperationRequest::Process {
                    executable: exe.clone(),
                    working_dir: PathBuf::new(),
                    args,
                    timeout,
                }
            ),
            Err(ToolError::InvalidCommand)
        ));
    }
}

#[test]
fn command_permission_binds_arguments_working_directory_and_timeout() {
    let workspace = tempfile::tempdir().unwrap();
    fs::create_dir(workspace.path().join("nested")).unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let executable = std::env::current_exe().unwrap();
    let prepare = |working_dir: PathBuf, args: Vec<String>, timeout: Duration| {
        tool.prepare(
            ctx,
            OperationRequest::Process {
                executable: executable.clone(),
                working_dir,
                args,
                timeout,
            },
        )
        .unwrap()
    };
    let approved = prepare(
        PathBuf::new(),
        vec!["--list".into()],
        Duration::from_secs(2),
    );
    let changed_args = prepare(
        PathBuf::new(),
        vec!["--help".into()],
        Duration::from_secs(2),
    );
    let changed_dir = prepare(
        "nested".into(),
        vec!["--list".into()],
        Duration::from_secs(2),
    );
    let changed_timeout = prepare(
        PathBuf::new(),
        vec!["--list".into()],
        Duration::from_secs(3),
    );
    let mut gate = PermissionGate::new();
    gate.grant(approved.scope().clone()).unwrap();
    for operation in [&changed_args, &changed_dir, &changed_timeout] {
        assert_ne!(approved.scope().resource, operation.scope().resource);
        assert!(matches!(
            tool.approve(&gate, OperationId::new(), operation),
            Err(ToolError::PermissionDenied)
        ));
    }
}

#[cfg(any(unix, windows))]
#[test]
fn approved_process_refuses_working_directory_replaced_with_symlink() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let safe = workspace.path().join("safe");
    fs::create_dir(&safe).unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let request = || process_request(std::env::current_exe().unwrap(), "safe".into());
    let prepared = tool.prepare(ctx, request()).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    fs::remove_dir(&safe).unwrap();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(outside.path(), &safe);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(outside.path(), &safe);
    if made.is_err() {
        // Symlink privileges can be unavailable on Windows.
        return;
    }
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert!(output.is_none());
    assert!(!outside.path().join("created.txt").exists());
}

#[test]
fn approved_process_refuses_ordinary_directory_replacement_and_consumes_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let safe = workspace.path().join("safe");
    let displaced = workspace.path().join("displaced");
    fs::create_dir(&safe).unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let request = || process_request(std::env::current_exe().unwrap(), "safe".into());
    let prepared = tool.prepare(ctx, request()).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();

    // No symlink is involved: a normal directory has been replaced with a
    // second, different inode/file ID at exactly the same approved pathname.
    fs::rename(&safe, &displaced).unwrap();
    fs::create_dir(&safe).unwrap();
    let reapproved_path = tool.prepare(ctx, request()).unwrap();
    assert_ne!(prepared.digest(), reapproved_path.digest());
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert!(output.is_none());
    assert!(matches!(
        tool.execute(&gate, &approval, reapproved_path, &CancellationToken::new()),
        Err(ToolError::ApprovalMismatch)
    ));
    fs::remove_dir(&safe).unwrap();
    fs::rename(&displaced, &safe).unwrap();
    // Recreating the originally approved inode still does not revive a
    // consumed approval. Identical path and digest cannot bypass replay.
    let replay = tool.prepare(ctx, request()).unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, replay, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
}

#[test]
fn git_status_refuses_replaced_repository_directory_before_spawning() {
    let workspace = tempfile::tempdir().unwrap();
    let repo = workspace.path().join("repository");
    fs::create_dir(&repo).unwrap();
    fs::create_dir(repo.join(".git")).unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let request = || OperationRequest::Git {
        executable: std::env::current_exe().unwrap(),
        repository: "repository".into(),
        action: GitAction::Status,
        timeout: Duration::from_secs(2),
    };
    let prepared = tool.prepare(ctx, request()).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    fs::rename(&repo, workspace.path().join("former")).unwrap();
    fs::create_dir(&repo).unwrap();
    fs::create_dir(repo.join(".git")).unwrap();
    let replacement = tool.prepare(ctx, request()).unwrap();
    assert_ne!(prepared.digest(), replacement.digest());
    let (receipt, result) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert!(result.is_none());
}

#[test]
fn cancelled_process_approval_never_spawns_and_cannot_be_replayed() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let request = || process_request(std::env::current_exe().unwrap(), PathBuf::new());
    let prepared = tool.prepare(ctx, request()).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &cancelled)
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(output.is_none());
    let replay = tool.prepare(ctx, request()).unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, replay, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
}

#[test]
fn gitpush_scope_binds_repository_remote_and_branch_without_executing_push() {
    let workspace = tempfile::tempdir().unwrap();
    for directory in ["first", "second"] {
        let repo = workspace.path().join(directory);
        fs::create_dir(&repo).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    }
    let tool = executor(workspace.path());
    let ctx = context();
    let exe = std::env::current_exe().unwrap();
    let prepare = |repo: &str, remote_url: &str, branch: &str| {
        tool.prepare(
            ctx,
            OperationRequest::Git {
                executable: exe.clone(),
                repository: PathBuf::from(repo),
                action: GitAction::Push {
                    remote_url: remote_url.into(),
                    branch: branch.into(),
                },
                timeout: Duration::from_secs(2),
            },
        )
        .unwrap()
    };
    let allowed = prepare("first", "https://example.test/owner/repo.git", "main");
    let other_remote = prepare("first", "https://example.test/owner/other.git", "main");
    let other_branch = prepare("first", "https://example.test/owner/repo.git", "release");
    let other_repo = prepare("second", "https://example.test/owner/repo.git", "main");
    assert_eq!(allowed.scope().action, ToolAction::GitPush);
    for different in [&other_remote, &other_branch, &other_repo] {
        assert_ne!(allowed.scope().resource, different.scope().resource);
    }
    let mut gate = PermissionGate::new();
    gate.grant(allowed.scope().clone()).unwrap();
    assert!(tool.approve(&gate, OperationId::new(), &allowed).is_ok());
    for different in [&other_remote, &other_branch, &other_repo] {
        assert!(matches!(
            tool.approve(&gate, OperationId::new(), different),
            Err(ToolError::PermissionDenied)
        ));
    }
    assert!(matches!(
        tool.prepare(
            ctx,
            OperationRequest::Git {
                executable: exe,
                repository: "first".into(),
                action: GitAction::Push {
                    remote_url: "ext::untrusted".into(),
                    branch: "main".into(),
                },
                timeout: Duration::from_secs(2),
            }
        ),
        Err(ToolError::InvalidCommand)
    ));
}

#[test]
fn cancelled_approval_is_consumed_without_side_effects() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = executor(workspace.path());
    let ctx = context();
    let operation = tool
        .prepare(ctx, create_request("aborted.txt", b"never"))
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(operation.scope().clone()).unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (receipt, _) = tool
        .execute(&gate, &approval, operation, &cancelled)
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(!workspace.path().join("aborted.txt").exists());
    assert_eq!(tool.receipt(approval.operation_id), Some(receipt));
    let replay = tool
        .prepare(ctx, create_request("aborted.txt", b"never"))
        .unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, replay, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
}

#[cfg(any(unix, windows))]
#[test]
fn symlink_in_workspace_path_is_denied_before_write_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let linked = workspace.path().join("link");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(outside.path(), &linked);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(outside.path(), &linked);
    if made.is_err() {
        // Windows symlink privilege can be disabled for a normal developer user.
        return;
    }
    let tool = executor(workspace.path());
    let result = tool.prepare(context(), create_request("link/outside.txt", b"blocked"));
    assert!(matches!(
        result,
        Err(ToolError::SymlinkDenied) | Err(ToolError::ScopeEscape)
    ));
    assert!(!outside.path().join("outside.txt").exists());
}
