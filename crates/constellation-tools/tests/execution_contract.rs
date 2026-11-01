use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId, ToolAction};
use constellation_tools::{
    CancellationToken, ExecutionOutput, GitAction, LocalExecutor, OperationContext,
    OperationDetails, OperationOutcome, OperationRequest, PreparedOperation, ToolError, WriteMode,
};

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn runner(path: &Path) -> LocalExecutor {
    LocalExecutor::new(path, 16384, 16384).unwrap()
}

fn grant(gate: &mut PermissionGate, prepared: &PreparedOperation) {
    gate.grant(prepared.scope().clone()).unwrap();
}

#[test]
fn write_requires_both_permission_and_unique_operation_approval() {
    let dir = tempfile::tempdir().unwrap();
    let tool = runner(dir.path());
    let context = context();
    let request = || OperationRequest::Write {
        path: "notes.txt".into(),
        bytes: b"approved notes".to_vec(),
        mode: WriteMode::CreateNew,
    };
    let prepared = tool.prepare(context, request()).unwrap();
    let preview = prepared.preview();
    assert_eq!(preview.digest, prepared.digest());
    let OperationDetails::Write { bytes, sha256, .. } = preview.details else {
        panic!("write preview")
    };
    assert_eq!(bytes, 14);
    assert_eq!(sha256.len(), 64);
    let mut gate = PermissionGate::new();
    let operation_id = OperationId::new();
    assert!(matches!(
        tool.approve(&gate, operation_id, &prepared),
        Err(ToolError::PermissionDenied)
    ));
    assert!(!dir.path().join("notes.txt").exists());
    grant(&mut gate, &prepared);
    let approval = tool.approve(&gate, operation_id, &prepared).unwrap();
    let receipt_digest = approval.digest.clone();
    assert!(matches!(
        tool.approve(&gate, operation_id, &prepared),
        Err(ToolError::OperationReplay)
    ));
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert_eq!(receipt.digest, receipt_digest);
    assert_eq!(tool.receipt(operation_id), Some(receipt));
    assert_eq!(
        fs::read(dir.path().join("notes.txt")).unwrap(),
        b"approved notes"
    );
    assert!(matches!(output, Some(ExecutionOutput::Write(_))));
    let replay = tool.prepare(context, request()).unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, replay, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));

    let modified = tool
        .prepare(
            context,
            OperationRequest::Write {
                path: "notes.txt".into(),
                bytes: b"malicious modification".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, modified, &CancellationToken::new()),
        Err(ToolError::ApprovalMismatch)
    ));
    assert_eq!(
        fs::read(dir.path().join("notes.txt")).unwrap(),
        b"approved notes"
    );
}

#[test]
fn scoped_replacement_checks_original_hash_and_preserves_file_on_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, b"before").unwrap();
    let tool = runner(dir.path());
    let context = context();
    let mut gate = PermissionGate::new();
    let original_hash = format!("{:x}", sha2::Sha256::digest(b"before"));
    let prepare = || {
        tool.prepare(
            context,
            OperationRequest::Write {
                path: "notes.txt".into(),
                bytes: b"after".to_vec(),
                mode: WriteMode::ReplaceIfSha256(original_hash.clone()),
            },
        )
        .unwrap()
    };
    use sha2::Digest;
    let operation = prepare();
    grant(&mut gate, &operation);
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    fs::write(&path, b"someone else edited").unwrap();
    let (failed, _) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(failed.outcome, OperationOutcome::Failed);
    assert_eq!(
        failed.failure_reason.as_deref(),
        Some("existing file changed since write approval")
    );
    assert_eq!(fs::read(&path).unwrap(), b"someone else edited");
    fs::write(&path, b"before").unwrap();
    let operation = prepare();
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    let (receipt, _) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert_eq!(fs::read(&path).unwrap(), b"after");
}

#[test]
fn read_is_bound_to_approval_and_revocation_is_checked_again_at_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("paper.txt"), "study notes").unwrap();
    let tool = runner(dir.path());
    let context = context();
    let operation = tool
        .prepare(
            context,
            OperationRequest::Read {
                path: "paper.txt".into(),
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &operation);
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    assert!(gate.revoke(operation.scope()));
    let (denied, output) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(denied.outcome, OperationOutcome::Failed);
    assert_eq!(
        denied.failure_reason.as_deref(),
        Some("file access was not explicitly approved for this agent and task")
    );
    assert!(output.is_none());
    assert_eq!(tool.receipt(approval.operation_id), Some(denied));
    let operation = tool
        .prepare(
            context,
            OperationRequest::Read {
                path: "paper.txt".into(),
            },
        )
        .unwrap();
    grant(&mut gate, &operation);
    assert!(matches!(
        tool.execute(&gate, &approval, operation, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
    let operation = tool
        .prepare(
            context,
            OperationRequest::Read {
                path: "paper.txt".into(),
            },
        )
        .unwrap();
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    let (receipt, output) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    let Some(ExecutionOutput::Read(content)) = output else {
        panic!("read result")
    };
    assert_eq!(content.content, "study notes");
}

#[test]
fn revoked_dispatch_is_finalized_in_authenticated_journal_and_regrant_needs_new_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let private = tempfile::tempdir().unwrap();
    let journal = private.path().join("revoked-attempts.jsonl");
    let key = [0x43; 32];
    let principal = context();
    let operation_id = OperationId::new();
    let request = || OperationRequest::Write {
        path: "must-not-exist.txt".into(),
        bytes: b"authorized only by a fresh approval".to_vec(),
        mode: WriteMode::CreateNew,
    };
    let target = workspace.path().join("must-not-exist.txt");
    let mut gate = PermissionGate::new();

    let (stale_approval, denied_receipt) = {
        let tool = LocalExecutor::with_authenticated_journal(
            workspace.path(),
            16_384,
            16_384,
            &journal,
            key,
        )
        .unwrap();
        let prepared = tool.prepare(principal, request()).unwrap();
        grant(&mut gate, &prepared);
        let approval = tool.approve(&gate, operation_id, &prepared).unwrap();
        assert!(gate.revoke(prepared.scope()));

        let (receipt, output) = tool
            .execute(&gate, &approval, prepared, &CancellationToken::new())
            .unwrap();
        assert_eq!(receipt.outcome, OperationOutcome::Failed);
        assert_eq!(
            receipt.failure_reason.as_deref(),
            Some("file access was not explicitly approved for this agent and task")
        );
        assert_eq!(receipt.operation_id, operation_id);
        assert!(receipt.output_sha256.is_none());
        assert!(output.is_none());
        assert!(!target.exists(), "revocation must prevent the file write");
        assert_eq!(tool.receipt(operation_id), Some(receipt.clone()));
        assert_eq!(fs::read_to_string(&journal).unwrap().lines().count(), 3);
        (approval, receipt)
    };

    // A new executor loads the authenticated receipt after the journal owner
    // exits. Regranting the *same* scope must not restore the old approval.
    let tool =
        LocalExecutor::with_authenticated_journal(workspace.path(), 16_384, 16_384, &journal, key)
            .unwrap();
    assert_eq!(tool.receipt(operation_id), Some(denied_receipt));
    let prepared = tool.prepare(principal, request()).unwrap();
    assert_eq!(prepared.digest(), stale_approval.digest);
    grant(&mut gate, &prepared);
    assert!(matches!(
        tool.execute(&gate, &stale_approval, prepared, &CancellationToken::new()),
        Err(ToolError::OperationReplay)
    ));
    assert!(!target.exists());

    let prepared = tool.prepare(principal, request()).unwrap();
    assert!(matches!(
        tool.approve(&gate, operation_id, &prepared),
        Err(ToolError::OperationReplay)
    ));
    let fresh = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let (receipt, output) = tool
        .execute(&gate, &fresh, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert!(matches!(output, Some(ExecutionOutput::Write(_))));
    assert_eq!(
        fs::read(&target).unwrap(),
        b"authorized only by a fresh approval"
    );
    assert_eq!(fs::read_to_string(&journal).unwrap().lines().count(), 6);
}

#[test]
fn read_rejects_same_length_content_swap_after_review_without_exposing_bytes() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("notes.txt");
    fs::write(&file, b"approved text").unwrap();
    let tool = runner(workspace.path());
    let context = context();
    let pending = tool
        .prepare(
            context,
            OperationRequest::Read {
                path: "notes.txt".into(),
            },
        )
        .unwrap();
    let original_preview = pending.preview();
    let OperationDetails::Read {
        sha256: reviewed_sha,
        bytes,
        ..
    } = original_preview.details
    else {
        panic!("expected read preview");
    };
    assert_eq!(bytes, b"approved text".len());
    assert_eq!(reviewed_sha.len(), 64);

    let mut gate = PermissionGate::new();
    grant(&mut gate, &pending);
    let approval = tool.approve(&gate, OperationId::new(), &pending).unwrap();
    fs::write(&file, b"replaced text").unwrap();
    let replacement = tool
        .prepare(
            context,
            OperationRequest::Read {
                path: "notes.txt".into(),
            },
        )
        .unwrap();
    assert_ne!(pending.digest(), replacement.digest());
    let (receipt, output) = tool
        .execute(&gate, &approval, pending, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert_eq!(
        receipt.failure_reason.as_deref(),
        Some("file contents changed since read approval")
    );
    assert!(output.is_none());
    assert_eq!(tool.receipt(approval.operation_id), Some(receipt));
    assert!(matches!(
        tool.execute(&gate, &approval, replacement, &CancellationToken::new()),
        Err(ToolError::ApprovalMismatch)
    ));
}

#[test]
fn oversized_existing_file_does_not_allocate_unbounded_memory() {
    let dir = tempfile::tempdir().unwrap();
    let tool = LocalExecutor::new(dir.path(), 8, 1024).unwrap();
    let target = dir.path().join("large.txt");
    fs::write(&target, vec![42; 4096]).unwrap();
    let operation = tool
        .prepare(
            context(),
            OperationRequest::Write {
                path: "large.txt".into(),
                bytes: b"new".to_vec(),
                mode: WriteMode::ReplaceIfSha256("0".repeat(64)),
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &operation);
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    let (receipt, _) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert_eq!(
        receipt.failure_reason.as_deref(),
        Some("file exceeds the configured byte limit")
    );
    assert_eq!(fs::metadata(target).unwrap().len(), 4096);
}

#[test]
fn process_output_limit_truncates_and_reports_failure() {
    let dir = tempfile::tempdir().unwrap();
    let tool = LocalExecutor::new(dir.path(), 1024, 32).unwrap();
    let operation = tool
        .prepare(
            context(),
            OperationRequest::Process {
                executable: std::env::current_exe().unwrap(),
                working_dir: PathBuf::new(),
                args: vec!["--list".into()],
                timeout: Duration::from_secs(10),
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &operation);
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    let (receipt, output) = tool
        .execute(&gate, &approval, operation, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::OutputLimit);
    let Some(ExecutionOutput::Process(output)) = output else {
        panic!("process result")
    };
    assert!(output.stdout.len() <= 32);
    assert!(output.stderr.len() <= 32);
}

#[test]
fn cancellation_before_dispatch_is_receipted_and_prevents_file_creation() {
    let dir = tempfile::tempdir().unwrap();
    let tool = runner(dir.path());
    let operation = tool
        .prepare(
            context(),
            OperationRequest::Write {
                path: "cancel.txt".into(),
                bytes: b"will never write".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &operation);
    let approval = tool.approve(&gate, OperationId::new(), &operation).unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let (receipt, output) = tool.execute(&gate, &approval, operation, &token).unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(output.is_none());
    assert!(!dir.path().join("cancel.txt").exists());
}

#[test]
fn rejected_paths_and_wrong_principal_have_no_effect() {
    let dir = tempfile::tempdir().unwrap();
    let tool = runner(dir.path());
    let context = context();
    for path in ["../escape.txt", "./bad.txt", "sub/../escape.txt"] {
        assert!(matches!(
            tool.prepare(
                context,
                OperationRequest::Write {
                    path: path.into(),
                    bytes: b"x".to_vec(),
                    mode: WriteMode::CreateNew,
                }
            ),
            Err(ToolError::InvalidRelativePath)
        ));
    }
    let prepared = tool
        .prepare(
            context,
            OperationRequest::Write {
                path: "only-me.txt".into(),
                bytes: b"x".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &prepared);
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let wrong_context = OperationContext {
        agent_id: AgentId::new(),
        ..context
    };
    let wrong = tool
        .prepare(
            wrong_context,
            OperationRequest::Write {
                path: "only-me.txt".into(),
                bytes: b"x".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, wrong, &CancellationToken::new()),
        Err(ToolError::PermissionDenied)
    ));
    assert!(!dir.path().join("only-me.txt").exists());
    // An unrelated principal cannot consume another principal's approval.
    let (receipt, _) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert_eq!(fs::read(dir.path().join("only-me.txt")).unwrap(), b"x");
}

#[test]
fn approved_process_uses_absolute_executable_and_captures_output() {
    let dir = tempfile::tempdir().unwrap();
    let tool = runner(dir.path());
    let executable = std::env::current_exe().unwrap();
    let context = context();
    let request = || OperationRequest::Process {
        executable: executable.clone(),
        working_dir: PathBuf::new(),
        args: vec!["--list".into()],
        timeout: Duration::from_secs(10),
    };
    let prepared = tool.prepare(context, request()).unwrap();
    assert_eq!(prepared.scope().action, ToolAction::ExecuteCommand);
    let mut gate = PermissionGate::new();
    assert!(matches!(
        tool.approve(&gate, OperationId::new(), &prepared),
        Err(ToolError::PermissionDenied)
    ));
    grant(&mut gate, &prepared);
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let (receipt, output) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    let Some(ExecutionOutput::Process(output)) = output else {
        panic!("expected process output")
    };
    assert_eq!(output.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("approved_process_uses_absolute_executable"));
    let different_args = tool
        .prepare(
            context,
            OperationRequest::Process {
                executable,
                working_dir: PathBuf::new(),
                args: vec!["--help".into()],
                timeout: Duration::from_secs(10),
            },
        )
        .unwrap();
    assert!(matches!(
        tool.execute(&gate, &approval, different_args, &CancellationToken::new()),
        Err(ToolError::PermissionDenied)
    ));
}

#[test]
fn running_process_can_be_cancelled_and_killed() {
    let dir = tempfile::tempdir().unwrap();
    let tool = runner(dir.path());
    let prepare = || {
        tool.prepare(
            context(),
            OperationRequest::Process {
                executable: std::env::current_exe().unwrap(),
                working_dir: PathBuf::new(),
                args: vec!["--ignored".into(), "--exact".into(), "slow_child".into()],
                timeout: Duration::from_secs(5),
            },
        )
        .unwrap()
    };
    let prepared = prepare();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &prepared);
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let cancel = CancellationToken::new();
    let cloned = cancel.clone();
    let notifier = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        cloned.cancel();
    });
    let (receipt, _) = tool.execute(&gate, &approval, prepared, &cancel).unwrap();
    notifier.join().unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    let prepared = tool
        .prepare(
            context(),
            OperationRequest::Process {
                executable: std::env::current_exe().unwrap(),
                working_dir: PathBuf::new(),
                args: vec!["--ignored".into(), "--exact".into(), "slow_child".into()],
                timeout: Duration::from_millis(100),
            },
        )
        .unwrap();
    grant(&mut gate, &prepared);
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let (receipt, _) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::TimedOut);
}

#[test]
#[ignore = "Child fixture; exercised only through a controlled process invocation"]
fn slow_child() {
    thread::sleep(Duration::from_secs(3));
}

#[test]
fn git_push_needs_distinct_permission_and_exact_approved_branch() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join(".git").join("config"),
        "[core]\n\tbare = false\n",
    )
    .unwrap();
    let tool = runner(dir.path());
    let executable = std::env::current_exe().unwrap();
    let ctx = context();
    let status = tool
        .prepare(
            ctx,
            OperationRequest::Git {
                executable: executable.clone(),
                repository: PathBuf::new(),
                action: GitAction::Status,
                timeout: Duration::from_secs(5),
            },
        )
        .unwrap();
    let push = tool
        .prepare(
            ctx,
            OperationRequest::Git {
                executable,
                repository: PathBuf::new(),
                action: GitAction::Push {
                    remote_url: "https://example.com/team/repo.git".into(),
                    branch: "main".into(),
                },
                timeout: Duration::from_secs(5),
            },
        )
        .unwrap();
    assert_eq!(status.scope().action, ToolAction::ExecuteCommand);
    assert_eq!(push.scope().action, ToolAction::GitPush);
    assert_ne!(status.digest(), push.digest());
    let mut gate = PermissionGate::new();
    grant(&mut gate, &status);
    assert!(matches!(
        tool.approve(&gate, OperationId::new(), &push),
        Err(ToolError::PermissionDenied)
    ));
    assert!(tool.approve(&gate, OperationId::new(), &status).is_ok());
    assert!(push
        .scope()
        .resource
        .contains("https://example.com/team/repo.git"));
    grant(&mut gate, &push);
    let approved_push = tool.approve(&gate, OperationId::new(), &push).unwrap();
    assert!(matches!(
        tool.prepare(
            ctx,
            OperationRequest::Git {
                executable: std::env::current_exe().unwrap(),
                repository: PathBuf::new(),
                action: GitAction::Push {
                    remote_url: "ext::evil".into(),
                    branch: "main".into()
                },
                timeout: Duration::from_secs(5),
            }
        ),
        Err(ToolError::InvalidCommand)
    ));
    fs::write(
        dir.path().join(".git").join("config"),
        "[url \"https://attacker.example/\"]\n\tinsteadOf = https://example.com/\n",
    )
    .unwrap();
    // A changed repository configuration invalidates approval before spawn.
    let (receipt, output) = tool
        .execute(&gate, &approved_push, push, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert!(output.is_none());
    assert!(matches!(
        tool.prepare(
            ctx,
            OperationRequest::Git {
                executable: std::env::current_exe().unwrap(),
                repository: PathBuf::new(),
                action: GitAction::Push {
                    remote_url: "https://example.com/team/repo.git".into(),
                    branch: "main".into()
                },
                timeout: Duration::from_secs(5),
            }
        ),
        Err(ToolError::InvalidCommand)
    ));
}

#[test]
fn git_status_executes_against_a_real_repository_when_git_installed() {
    // Homebrew commonly publishes /opt/homebrew/bin/git as a symlink into
    // Cellar. The executor correctly refuses symlink executables, even when
    // their targets are regular files: resolve the Git identity explicitly
    // *before* requesting approval, never by relaxing executable_file().
    let executable = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(if cfg!(windows) { "git.exe" } else { "git" }))
            .filter(|path| path.is_file())
            .filter_map(|path| fs::canonicalize(path).ok())
            .find(|path| {
                path.is_file()
                    && path.file_name().is_some_and(|name| {
                        if cfg!(windows) {
                            name.to_string_lossy().eq_ignore_ascii_case("git.exe")
                        } else {
                            name == "git"
                        }
                    })
            })
    });
    let Some(executable) = executable else { return };
    let dir = tempfile::tempdir().unwrap();
    assert!(Command::new(&executable)
        .args(["init", "-q"])
        .current_dir(dir.path())
        .status()
        .unwrap()
        .success());
    let tool = runner(dir.path());
    let prepared = tool
        .prepare(
            context(),
            OperationRequest::Git {
                executable,
                repository: PathBuf::new(),
                action: GitAction::Status,
                timeout: Duration::from_secs(10),
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    grant(&mut gate, &prepared);
    let approval = tool.approve(&gate, OperationId::new(), &prepared).unwrap();
    let (receipt, result) = tool
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Succeeded);
    assert!(matches!(result, Some(ExecutionOutput::Process(_))));
}

#[test]
fn git_action_refuses_path_lookup_and_shell_metacharacters() {
    let root = tempfile::tempdir().unwrap();
    let tool = runner(root.path());
    let executable = std::env::current_exe().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    // A caller must supply an existing absolute executable, never a program
    // name that could be resolved through a spoofed PATH entry.
    for candidate in ["git", "git.exe", "./git", "../git"] {
        assert!(matches!(
            tool.prepare(
                context(),
                OperationRequest::Git {
                    executable: candidate.into(),
                    repository: PathBuf::new(),
                    action: GitAction::Status,
                    timeout: Duration::from_secs(5),
                }
            ),
            Err(ToolError::InvalidCommand)
        ));
    }
    // GitPush does not invoke any shell and rejects branch/ref injection at
    // the preparation boundary, before an approval or child process exists.
    for branch in [
        "main;touch /tmp/owned",
        "main$(touch /tmp/owned)",
        "main`touch /tmp/owned`",
        "--upload-pack=sh",
        "main\nnext",
    ] {
        assert!(matches!(
            tool.prepare(
                context(),
                OperationRequest::Git {
                    executable: executable.clone(),
                    repository: PathBuf::new(),
                    action: GitAction::Push {
                        remote_url: "https://example.com/repo.git".into(),
                        branch: branch.into(),
                    },
                    timeout: Duration::from_secs(5),
                }
            ),
            Err(ToolError::InvalidCommand)
        ));
    }
    // A semicolon inside an otherwise valid HTTPS URL remains an exact argv
    // string, not a shell expression; this prepares a proposal only and never
    // approves a remote push or spawns Git.
    let inert_url = "https://example.com/repo.git;touch-owned";
    let prepared = tool
        .prepare(
            context(),
            OperationRequest::Git {
                executable,
                repository: PathBuf::new(),
                action: GitAction::Push {
                    remote_url: inert_url.into(),
                    branch: "main".into(),
                },
                timeout: Duration::from_secs(5),
            },
        )
        .unwrap();
    let OperationDetails::Command { argv, git, .. } = prepared.preview().details else {
        panic!("expected Git command preview");
    };
    assert!(git);
    assert_eq!(argv[argv.len() - 2], inert_url);
    assert_eq!(argv.last().unwrap(), "refs/heads/main:refs/heads/main");
    assert_eq!(argv.iter().filter(|arg| arg.as_str() == "push").count(), 1);
}

#[cfg(unix)]
#[test]
fn git_action_rejects_symlink_executable_even_if_target_is_a_real_binary() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let tool = runner(root.path());
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    let executable = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let alias = root.path().join("git");
    symlink(&executable, &alias).unwrap();
    assert!(matches!(
        tool.prepare(
            context(),
            OperationRequest::Git {
                executable: alias,
                repository: PathBuf::new(),
                action: GitAction::Status,
                timeout: Duration::from_secs(5),
            }
        ),
        Err(ToolError::InvalidCommand)
    ));
    // Explicit canonical identity remains possible; a symlink in the
    // selected request path never becomes a silent approval redirect.
    assert!(tool
        .prepare(
            context(),
            OperationRequest::Git {
                executable,
                repository: PathBuf::new(),
                action: GitAction::Status,
                timeout: Duration::from_secs(5),
            }
        )
        .is_ok());

    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join(".git")).unwrap();
    fs::write(
        outside.path().join(".git/config"),
        "[core]\n\tbare = false\n",
    )
    .unwrap();
    let linked_repository = root.path().join("outside-repository");
    symlink(outside.path(), &linked_repository).unwrap();
    // A Git operation must not escape the approved workspace via a symlinked
    // repository directory, even if a real .git directory exists at the end.
    assert!(matches!(
        tool.prepare(
            context(),
            OperationRequest::Git {
                executable: fs::canonicalize(std::env::current_exe().unwrap()).unwrap(),
                repository: PathBuf::from("outside-repository"),
                action: GitAction::Status,
                timeout: Duration::from_secs(5),
            }
        ),
        Err(ToolError::SymlinkDenied | ToolError::ScopeEscape)
    ));
}
