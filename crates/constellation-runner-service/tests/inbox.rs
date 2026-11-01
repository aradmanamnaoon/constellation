use std::{
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};

use constellation_runner_protocol::{
    BoundPermission, Capability, Clock, Endpoint, MessagePolicy, Pair, PermissionScope,
    ProtocolError, Secret, Side, TaskAttribution,
};
use constellation_runner_service::{JobState, RunnerStore, ServiceError};
use rusqlite::Connection;

const KEY: [u8; 32] = [0x9d; 32]; // Synthetic fixture key only, never production.

struct FixedClock(i64);
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

fn scope() -> PermissionScope {
    PermissionScope {
        capability: Capability::DraftText,
        resource_id: "research-doc-1".into(),
    }
}

fn workstation() -> Endpoint {
    let mut pair = Pair::enroll(
        "workstation-1",
        "runner-1",
        Secret::from_bytes(KEY).unwrap(),
    )
    .unwrap();
    pair.approve().unwrap();
    Endpoint::new(Side::Workstation, pair, MessagePolicy::default()).unwrap()
}

fn state(db: &Path) -> RunnerStore {
    RunnerStore::open(db, KEY).unwrap()
}

fn setup() -> (tempfile::TempDir, PathBuf, RunnerStore) {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("runner.sqlite");
    let runner = RunnerStore::initialize(&db, KEY, "workstation-1", "runner-1").unwrap();
    (tmp, db, runner)
}

fn signed_job(ws: &mut Endpoint, clock: &impl Clock, id: &str) -> Vec<u8> {
    ws.issue_job(
        id,
        "agent-1",
        scope(),
        "Write a one-paragraph draft",
        30_000,
        clock,
    )
    .unwrap()
}

fn task() -> TaskAttribution {
    TaskAttribution {
        owner_id: "owner-1".into(),
        run_id: "run-1".into(),
        task_id: "task-1".into(),
    }
}

#[test]
fn lost_receipts_reconcile_after_expiry_and_restart_without_reapplying_job_or_cancel() {
    let (_tmp, db, mut store) = setup();
    store.approve_pair().unwrap();
    let mut ws = workstation();
    let before = FixedClock(1_000_000);
    let job = ws
        .issue_bound_job(
            "recover-job-1",
            "agent-1",
            BoundPermission {
                scope: scope(),
                task: task(),
            },
            "Queue this exact task once",
            30_000,
            &before,
        )
        .unwrap();
    let lost = store.accept(&job, &before).unwrap();
    assert_eq!(store.list_jobs().unwrap().len(), 1);
    drop(lost); // Simulates committed receipt lost between runner and client.
    drop(store);

    let later = FixedClock(1_100_000);
    let mut restarted = state(&db);
    assert!(matches!(
        restarted.accept(&job, &later),
        Err(ServiceError::Protocol(ProtocolError::TimeWindow))
    ));
    let recovery = ws.issue_receipt_reconciliation(1, 30_000, &later).unwrap();
    let recovered_receipt = restarted.accept(&recovery, &later).unwrap();
    assert!(ws.receive(&recovered_receipt, &later).unwrap().is_new());
    assert_eq!(store_generation(&db), 4); // job + receipt + query + new receipt
    assert_eq!(restarted.list_jobs().unwrap().len(), 1);
    assert_eq!(restarted.list_jobs().unwrap()[0].task, Some(task()));
    assert!(matches!(
        restarted.accept(&recovery, &later),
        Err(ServiceError::Protocol(ProtocolError::Replay))
    ));
    restarted
        .grant_bound_job("recover-job-1", &task(), &scope())
        .unwrap();
    assert!(restarted
        .authorized_bound_task("recover-job-1", &task(), &scope())
        .unwrap());

    let cancel = ws.issue_cancel("recover-job-1", 30_000, &later).unwrap();
    let second_lost = restarted.accept(&cancel, &later).unwrap();
    drop(second_lost);
    drop(restarted);
    let even_later = FixedClock(1_200_000);
    let mut resumed = state(&db);
    let reconcile_cancel = ws
        .issue_receipt_reconciliation(3, 30_000, &even_later)
        .unwrap();
    let cancel_ack = resumed.accept(&reconcile_cancel, &even_later).unwrap();
    assert!(ws.receive(&cancel_ack, &even_later).unwrap().is_new());
    assert_eq!(resumed.list_jobs().unwrap().len(), 1);
    assert_eq!(resumed.list_jobs().unwrap()[0].state, JobState::Cancelled);
    assert!(!resumed
        .authorized_bound_task("recover-job-1", &task(), &scope())
        .unwrap());

    // A second reconciliation signs a new sequence, and is idempotent at
    // the workstation: it cannot restore a cancelled grant.
    let again = ws
        .issue_receipt_reconciliation(3, 30_000, &even_later)
        .unwrap();
    let again_ack = resumed.accept(&again, &even_later).unwrap();
    assert!(matches!(
        ws.receive(&again_ack, &even_later),
        Ok(constellation_runner_protocol::ReceiveOutcome::DuplicateReceipt(_))
    ));
    assert_eq!(resumed.list_jobs().unwrap()[0].state, JobState::Cancelled);
}

fn store_generation(db: &Path) -> i64 {
    Connection::open(db)
        .unwrap()
        .query_row(
            "SELECT generation FROM runner_state WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn reconciliation_rejects_signed_alternate_job_digest_and_task_or_scope() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(2_000_000);
    let mut ws = workstation();
    let good = ws
        .issue_bound_job(
            "job-bound-1",
            "agent-1",
            BoundPermission {
                scope: scope(),
                task: task(),
            },
            "Original",
            30_000,
            &clock,
        )
        .unwrap();
    runner.accept(&good, &clock).unwrap();
    let first_generation = store_generation(&db);
    // Distinct same-key peers can produce fully valid MACs. The durable
    // original commitment still refuses a different signed task or content.
    for case in 0..3 {
        let mut other = workstation();
        let mut other_task = task();
        let mut other_scope = scope();
        let mut instruction = "Original";
        match case {
            0 => other_task.run_id = "different-run".into(),
            1 => other_scope.resource_id = "other-source".into(),
            _ => instruction = "Replaced instruction",
        }
        other
            .issue_bound_job(
                "job-bound-1",
                "agent-1",
                BoundPermission {
                    scope: other_scope,
                    task: other_task,
                },
                instruction,
                30_000,
                &clock,
            )
            .unwrap();
        let forged = other
            .issue_receipt_reconciliation(1, 30_000, &clock)
            .unwrap();
        assert!(matches!(
            runner.accept(&forged, &clock),
            Err(ServiceError::Protocol(ProtocolError::Conflict))
        ));
        assert_eq!(store_generation(&db), first_generation);
    }
    let valid = ws.issue_receipt_reconciliation(1, 30_000, &clock).unwrap();
    let response = runner.accept(&valid, &clock).unwrap();
    assert!(ws.receive(&response, &clock).unwrap().is_new());
    assert_eq!(runner.list_jobs().unwrap().len(), 1);
}

#[test]
fn reconciliation_rolls_back_authenticated_sequence_when_checkpoint_write_fails() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(3_000_000);
    let mut ws = workstation();
    let job = signed_job(&mut ws, &clock, "job-1");
    runner.accept(&job, &clock).unwrap();
    let recovery = ws.issue_receipt_reconciliation(1, 30_000, &clock).unwrap();
    let generation = store_generation(&db);
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_reconciliation BEFORE UPDATE OF checkpoint ON runner_state
         BEGIN SELECT RAISE(ABORT,'synthetic committed receipt disk failure'); END;",
    )
    .unwrap();
    assert!(matches!(
        runner.accept(&recovery, &clock),
        Err(ServiceError::Sql(_))
    ));
    assert_eq!(store_generation(&db), generation);
    assert_eq!(runner.list_jobs().unwrap().len(), 1);
    conn.execute_batch("DROP TRIGGER fail_reconciliation")
        .unwrap();
    drop(runner);
    let mut recovered = state(&db);
    let receipt = recovered.accept(&recovery, &clock).unwrap();
    assert!(ws.receive(&receipt, &clock).unwrap().is_new());
    assert_eq!(recovered.list_jobs().unwrap().len(), 1);
}

#[test]
fn revoked_pair_and_tampered_checkpoint_never_reconcile_a_receipt() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(4_000_000);
    let mut ws = workstation();
    runner
        .accept(&signed_job(&mut ws, &clock, "job-1"), &clock)
        .unwrap();
    let recovery = ws.issue_receipt_reconciliation(1, 30_000, &clock).unwrap();
    drop(runner);
    let conn = Connection::open(&db).unwrap();
    conn.execute("UPDATE runner_state SET checkpoint=?1", [vec![b'X'; 64]])
        .unwrap();
    assert!(matches!(
        state(&db).accept(&recovery, &clock),
        Err(ServiceError::Protocol(ProtocolError::Checkpoint))
    ));
    // Simulated corruption is separate from revocation; restore only the
    // authentic previously captured checkpoint from a fresh setup fixture.
    let (_tmp2, db2, mut runner2) = setup();
    runner2.approve_pair().unwrap();
    let mut ws2 = workstation();
    runner2
        .accept(&signed_job(&mut ws2, &clock, "job-1"), &clock)
        .unwrap();
    let recovery2 = ws2.issue_receipt_reconciliation(1, 30_000, &clock).unwrap();
    runner2.revoke_pair().unwrap();
    drop(runner2);
    assert!(matches!(
        state(&db2).accept(&recovery2, &clock),
        Err(ServiceError::Denied)
    ));
}

#[test]
fn bound_job_owner_run_task_scope_and_cancel_are_pinned_across_restart() {
    let (_tmp, db, mut store) = setup();
    store.approve_pair().unwrap();
    let clock = FixedClock(1_000_000);
    let mut ws = workstation();
    let signed = ws
        .issue_bound_job(
            "bound-job-1",
            "agent-1",
            BoundPermission {
                scope: scope(),
                task: task(),
            },
            "Bound draft only",
            30_000,
            &clock,
        )
        .unwrap();

    // All attribution fields are authenticated, so changing any one of them
    // in an otherwise well-formed frame cannot create a new inbox row.
    for (original, replacement) in [
        ("owner-1", "owner-2"),
        ("run-1", "run-2"),
        ("task-1", "task-2"),
        ("research-doc-1", "research-doc-2"),
    ] {
        let wire = String::from_utf8(signed.clone()).unwrap();
        let altered = wire.replacen(original, replacement, 1);
        assert_ne!(wire, altered);
        assert!(store.accept(altered.as_bytes(), &clock).is_err());
    }
    assert!(store.list_jobs().unwrap().is_empty());
    store.accept(&signed, &clock).unwrap();
    assert_eq!(store.list_jobs().unwrap()[0].task, Some(task()));
    assert!(matches!(
        store.grant_job("bound-job-1", &scope()),
        Err(ServiceError::Denied)
    ));
    assert!(!store
        .authorized_for_future_executor("bound-job-1", &scope())
        .unwrap());
    let mut other_task = task();
    other_task.run_id = "run-2".into();
    assert!(matches!(
        store.grant_bound_job("bound-job-1", &other_task, &scope()),
        Err(ServiceError::Denied)
    ));
    store
        .grant_bound_job("bound-job-1", &task(), &scope())
        .unwrap();
    assert!(store
        .authorized_bound_task("bound-job-1", &task(), &scope())
        .unwrap());
    assert!(!store
        .authorized_bound_task("bound-job-1", &other_task, &scope())
        .unwrap());
    drop(store);

    let mut restarted = state(&db);
    assert!(restarted
        .authorized_bound_task("bound-job-1", &task(), &scope())
        .unwrap());
    assert!(matches!(
        restarted.accept(&signed, &clock),
        Err(ServiceError::Protocol(ProtocolError::Replay))
    ));
    let cancel = ws.issue_cancel("bound-job-1", 30_000, &clock).unwrap();
    restarted.accept(&cancel, &clock).unwrap();
    assert!(!restarted
        .authorized_bound_task("bound-job-1", &task(), &scope())
        .unwrap());
    assert_eq!(restarted.list_jobs().unwrap()[0].state, JobState::Cancelled);
}

#[test]
fn version_one_database_migrates_atomically_without_losing_replay_or_grants() {
    let (_tmp, db, mut store) = setup();
    store.approve_pair().unwrap();
    let clock = FixedClock(1_000_000);
    let mut ws = workstation();
    let signed = signed_job(&mut ws, &clock, "legacy-job");
    store.accept(&signed, &clock).unwrap();
    store.grant_job("legacy-job", &scope()).unwrap();
    drop(store);
    // Recreate the exact three-column-less v1 inbox shape around the
    // committed checkpoint and jobs, simulating an installed older service.
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE old_inbox (
           job_id TEXT PRIMARY KEY, message_seq INTEGER NOT NULL UNIQUE,
           agent_id TEXT NOT NULL, capability TEXT NOT NULL,
           resource_id TEXT NOT NULL, instruction TEXT NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('pending','granted','cancelled'))
         );
         INSERT INTO old_inbox SELECT job_id,message_seq,agent_id,capability,
                                resource_id,instruction,state FROM inbox;
         DROP TABLE inbox;
         ALTER TABLE old_inbox RENAME TO inbox;
         PRAGMA user_version=1;
         COMMIT;",
    )
    .unwrap();
    drop(conn);
    let mut migrated = state(&db);
    assert_eq!(migrated.list_jobs().unwrap()[0].state, JobState::Granted);
    assert!(migrated
        .authorized_for_future_executor("legacy-job", &scope())
        .unwrap());
    assert!(matches!(
        migrated.accept(&signed, &clock),
        Err(ServiceError::Protocol(ProtocolError::Replay))
    ));
    assert_eq!(
        Connection::open(&db)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn pending_pair_denies_remote_messages_and_tool_grants_until_local_operator_approval() {
    let (_tmp, db, mut store) = setup();
    let clock = FixedClock(1_000_000);
    let mut ws = workstation();
    let wire = signed_job(&mut ws, &clock, "job-1");
    assert_eq!(store.approval_status().unwrap(), "pending");
    assert!(matches!(
        store.approved_pair_identity(),
        Err(ServiceError::Denied)
    ));
    assert!(matches!(
        store.accept(&wire, &clock),
        Err(ServiceError::Denied)
    ));
    assert!(matches!(
        store.grant_job("job-1", &scope()),
        Err(ServiceError::Denied)
    ));
    assert!(store.list_jobs().unwrap().is_empty());
    store.approve_pair().unwrap();
    assert_eq!(store.approval_status().unwrap(), "approved");
    let identity = store.approved_pair_identity().unwrap();
    assert_eq!(identity.workstation_id, "workstation-1");
    assert_eq!(identity.runner_id, "runner-1");
    assert_eq!(identity.key_epoch, 1);
    let receipt = store.accept(&wire, &clock).unwrap();
    assert!(ws.receive(&receipt, &clock).unwrap().is_new());
    let jobs = store.list_jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, JobState::Pending);
    assert_eq!(jobs[0].scope, scope());
    assert!(!store
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
    let wrong = PermissionScope {
        capability: Capability::DraftText,
        resource_id: "other-doc".into(),
    };
    assert!(matches!(
        store.grant_job("job-1", &wrong),
        Err(ServiceError::Denied)
    ));
    store.grant_job("job-1", &scope()).unwrap();
    assert!(store
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
    assert!(!store
        .authorized_for_future_executor("job-1", &wrong)
        .unwrap());
    drop(store);
    let mut recovered = state(&db);
    assert!(recovered
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
    assert!(matches!(
        recovered.accept(&wire, &clock),
        Err(ServiceError::Protocol(ProtocolError::Replay))
    ));
    recovered.revoke_grant("job-1").unwrap();
    assert!(!recovered
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
    assert!(matches!(
        recovered.revoke_grant("job-1"),
        Err(ServiceError::Denied)
    ));
}

#[test]
fn cancellation_revocation_and_replay_survive_restart() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(1_000_000);
    let mut ws = workstation();
    let job = signed_job(&mut ws, &clock, "job-1");
    runner.accept(&job, &clock).unwrap();
    runner.grant_job("job-1", &scope()).unwrap();
    let cancel = ws.issue_cancel("job-1", 20_000, &clock).unwrap();
    let receipt = runner.accept(&cancel, &clock).unwrap();
    assert!(ws.receive(&receipt, &clock).unwrap().is_new());
    assert!(!runner
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
    assert_eq!(runner.list_jobs().unwrap()[0].state, JobState::Cancelled);
    drop(runner);
    let mut restarted = state(&db);
    assert!(matches!(
        restarted.accept(&cancel, &clock),
        Err(ServiceError::Protocol(ProtocolError::Replay))
    ));
    assert!(matches!(
        restarted.grant_job("job-1", &scope()),
        Err(ServiceError::Denied)
    ));
    restarted.revoke_pair().unwrap();
    assert!(matches!(
        restarted.approved_pair_identity(),
        Err(ServiceError::Denied)
    ));
    assert!(matches!(restarted.revoke_pair(), Err(ServiceError::Denied)));
    assert_eq!(restarted.approval_status().unwrap(), "revoked");
    assert!(!restarted
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
    assert!(matches!(
        restarted.accept(&job, &clock),
        Err(ServiceError::Denied)
    ));
    assert!(matches!(
        restarted.approve_pair(),
        Err(ServiceError::Denied)
    ));
    drop(restarted);
    let restored = state(&db);
    assert_eq!(restored.approval_status().unwrap(), "revoked");
}

#[test]
fn forged_wrong_key_expired_and_oversized_frames_have_no_effect() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(2_000_000);
    let mut ws = workstation();
    let original = signed_job(&mut ws, &clock, "job-1");
    let mut forged = original.clone();
    *forged.last_mut().unwrap() = b'!';
    assert!(runner.accept(&forged, &clock).is_err());
    assert!(matches!(
        runner.accept(&original, &FixedClock(2_030_000)),
        Err(ServiceError::Protocol(ProtocolError::TimeWindow))
    ));
    assert!(runner.accept(&vec![b' '; 16_385], &clock).is_err());
    assert!(RunnerStore::open(&db, [0x51; 32]).is_err());
    assert!(runner.list_jobs().unwrap().is_empty());
    runner.accept(&original, &clock).unwrap();
    assert_eq!(runner.list_jobs().unwrap().len(), 1);
}

#[test]
fn failed_inbox_insert_rolls_back_replay_ledger_and_allows_authenticated_retry() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(3_000_000);
    let mut ws = workstation();
    let job = signed_job(&mut ws, &clock, "job-1");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER refuse_inbox BEFORE INSERT ON inbox
         BEGIN SELECT RAISE(ABORT,'synthetic disk write failure'); END;",
    )
    .unwrap();
    let before: i64 = conn
        .query_row(
            "SELECT generation FROM runner_state WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(matches!(
        runner.accept(&job, &clock),
        Err(ServiceError::Sql(_))
    ));
    assert!(runner.list_jobs().unwrap().is_empty());
    let after: i64 = conn
        .query_row(
            "SELECT generation FROM runner_state WHERE id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        before, after,
        "failed queue write cannot advance replay generation"
    );
    conn.execute_batch("DROP TRIGGER refuse_inbox").unwrap();
    drop(runner);
    let mut recovered = state(&db);
    recovered.accept(&job, &clock).unwrap();
    assert_eq!(recovered.list_jobs().unwrap().len(), 1);
}

#[test]
fn concurrent_independent_connections_accept_job_exactly_once() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    drop(runner);
    let clock = FixedClock(4_000_000);
    let mut ws = workstation();
    let job = signed_job(&mut ws, &clock, "job-1");
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = db.clone();
            let frame = job.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut runner = state(&path);
                barrier.wait();
                runner.accept(&frame, &FixedClock(4_000_000))
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|x| x.is_ok()).count(), 1);
    assert!(outcomes
        .iter()
        .any(|x| matches!(x, Err(ServiceError::Protocol(ProtocolError::Replay)))));
    let recovered = state(&db);
    assert_eq!(recovered.list_jobs().unwrap().len(), 1);
}

#[test]
fn concurrent_local_grant_and_authenticated_cancel_end_cancelled() {
    let (_tmp, db, mut runner) = setup();
    runner.approve_pair().unwrap();
    let clock = FixedClock(5_000_000);
    let mut ws = workstation();
    let job = signed_job(&mut ws, &clock, "job-1");
    runner.accept(&job, &clock).unwrap();
    let cancel = ws.issue_cancel("job-1", 30_000, &clock).unwrap();
    drop(runner);
    let barrier = Arc::new(Barrier::new(2));
    let grant_db = db.clone();
    let grant_barrier = Arc::clone(&barrier);
    let granting = std::thread::spawn(move || {
        let mut store = state(&grant_db);
        grant_barrier.wait();
        store.grant_job("job-1", &scope())
    });
    let cancelling = std::thread::spawn(move || {
        let mut store = state(&db);
        barrier.wait();
        store.accept(&cancel, &FixedClock(5_000_000))
    });
    let grant_result = granting.join().unwrap();
    let cancellation = cancelling.join().unwrap();
    assert!(cancellation.is_ok(), "signed cancellation cannot be lost");
    assert!(grant_result.is_ok() || matches!(grant_result, Err(ServiceError::Denied)));
    let recovered = state(&_tmp.path().join("runner.sqlite"));
    assert_eq!(recovered.list_jobs().unwrap()[0].state, JobState::Cancelled);
    assert!(!recovered
        .authorized_for_future_executor("job-1", &scope())
        .unwrap());
}

#[test]
fn foreign_database_is_rejected_without_schema_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("foreign.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE private_data (secret TEXT); INSERT INTO private_data VALUES('kept');",
    )
    .unwrap();
    drop(conn);
    assert!(matches!(
        RunnerStore::open(&path, KEY),
        Err(ServiceError::Schema)
    ));
    let conn = Connection::open(&path).unwrap();
    let secret: String = conn
        .query_row("SELECT secret FROM private_data", [], |row| row.get(0))
        .unwrap();
    assert_eq!(secret, "kept");
}
