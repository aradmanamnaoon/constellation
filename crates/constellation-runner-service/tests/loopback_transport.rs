//! Exercises actual OS TCP sockets and independent runner CLI processes.
//! No command execution is possible: received jobs remain inert SQLite rows.
use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use constellation_runner_protocol::{
    read_frame, write_frame, BoundPermission, Capability, Endpoint, MessagePolicy, Pair,
    PermissionScope, Secret, Side, TaskAttribution,
};
use constellation_runner_service::{
    authenticate_loopback_client, authenticate_loopback_client_bound, serve_loopback, JobState,
    PairIdentity, RunnerStore, SystemClock,
};

const KEY: [u8; 32] = [0x74; 32]; // Synthetic test-only shared pairing key.

fn setup() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("runner.sqlite");
    let mut store = RunnerStore::initialize(&db, KEY, "workstation-1", "runner-1").unwrap();
    store.approve_pair().unwrap();
    (tmp, db)
}

fn workstation(key: [u8; 32]) -> Endpoint {
    let mut pair = Pair::enroll(
        "workstation-1",
        "runner-1",
        Secret::from_bytes(key).unwrap(),
    )
    .unwrap();
    pair.approve().unwrap();
    Endpoint::new(Side::Workstation, pair, MessagePolicy::default()).unwrap()
}

fn scope() -> PermissionScope {
    PermissionScope {
        capability: Capability::DraftText,
        resource_id: "research-doc-1".into(),
    }
}

fn connect(addr: SocketAddr, key: &[u8; 32]) -> TcpStream {
    let mut socket = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    authenticate_loopback_client_bound(&mut socket, key, &identity()).unwrap();
    socket
}

#[test]
fn signed_receipt_recovers_after_lost_tcp_delivery_and_real_service_restart() {
    let (_dir, db) = setup();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let first_server = thread::spawn(move || serve_loopback(listener, &stored, KEY, 1));

    let now = SystemClock;
    let mut ws = workstation(KEY);
    let job = ws
        .issue_bound_job(
            "recovery-job-1",
            "agent-1",
            BoundPermission {
                scope: scope(),
                task: TaskAttribution {
                    owner_id: "owner-1".into(),
                    run_id: "run-1".into(),
                    task_id: "task-1".into(),
                },
            },
            "Queue once, no execution",
            30_000,
            &now,
        )
        .unwrap();
    let mut first = connect(addr, &KEY);
    write_frame(&mut first, &job).unwrap();
    let response_lost_by_client = read_frame(&mut first).unwrap().unwrap();
    drop(response_lost_by_client);
    drop(first);
    assert_eq!(first_server.join().unwrap().unwrap(), 1);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let restarted = thread::spawn(move || serve_loopback(listener, &stored, KEY, 1));
    let mut second = connect(addr, &KEY);
    let reconciliation = ws.issue_receipt_reconciliation(1, 30_000, &now).unwrap();
    write_frame(&mut second, &reconciliation).unwrap();
    let recovered = read_frame(&mut second).unwrap().unwrap();
    assert!(ws.receive(&recovered, &now).unwrap().is_new());
    drop(second);
    assert_eq!(restarted.join().unwrap().unwrap(), 1);
    let jobs = RunnerStore::open(&db, KEY).unwrap().list_jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, JobState::Pending);
    assert_eq!(jobs[0].task.as_ref().unwrap().task_id, "task-1");
}

fn identity() -> PairIdentity {
    PairIdentity {
        workstation_id: "workstation-1".into(),
        runner_id: "runner-1".into(),
        key_epoch: 1,
    }
}

#[test]
fn mutual_handshake_replay_and_cancel_are_durable_across_real_tcp_connections() {
    let (_dir, db) = setup();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let server = thread::spawn(move || serve_loopback(listener, &stored, KEY, 4));
    let now = SystemClock;
    let mut ws = workstation(KEY);
    let signed = ws
        .issue_job(
            "job-1",
            "agent-1",
            scope(),
            "Draft a short text without executing tools",
            30_000,
            &now,
        )
        .unwrap();

    // A different key cannot pass the mutual nonce proof, and no inbox row
    // or authenticated receipt is created by a failed handshake.
    let mut wrong = TcpStream::connect(addr).unwrap();
    wrong
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(authenticate_loopback_client(&mut wrong, &[0x75; 32]).is_err());
    drop(wrong);
    assert!(RunnerStore::open(&db, KEY)
        .unwrap()
        .list_jobs()
        .unwrap()
        .is_empty());

    let mut first = connect(addr, &KEY);
    write_frame(&mut first, &signed).unwrap();
    let receipt = read_frame(&mut first).unwrap().unwrap();
    assert!(ws.receive(&receipt, &now).unwrap().is_new());
    drop(first);
    assert_eq!(
        RunnerStore::open(&db, KEY).unwrap().list_jobs().unwrap()[0].state,
        JobState::Pending
    );

    // Even a freshly authenticated new socket cannot replay a signed job.
    let mut replay = connect(addr, &KEY);
    write_frame(&mut replay, &signed).unwrap();
    assert!(read_frame(&mut replay).unwrap().is_none());
    drop(replay);

    // The legitimate workstation can cancel, but not execute a process.
    let signed_cancel = ws.issue_cancel("job-1", 30_000, &now).unwrap();
    let mut second = connect(addr, &KEY);
    write_frame(&mut second, &signed_cancel).unwrap();
    let receipt = read_frame(&mut second).unwrap().unwrap();
    assert!(ws.receive(&receipt, &now).unwrap().is_new());
    drop(second);
    assert_eq!(server.join().unwrap().unwrap(), 4);
    drop(RunnerStore::open(&db, KEY).unwrap());
    let jobs = RunnerStore::open(&db, KEY).unwrap().list_jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, JobState::Cancelled);
}

#[test]
fn expected_pair_and_epoch_are_authenticated_before_any_signed_job_is_accepted() {
    let (_dir, db) = setup();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let server = thread::spawn(move || serve_loopback(listener, &stored, KEY, 4));

    let mut wrong_workstation = identity();
    wrong_workstation.workstation_id = "another-owner-workstation".into();
    let mut wrong_runner = identity();
    wrong_runner.runner_id = "another-runner".into();
    let mut stale_epoch = identity();
    stale_epoch.key_epoch = 2;
    for wrong in [&wrong_workstation, &wrong_runner, &stale_epoch] {
        let mut socket = TcpStream::connect(addr).unwrap();
        assert!(authenticate_loopback_client_bound(&mut socket, &KEY, wrong).is_err());
        drop(socket);
    }
    assert!(RunnerStore::open(&db, KEY)
        .unwrap()
        .list_jobs()
        .unwrap()
        .is_empty());

    let mut correct = connect(addr, &KEY);
    let clock = SystemClock;
    let mut workstation = workstation(KEY);
    let job = workstation
        .issue_job(
            "job-context-1",
            "agent-1",
            scope(),
            "Draft only",
            30_000,
            &clock,
        )
        .unwrap();
    write_frame(&mut correct, &job).unwrap();
    let receipt = read_frame(&mut correct).unwrap().unwrap();
    assert!(workstation.receive(&receipt, &clock).unwrap().is_new());
    assert_eq!(server.join().unwrap().unwrap(), 4);
    assert_eq!(
        RunnerStore::open(&db, KEY)
            .unwrap()
            .list_jobs()
            .unwrap()
            .len(),
        1
    );

    // Restarted service reads the same operator-pinned identity and replay
    // checkpoint; renewed challenges never restore authority to replay jobs.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let restarted = thread::spawn(move || serve_loopback(listener, &stored, KEY, 1));
    let mut replay = connect(addr, &KEY);
    write_frame(&mut replay, &job).unwrap();
    assert!(read_frame(&mut replay).unwrap().is_none());
    assert_eq!(restarted.join().unwrap().unwrap(), 1);
    assert_eq!(
        RunnerStore::open(&db, KEY)
            .unwrap()
            .list_jobs()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn previous_handshake_version_is_rejected_without_mutating_durable_state() {
    let (_dir, db) = setup();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let server = thread::spawn(move || serve_loopback(listener, &stored, KEY, 1));
    let mut legacy = TcpStream::connect(addr).unwrap();
    legacy
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut challenge = [0_u8; 69];
    legacy.read_exact(&mut challenge).unwrap();
    assert_eq!(&challenge[..5], b"CRLS2");
    let mut legacy_answer = [0_u8; 69];
    legacy_answer[..5].copy_from_slice(b"CRLC1");
    legacy.write_all(&legacy_answer).unwrap();
    assert!(legacy.read(&mut [0_u8]).unwrap_or(0) == 0);
    assert_eq!(server.join().unwrap().unwrap(), 1);
    assert!(RunnerStore::open(&db, KEY)
        .unwrap()
        .list_jobs()
        .unwrap()
        .is_empty());
}

#[test]
fn unauthenticated_forged_and_oversized_tcp_frames_never_mutate_the_inbox() {
    let (_dir, db) = setup();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let stored = db.clone();
    let server = thread::spawn(move || serve_loopback(listener, &stored, KEY, 3));
    let clock = SystemClock;
    let mut ws = workstation(KEY);
    let signed = ws
        .issue_job("job-1", "agent-1", scope(), "Queued draft", 30_000, &clock)
        .unwrap();

    // Sending a valid per-message HMAC is not a substitute for the separate
    // session-bound nonce handshake. The service sends only a random
    // challenge, then closes without accepting anything.
    let mut preauth = TcpStream::connect(addr).unwrap();
    preauth
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(&mut preauth, &signed).unwrap();
    let mut challenge = [0; 69];
    preauth.read_exact(&mut challenge).unwrap();
    assert_eq!(&challenge[..5], b"CRLS2");
    assert!(read_frame(&mut preauth).is_err() || preauth.read(&mut [0_u8]).unwrap_or(0) == 0);
    drop(preauth);

    let mut forged = connect(addr, &KEY);
    let mut tampered = signed.clone();
    *tampered.last_mut().unwrap() ^= 1;
    write_frame(&mut forged, &tampered).unwrap();
    assert!(read_frame(&mut forged).unwrap().is_none());
    drop(forged);

    let mut oversized = connect(addr, &KEY);
    oversized
        .write_all(&((16 * 1024 + 1) as u32).to_be_bytes())
        .unwrap();
    assert!(read_frame(&mut oversized).unwrap().is_none());
    drop(oversized);
    assert_eq!(server.join().unwrap().unwrap(), 3);
    assert!(RunnerStore::open(&db, KEY)
        .unwrap()
        .list_jobs()
        .unwrap()
        .is_empty());
}

fn executable() -> &'static str {
    env!("CARGO_BIN_EXE_constellation-runner-service")
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(executable()).args(args).output().unwrap()
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_private_test_key(path: &Path) -> [u8; 32] {
    let text = fs::read_to_string(path).unwrap();
    let text = text.trim();
    let mut key = [0_u8; 32];
    for (i, part) in text.as_bytes().chunks_exact(2).enumerate() {
        key[i] = u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap();
    }
    key
}

#[test]
fn explicitly_opted_in_cli_process_rejects_pending_and_observes_external_revocation() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("runner.sqlite");
    let keyfile = tmp.path().join("runner.key");
    let db = db.to_str().unwrap();
    let keypath = keyfile.to_str().unwrap();
    assert!(cli(&["generate-key", keypath]).status.success());
    assert!(cli(&["init", db, keypath, "workstation-1", "runner-1"])
        .status
        .success());
    // An unapproved pair cannot even start an opt-in loopback listener.
    assert!(!cli(&["serve-loopback", db, keypath, "46277"])
        .status
        .success());
    assert!(!cli(&["serve-loopback", db, keypath, "0"]).status.success());
    assert!(!cli(&["serve-loopback", db, keypath, "not-a-port"])
        .status
        .success());
    assert!(cli(&["approve-pair", db, keypath]).status.success());

    let port_holder = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = port_holder.local_addr().unwrap();
    drop(port_holder); // CLI creates a separate *real process* listener.
    let mut command = Command::new(executable());
    command.args(["serve-loopback", db, keypath, &addr.port().to_string()]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = ChildGuard(command.spawn().unwrap());
    let mut socket = None;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(100)) {
            Ok(tcp) => {
                socket = Some(tcp);
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut first = socket.expect("explicit loopback CLI did not bind 127.0.0.1");
    let secret = read_private_test_key(Path::new(keypath));
    authenticate_loopback_client(&mut first, &secret).unwrap();
    let clock = SystemClock;
    let mut ws = workstation(secret);
    let job = ws
        .issue_job(
            "process-1",
            "agent-1",
            scope(),
            "Queue only; no tool",
            30_000,
            &clock,
        )
        .unwrap();
    write_frame(&mut first, &job).unwrap();
    let receipt = read_frame(&mut first).unwrap().unwrap();
    assert!(ws.receive(&receipt, &clock).unwrap().is_new());

    // A different actual CLI process revokes the pair in persistent SQLite.
    assert!(cli(&["revoke-pair", db, keypath]).status.success());
    assert!(
        RunnerStore::open(Path::new(db), secret)
            .unwrap()
            .approval_status()
            .unwrap()
            == "revoked"
    );
    // Even the original already-authenticated socket cannot submit a signed
    // cancel after the independent revocation commit. Its previously issued
    // session proof never acts as continuing authorization.
    let after_revoke_cancel = ws.issue_cancel("process-1", 30_000, &clock).unwrap();
    write_frame(&mut first, &after_revoke_cancel).unwrap();
    assert!(read_frame(&mut first).unwrap().is_none());
    drop(first);
    let mut after_revoke = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).unwrap();
    assert!(authenticate_loopback_client(&mut after_revoke, &secret).is_err());
    assert_eq!(
        RunnerStore::open(Path::new(db), secret)
            .unwrap()
            .list_jobs()
            .unwrap()[0]
            .state,
        JobState::Cancelled
    );
    // The daemon has no tool-executor API. Stop it after observing revocation.
    let _ = child.0.kill();
}
