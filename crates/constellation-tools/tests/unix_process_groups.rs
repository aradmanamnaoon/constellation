//! POSIX child -> grandchild cleanup regressions. These run only on Unix;
//! the Windows development machine cannot execute them.
#![cfg(unix)]

use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, ExecutionOutput, LocalExecutor, OperationContext, OperationOutcome,
    OperationRequest,
};

const FIXTURE_DURATION: Duration = Duration::from_secs(16);
const READY_DEADLINE: Duration = Duration::from_secs(6);

fn wait_for_file(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < READY_DEADLINE,
            "missing fixture marker: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(15));
    }
}

#[test]
#[ignore = "executed only as grandchild of the approved local process"]
fn tick_grandchild_fixture() {
    fs::write("grandchild.ready", b"started").unwrap();
    let begin = Instant::now();
    let mut n = 0_u64;
    while begin.elapsed() < FIXTURE_DURATION {
        fs::write("grandchild.heartbeat", n.to_string()).unwrap();
        n += 1;
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "executed only as approved direct child"]
fn spawn_grandchild_fixture() {
    let mut grandchild = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tick_grandchild_fixture", "--ignored"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Wait in another thread so this fixture can exit while leaving the
    // grandchild alive, exercising cleanup after a successful direct child.
    let waiter = thread::spawn(move || grandchild.wait());
    wait_for_file(Path::new("grandchild.heartbeat"));
    if Path::new("parent-exit").exists() {
        wait_for_file(Path::new("release-parent"));
        return;
    }
    if Path::new("emit-output").exists() {
        wait_for_file(Path::new("release-output"));
        println!("{}", "x".repeat(4096));
    }
    thread::sleep(FIXTURE_DURATION);
    let _ = waiter.join();
}

#[test]
#[ignore = "executed only as an independent control process"]
fn unrelated_control_fixture() {
    fs::write("control.ready", b"started").unwrap();
    let start = Instant::now();
    let mut n = 0_u64;
    while start.elapsed() < FIXTURE_DURATION {
        fs::write("control.heartbeat", n.to_string()).unwrap();
        n += 1;
        thread::sleep(Duration::from_millis(20));
    }
}

struct ControlProcess(Child);

impl Drop for ControlProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[derive(Clone, Copy)]
enum Cause {
    Cancel,
    Timeout,
    OutputLimit,
    ParentExit,
}

fn run_group_cleanup(cause: Cause) {
    let workspace = tempfile::tempdir().unwrap();
    if matches!(cause, Cause::OutputLimit) {
        fs::write(workspace.path().join("emit-output"), b"1").unwrap();
    }
    if matches!(cause, Cause::ParentExit) {
        fs::write(workspace.path().join("parent-exit"), b"1").unwrap();
    }

    // An independent control runs in a distinct session, proving that
    // cleanup does not signal an unrelated process or the test runner shell.
    let mut unrelated_command = Command::new(std::env::current_exe().unwrap());
    unrelated_command
        .args(["--exact", "unrelated_control_fixture", "--ignored"])
        .current_dir(workspace.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: async-signal-safe setsid only, before exec; no Rust allocations
    // or locks in the forked child.
    unsafe {
        unrelated_command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut unrelated = ControlProcess(unrelated_command.spawn().unwrap());
    wait_for_file(&workspace.path().join("control.heartbeat"));

    let executor = LocalExecutor::new(
        workspace.path(),
        16_384,
        if matches!(cause, Cause::OutputLimit) {
            64
        } else {
            16_384
        },
    )
    .unwrap();
    let prepared = executor
        .prepare(
            OperationContext {
                agent_id: AgentId::new(),
                room_id: RoomId::new(),
                task_id: TaskId::new(),
            },
            OperationRequest::Process {
                executable: std::env::current_exe().unwrap(),
                working_dir: PathBuf::new(),
                args: vec![
                    "--exact".into(),
                    "spawn_grandchild_fixture".into(),
                    "--ignored".into(),
                    "--nocapture".into(),
                ],
                timeout: if matches!(cause, Cause::Timeout) {
                    Duration::from_secs(3)
                } else {
                    Duration::from_secs(10)
                },
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor
        .approve(&gate, OperationId::new(), &prepared)
        .unwrap();
    let cancel = CancellationToken::new();
    let for_worker = cancel.clone();
    let run = thread::spawn(move || {
        executor
            .execute(&gate, &approval, prepared, &for_worker)
            .unwrap()
    });

    wait_for_file(&workspace.path().join("grandchild.heartbeat"));
    match cause {
        Cause::Cancel => cancel.cancel(),
        Cause::OutputLimit => fs::write(workspace.path().join("release-output"), b"1").unwrap(),
        Cause::ParentExit => fs::write(workspace.path().join("release-parent"), b"1").unwrap(),
        Cause::Timeout => {}
    }
    let (receipt, output) = run.join().unwrap();
    let expected = match cause {
        Cause::Cancel => OperationOutcome::Cancelled,
        Cause::Timeout => OperationOutcome::TimedOut,
        Cause::OutputLimit => OperationOutcome::OutputLimit,
        Cause::ParentExit => OperationOutcome::Succeeded,
    };
    assert_eq!(receipt.outcome, expected);
    if let (Cause::OutputLimit, Some(ExecutionOutput::Process(result))) = (cause, output) {
        assert!(result.stdout.len() <= 64);
        assert!(result.stderr.len() <= 64);
    }

    // A terminated descendant cannot keep making progress, even on systems
    // that temporarily retain a zombie PID after its parent was reaped.
    thread::sleep(Duration::from_millis(80));
    let heartbeat = workspace.path().join("grandchild.heartbeat");
    let before = fs::read(&heartbeat).unwrap();
    let control_path = workspace.path().join("control.heartbeat");
    let control_before = fs::read(&control_path).unwrap();
    thread::sleep(Duration::from_millis(220));
    assert_eq!(
        fs::read(heartbeat).unwrap(),
        before,
        "grandchild kept running"
    );
    assert_ne!(
        fs::read(control_path).unwrap(),
        control_before,
        "cleanup affected unrelated control process"
    );
    assert!(unrelated.0.try_wait().unwrap().is_none());
}

#[test]
fn cancellation_reaps_unix_grandchild_without_killing_unrelated_control() {
    run_group_cleanup(Cause::Cancel);
}

#[test]
fn timeout_reaps_unix_grandchild_without_killing_unrelated_control() {
    run_group_cleanup(Cause::Timeout);
}

#[test]
fn output_limit_reaps_unix_grandchild_without_killing_unrelated_control() {
    run_group_cleanup(Cause::OutputLimit);
}

#[test]
fn direct_parent_exit_reaps_unix_grandchild_without_killing_control() {
    run_group_cleanup(Cause::ParentExit);
}
