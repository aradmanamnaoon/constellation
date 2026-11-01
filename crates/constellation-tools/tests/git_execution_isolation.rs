//! Real offline Git process tests. Only temporary repositories are modified.
//! No push or fetch is performed; no remote endpoint is contacted.
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId, ToolAction};
use constellation_tools::{
    CancellationToken, ExecutionOutput, GitAction, LocalExecutor, OperationContext,
    OperationOutcome, OperationRequest, ToolError,
};

fn git_binary() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(if cfg!(windows) { "git.exe" } else { "git" }))
        .filter(|candidate| candidate.is_file())
        // Homebrew commonly exposes /opt/homebrew/bin/git as a symlink.
        // The production executor intentionally rejects a symlink supplied
        // for approval: tests must present the canonical executable identity
        // and never weaken the executor's host-enforced identity contract.
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .find(|canonical| {
            canonical.is_file()
                && canonical.file_name().is_some_and(|name| {
                    if cfg!(windows) {
                        name.to_string_lossy().eq_ignore_ascii_case("git.exe")
                    } else {
                        name == "git"
                    }
                })
        })
}

fn new_repo(path: &Path, git: &Path) {
    fs::create_dir_all(path).unwrap();
    assert!(Command::new(git)
        .args(["init", "-q"])
        .current_dir(path)
        .status()
        .unwrap()
        .success());
}

fn ctx() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn prepare_git(
    executor: &LocalExecutor,
    git: &Path,
    action: GitAction,
) -> constellation_tools::Result<constellation_tools::PreparedOperation> {
    executor.prepare(
        ctx(),
        OperationRequest::Git {
            executable: git.to_path_buf(),
            repository: PathBuf::new(),
            action,
            timeout: Duration::from_secs(12),
        },
    )
}

fn run_approved(
    executor: &LocalExecutor,
    prepared: constellation_tools::PreparedOperation,
) -> (OperationOutcome, Option<ExecutionOutput>) {
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor
        .approve(&gate, OperationId::new(), &prepared)
        .unwrap();
    let (receipt, output) = executor
        .execute(&gate, &approval, prepared, &CancellationToken::new())
        .unwrap();
    (receipt.outcome, output)
}

struct TemporarilySetEnv(Vec<(String, Option<OsString>)>);
impl TemporarilySetEnv {
    fn set(entries: &[(&str, &str)]) -> Self {
        let mut previous = Vec::new();
        for &(key, value) in entries {
            previous.push((key.to_string(), env::var_os(key)));
            // Test binary is run with --test-threads=1. The local executor
            // must NOT inherit these poison variables into its Git child.
            env::set_var(key, value);
        }
        Self(previous)
    }
}
impl Drop for TemporarilySetEnv {
    fn drop(&mut self) {
        for (key, previous) in self.0.drain(..).rev() {
            if let Some(value) = previous {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }
}

#[test]
fn real_git_status_and_diff_ignore_ambient_config_and_path_poisoning() {
    let Some(git) = git_binary() else { return };
    let workspace = tempfile::tempdir().unwrap();
    let repository = workspace.path().join("repo");
    new_repo(&repository, &git);
    fs::write(repository.join("readme.txt"), "original\n").unwrap();
    assert!(Command::new(&git)
        .args(["add", "readme.txt"])
        .current_dir(&repository)
        .status()
        .unwrap()
        .success());
    fs::write(repository.join("readme.txt"), "changed\n").unwrap();

    let global_config = workspace.path().join("attacker-global.gitconfig");
    fs::write(&global_config, "[core]\n\tbare = true\n").unwrap();
    let attacker_path = workspace.path().join("attacker-path");
    fs::create_dir(&attacker_path).unwrap();
    let poisoned_path = env::join_paths([attacker_path]).unwrap();
    let _poison = TemporarilySetEnv::set(&[
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "core.bare"),
        ("GIT_CONFIG_VALUE_0", "true"),
        ("GIT_CONFIG_GLOBAL", global_config.to_str().unwrap()),
        ("GIT_ALLOW_PROTOCOL", "file"),
        ("GIT_NO_LAZY_FETCH", "0"),
        ("GIT_OPTIONAL_LOCKS", "1"),
        ("PATH", poisoned_path.to_str().unwrap()),
    ]);
    let tool = LocalExecutor::new(&repository, 65536, 32768).unwrap();
    let (status, output) =
        run_approved(&tool, prepare_git(&tool, &git, GitAction::Status).unwrap());
    assert_eq!(status, OperationOutcome::Succeeded);
    let Some(ExecutionOutput::Process(output)) = output else {
        panic!("status output")
    };
    assert!(String::from_utf8_lossy(&output.stdout).contains("readme.txt"));

    let (diff, output) = run_approved(&tool, prepare_git(&tool, &git, GitAction::Diff).unwrap());
    assert_eq!(diff, OperationOutcome::Succeeded);
    let Some(ExecutionOutput::Process(output)) = output else {
        panic!("diff output")
    };
    assert!(String::from_utf8_lossy(&output.stdout).contains("changed"));
    assert_eq!(output.exit_code, Some(0));
}

#[test]
fn executable_capable_local_git_configuration_is_rejected_for_read_only_actions() {
    let Some(git) = git_binary() else { return };
    let workspace = tempfile::tempdir().unwrap();
    new_repo(workspace.path(), &git);
    let config_path = workspace.path().join(".git/config");
    let original = fs::read_to_string(&config_path).unwrap();
    let tool = LocalExecutor::new(workspace.path(), 65536, 32768).unwrap();
    for unsafe_entry in [
        "[filter \"run\"]\n\tclean = echo unreviewed\n",
        "[diff \"run\"]\n\tcommand = echo unreviewed\n",
        "[credential]\n\thelper = echo unreviewed\n",
        "[include]\n\tpath = ../other-config\n",
        "[includeIf \"gitdir:/\"]\n\tpath = ../other-config\n",
        "[url \"https://bad.example/\"]\n\tinsteadOf = https://good.example/\n",
        "[core]\n\tfsmonitor = echo unreviewed\n",
        "[core]\n\thooksPath = ../unreviewed\n",
        "[core]\n\tworktree = ../outside\n",
        "[remote \"origin\"]\n\treceivepack = echo unreviewed\n",
        "[extensions]\n\tworktreeConfig = true\n",
    ] {
        fs::write(&config_path, format!("{original}\n{unsafe_entry}")).unwrap();
        for action in [GitAction::Status, GitAction::Diff] {
            assert!(
                matches!(
                    prepare_git(&tool, &git, action),
                    Err(ToolError::InvalidCommand)
                ),
                "dangerous Git config was accepted: {unsafe_entry}"
            );
        }
    }
    fs::write(&config_path, original).unwrap();
    assert!(prepare_git(&tool, &git, GitAction::Status).is_ok());
}

#[test]
fn changing_git_config_after_human_approval_consumes_attempt_without_spawning() {
    let Some(git) = git_binary() else { return };
    let workspace = tempfile::tempdir().unwrap();
    new_repo(workspace.path(), &git);
    let tool = LocalExecutor::new(workspace.path(), 65536, 32768).unwrap();
    let approved = prepare_git(&tool, &git, GitAction::Status).unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(approved.scope().clone()).unwrap();
    let id = OperationId::new();
    let approval = tool.approve(&gate, id, &approved).unwrap();
    let config_path = workspace.path().join(".git/config");
    let original = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        format!("{original}\n[filter \"unreviewed\"]\n\tprocess = echo unsafe\n"),
    )
    .unwrap();
    let (receipt, output) = tool
        .execute(&gate, &approval, approved, &CancellationToken::new())
        .unwrap();
    assert_eq!(receipt.outcome, OperationOutcome::Failed);
    assert!(output.is_none());
    assert_eq!(tool.receipt(id), Some(receipt));
    assert!(prepare_git(&tool, &git, GitAction::Status).is_err());
}

#[test]
fn actual_local_repo_git_push_preview_still_requires_distinct_human_scope() {
    let Some(git) = git_binary() else { return };
    let workspace = tempfile::tempdir().unwrap();
    new_repo(workspace.path(), &git);
    let tool = LocalExecutor::new(workspace.path(), 65536, 32768).unwrap();
    let prepared = prepare_git(
        &tool,
        &git,
        GitAction::Push {
            remote_url: "https://example.test/team/repository.git".to_owned(),
            branch: "main".to_owned(),
        },
    )
    .unwrap();
    assert_eq!(prepared.scope().action, ToolAction::GitPush);
    let gate = PermissionGate::new();
    assert!(matches!(
        tool.approve(&gate, OperationId::new(), &prepared),
        Err(ToolError::PermissionDenied)
    ));
    // This is intentionally a preview-only test: no push/network call.
}

#[test]
fn git_action_cannot_execute_an_unrelated_approved_binary() {
    let Some(git) = git_binary() else { return };
    let workspace = tempfile::tempdir().unwrap();
    new_repo(workspace.path(), &git);
    let tool = LocalExecutor::new(workspace.path(), 65536, 32768).unwrap();
    let non_git_binary = env::current_exe().unwrap();
    let prepared = tool
        .prepare(
            ctx(),
            OperationRequest::Git {
                executable: non_git_binary,
                repository: PathBuf::new(),
                action: GitAction::Status,
                timeout: Duration::from_secs(5),
            },
        )
        .unwrap();
    let (outcome, result) = run_approved(&tool, prepared);
    assert_eq!(outcome, OperationOutcome::Failed);
    assert!(result.is_none(), "a non-Git process must never launch");
}
