#![cfg(windows)]

use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, LocalExecutor, OperationContext, OperationOutcome, OperationRequest,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{OpenProcess, WaitForSingleObject, CREATE_NO_WINDOW},
};

const FIXTURE_LIFETIME: Duration = Duration::from_secs(18);
const FIXTURE_SETUP_DEADLINE: Duration = Duration::from_secs(7);

fn wait_for_file(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < FIXTURE_SETUP_DEADLINE,
            "fixture did not start: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(15));
    }
}

#[test]
#[ignore = "Spawned by the guarded local executor only"]
fn parent_spawns_grandchild() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "grandchild_sleeper", "--ignored"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().expect("spawn grandchild");
    fs::write("grandchild.pid", child.id().to_string()).unwrap();
    let reaper = thread::spawn(move || child.wait());
    wait_for_file(Path::new("grandchild.ready"));
    if Path::new("exit-parent").exists() {
        // The executor must close the job when the direct child exits even
        // without a timeout/cancellation, or this descendant would survive.
        wait_for_file(Path::new("release-parent-exit"));
        return;
    }
    if Path::new("emit-output").exists() {
        wait_for_file(Path::new("release-output"));
        println!("{}", "x".repeat(32_768));
    }
    thread::sleep(FIXTURE_LIFETIME);
    let _ = reaper.join();
}

#[test]
#[ignore = "Spawned by the child fixture only"]
fn grandchild_sleeper() {
    fs::write("grandchild.ready", b"running").unwrap();
    thread::sleep(FIXTURE_LIFETIME);
}

#[derive(Clone, Copy)]
enum StopReason {
    Cancel,
    Deadline,
    Output,
    ParentExit,
}

fn assert_grandchild_reaped(reason: StopReason) {
    let workspace = tempfile::tempdir().unwrap();
    if matches!(reason, StopReason::Output) {
        fs::write(workspace.path().join("emit-output"), b"1").unwrap();
    }
    if matches!(reason, StopReason::ParentExit) {
        fs::write(workspace.path().join("exit-parent"), b"1").unwrap();
    }
    let executor = LocalExecutor::new(
        workspace.path(),
        16_384,
        if matches!(reason, StopReason::Output) {
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
                    "parent_spawns_grandchild".into(),
                    "--ignored".into(),
                    "--nocapture".into(),
                ],
                timeout: if matches!(reason, StopReason::Deadline) {
                    Duration::from_secs(5)
                } else {
                    Duration::from_secs(12)
                },
            },
        )
        .unwrap();
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor
        .approve(&gate, OperationId::new(), &prepared)
        .unwrap();
    let cancellation = CancellationToken::new();
    let worker_cancel = cancellation.clone();
    let run = thread::spawn(move || {
        executor
            .execute(&gate, &approval, prepared, &worker_cancel)
            .unwrap()
            .0
            .outcome
    });

    wait_for_file(&workspace.path().join("grandchild.ready"));
    let pid = fs::read_to_string(workspace.path().join("grandchild.pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    // Hold a real process handle before ending the job, so PID reuse cannot
    // make a terminated grandchild appear alive or vice versa.
    let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) }; // SYNCHRONIZE
    assert!(!handle.is_null(), "cannot open grandchild process {pid}");

    match reason {
        StopReason::Cancel => cancellation.cancel(),
        StopReason::Deadline => {}
        StopReason::Output => {
            fs::write(workspace.path().join("release-output"), b"1").unwrap();
        }
        StopReason::ParentExit => {
            fs::write(workspace.path().join("release-parent-exit"), b"1").unwrap();
        }
    }

    let outcome = run.join().unwrap();
    let expected = match reason {
        StopReason::Cancel => OperationOutcome::Cancelled,
        StopReason::Deadline => OperationOutcome::TimedOut,
        StopReason::Output => OperationOutcome::OutputLimit,
        StopReason::ParentExit => OperationOutcome::Succeeded,
    };
    assert_eq!(outcome, expected);
    let wait_status = unsafe { WaitForSingleObject(handle, 5000) };
    unsafe { CloseHandle(handle) };
    assert_eq!(
        wait_status, WAIT_OBJECT_0,
        "grandchild {pid} survived {outcome:?}"
    );
}

#[test]
fn cancellation_kills_real_grandchild() {
    assert_grandchild_reaped(StopReason::Cancel);
}

#[test]
fn timeout_kills_real_grandchild() {
    assert_grandchild_reaped(StopReason::Deadline);
}

#[test]
fn output_limit_kills_real_grandchild() {
    assert_grandchild_reaped(StopReason::Output);
}

#[test]
fn successful_parent_exit_also_reaps_remaining_grandchild() {
    assert_grandchild_reaped(StopReason::ParentExit);
}
