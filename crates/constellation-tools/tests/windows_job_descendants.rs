//! Windows-only process-tree cancellation regression with an unrelated control.
//!
//! Every process in this test is an instance of this test binary, launched with
//! CREATE_NO_WINDOW inside an isolated temporary directory. Only the handles
//! for these deliberately created processes are inspected; no system process
//! enumeration, broad termination, user sessions or external binaries are used.
#![cfg(windows)]

use std::{
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId, ToolAction};
use constellation_tools::{
    CancellationToken, ExecutionOutput, ExecutionReceipt, LocalExecutor, OperationContext,
    OperationOutcome, OperationRequest,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, WaitForSingleObject, CREATE_NO_WINDOW},
};

const SETUP_TIMEOUT: Duration = Duration::from_secs(8);
const FIXTURE_LIFETIME: Duration = Duration::from_secs(24);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(16);
const SYNCHRONIZE: u32 = 0x0010_0000;

fn wait_for_file(path: &Path) {
    let started = Instant::now();
    while !path.is_file() {
        assert!(
            started.elapsed() < SETUP_TIMEOUT,
            "controlled fixture failed to signal readiness: {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_until_release(name: &str) {
    let started = Instant::now();
    while !Path::new(name).exists() && started.elapsed() < FIXTURE_LIFETIME {
        thread::sleep(Duration::from_millis(25));
    }
}

// Only the test itself invokes these ignored fixtures with exact test filters.
// They cannot be launched by the normal Cargo test harness as ordinary tests.
#[test]
#[ignore = "launched as a controlled descendant by the approved executor"]
fn scoped_grandchild_fixture() {
    fs::write("grandchild.ready", std::process::id().to_string()).unwrap();
    wait_until_release("release-grandchild");
}

#[test]
#[ignore = "launched as the approved executor's direct child"]
fn scoped_parent_fixture() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "scoped_grandchild_fixture", "--ignored"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start controlled grandchild");
    let pid = child.id();
    let _reaper = thread::spawn(move || child.wait());
    fs::write("grandchild.pid", pid.to_string()).unwrap();
    wait_until_release("release-parent");
}

#[test]
#[ignore = "launched directly by this test, outside the executor's Job Object"]
fn unrelated_control_fixture() {
    fs::write("control.ready", std::process::id().to_string()).unwrap();
    wait_until_release("release-control");
}

/// This guard owns exactly one process created by this test. Panic paths kill
/// and reap it, without consulting or affecting other processes on the host.
struct ControlChild(Child);

impl Drop for ControlChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// Holding the process handle prevents a reused PID from weakening the test.
struct DescendantHandle(HANDLE);

impl DescendantHandle {
    fn open(pid: u32) -> Self {
        // SYNCHRONIZE allows waiting only. It grants no termination rights.
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        assert!(
            !handle.is_null(),
            "could not open controlled grandchild PID {pid}"
        );
        Self(handle)
    }

    fn is_running(&self) -> bool {
        (unsafe { WaitForSingleObject(self.0, 0) }) == WAIT_TIMEOUT
    }

    fn exited_within(&self, milliseconds: u32) -> bool {
        (unsafe { WaitForSingleObject(self.0, milliseconds) }) == WAIT_OBJECT_0
    }
}

impl Drop for DescendantHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

type ToolResult = constellation_tools::Result<(ExecutionReceipt, Option<ExecutionOutput>)>;

/// Ensure the tool cancellation signal is sent and its thread joined even if
/// a setup assertion fails while a real descendant is still running.
struct RunningTool {
    cancel: CancellationToken,
    thread: Option<JoinHandle<ToolResult>>,
}

impl RunningTool {
    fn finish(mut self) -> ToolResult {
        self.thread
            .take()
            .unwrap()
            .join()
            .expect("tool thread panicked")
    }
}

impl Drop for RunningTool {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn cancelling_approved_job_kills_its_grandchild_but_not_an_unrelated_control() {
    let workspace = tempfile::tempdir().unwrap();
    let executable = std::env::current_exe().unwrap();

    // This is deliberately NOT launched by LocalExecutor. Its independent
    // process handle demonstrates that closing the approved Job Object cannot
    // indiscriminately terminate all processes with the same executable name.
    let mut control = ControlChild(
        Command::new(&executable)
            .args(["--exact", "unrelated_control_fixture", "--ignored"])
            .current_dir(workspace.path())
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start independent control"),
    );
    wait_for_file(&workspace.path().join("control.ready"));
    assert!(control.0.try_wait().unwrap().is_none());

    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();
    let prepared = executor
        .prepare(
            OperationContext {
                agent_id: AgentId::new(),
                room_id: RoomId::new(),
                task_id: TaskId::new(),
            },
            OperationRequest::Process {
                executable,
                working_dir: PathBuf::new(),
                args: vec![
                    "--exact".into(),
                    "scoped_parent_fixture".into(),
                    "--ignored".into(),
                ],
                timeout: PROCESS_TIMEOUT,
            },
        )
        .unwrap();
    assert_eq!(prepared.scope().action, ToolAction::ExecuteCommand);
    let mut gate = PermissionGate::new();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor
        .approve(&gate, OperationId::new(), &prepared)
        .unwrap();

    let cancel = CancellationToken::new();
    let child_cancel = cancel.clone();
    let running = RunningTool {
        cancel: cancel.clone(),
        thread: Some(thread::spawn(move || {
            executor.execute(&gate, &approval, prepared, &child_cancel)
        })),
    };

    wait_for_file(&workspace.path().join("grandchild.pid"));
    wait_for_file(&workspace.path().join("grandchild.ready"));
    let pid: u32 = fs::read_to_string(workspace.path().join("grandchild.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let descendant = DescendantHandle::open(pid);
    assert!(
        descendant.is_running(),
        "grandchild exited before cancellation"
    );
    assert!(control.0.try_wait().unwrap().is_none());

    cancel.cancel();
    let (receipt, _) = running
        .finish()
        .expect("approved tool must return a receipt");
    assert_eq!(receipt.outcome, OperationOutcome::Cancelled);
    assert!(
        descendant.exited_within(5_000),
        "Job Object cancellation left controlled grandchild {pid} alive"
    );
    assert!(
        control.0.try_wait().unwrap().is_none(),
        "cancelling the Job Object also terminated the independent control"
    );

    // Normal graceful exit for the independent fixture; ControlChild remains a
    // bounded cleanup fallback on every failure path.
    fs::write(workspace.path().join("release-control"), b"done").unwrap();
    let started = Instant::now();
    while control.0.try_wait().unwrap().is_none() && started.elapsed() < SETUP_TIMEOUT {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(control.0.try_wait().unwrap().unwrap().success());
}
