//! A durable, fail-closed attempt reservation made before any billable
//! proposal request. The counters live in the same SQLite file as StudioStore
//! and therefore survive app restart and encrypted project backup/recovery.
use std::{path::Path, time::Duration};

use constellation_agent_loop::MAX_MODEL_ATTEMPTS;
use constellation_core::{PROPOSAL_BUDGET_DDL, verify_initialized_schema};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

// This caps the number of file-proposal requests per configured provider ID
// across all tasks in one local workspace. It is a request cap, NOT a monetary
// limit: provider pricing and billable output vary and are not verifiable here.
pub(crate) const MAX_PROVIDER_PROPOSAL_REQUESTS: i64 = 128;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RequestUsage {
    pub task_used: i64,
    pub provider_used: i64,
    pub task_limit: i64,
    pub provider_limit: i64,
}

/// Read the same durable limits as the reservation path without creating a
/// database, modifying either counter, or exposing credentials to the UI.
/// A partially initialized or malformed schema fails closed.
pub(crate) fn request_usage(
    workspace_database: &Path,
    task_id: &str,
    provider_id: &str,
) -> Result<RequestUsage, String> {
    if task_id.len() != 36 || provider_id.len() != 36 {
        return Err(denied());
    }
    let mut db = Connection::open_with_flags(workspace_database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| denied())?;
    db.busy_timeout(Duration::from_secs(4))
        .map_err(|_| denied())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| denied())?;
    // Read under one database snapshot and reject renamed/altered budget
    // columns or a foreign schema, not merely absent/present table names.
    verify_initialized_schema(&tx).map_err(|_| denied())?;
    let tables: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN
             ('file_proposal_task_budget', 'file_proposal_provider_budget')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| denied())?;
    if tables != 0 && tables != 2 {
        return Err(denied());
    }
    let (task_used, provider_used) = if tables == 0 {
        (0, 0)
    } else {
        let task_used: i64 = tx
            .query_row(
                "SELECT attempts FROM file_proposal_task_budget WHERE task_id = ?1",
                [task_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| denied())?
            .unwrap_or(0);
        let provider_used: i64 = tx
            .query_row(
                "SELECT attempts FROM file_proposal_provider_budget WHERE provider_id = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| denied())?
            .unwrap_or(0);
        (task_used, provider_used)
    };
    if !(0..=MAX_MODEL_ATTEMPTS as i64).contains(&task_used)
        || !(0..=MAX_PROVIDER_PROPOSAL_REQUESTS).contains(&provider_used)
    {
        return Err(denied());
    }
    let usage = RequestUsage {
        task_used,
        provider_used,
        task_limit: MAX_MODEL_ATTEMPTS as i64,
        provider_limit: MAX_PROVIDER_PROPOSAL_REQUESTS,
    };
    tx.commit().map_err(|_| denied())?;
    Ok(usage)
}

fn denied() -> String {
    "The durable model proposal request budget is exhausted or unavailable".into()
}

/// Atomic two-scope reservation. Any SQL error, lock timeout, malformed
/// counter or corruption blocks the network request. Never decrement counters
/// on HTTP errors or cancellation: the remote request may have been billed.
pub(crate) fn reserve_request(
    workspace_database: &Path,
    task_id: &str,
    provider_id: &str,
) -> Result<(), String> {
    if task_id.len() != 36 || provider_id.len() != 36 {
        return Err(denied());
    }
    let mut db = Connection::open(workspace_database).map_err(|_| denied())?;
    db.busy_timeout(Duration::from_secs(4))
        .map_err(|_| denied())?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| denied())?;
    // The model request has not been dispatched yet. Fail closed on a
    // corrupted, foreign or legacy-unmigrated Core database, including a
    // budget table recreated by another process after app startup.
    verify_initialized_schema(&tx).map_err(|_| denied())?;
    // On a freshly initialized workspace, CREATE TABLE must be committed
    // atomically with *both* request counters. Previously a failed second
    // CREATE or exhausted request left a half-written extension, which Core's
    // strict schema admission correctly refuses on the next app restart.
    tx.execute_batch(PROPOSAL_BUDGET_DDL)
        .map_err(|_| denied())?;
    verify_initialized_schema(&tx).map_err(|_| denied())?;
    let task_attempts: Option<i64> = tx
        .query_row(
            "SELECT attempts FROM file_proposal_task_budget WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| denied())?;
    let provider_attempts: Option<i64> = tx
        .query_row(
            "SELECT attempts FROM file_proposal_provider_budget WHERE provider_id = ?1",
            params![provider_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| denied())?;
    if task_attempts.unwrap_or(0) >= MAX_MODEL_ATTEMPTS as i64
        || provider_attempts.unwrap_or(0) >= MAX_PROVIDER_PROPOSAL_REQUESTS
        || task_attempts.is_some_and(|count| count < 1)
        || provider_attempts.is_some_and(|count| count < 1)
    {
        return Err(denied());
    }
    tx.execute(
        "INSERT INTO file_proposal_task_budget (task_id, attempts) VALUES (?1, 1)
         ON CONFLICT(task_id) DO UPDATE SET attempts = attempts + 1",
        params![task_id],
    )
    .map_err(|_| denied())?;
    tx.execute(
        "INSERT INTO file_proposal_provider_budget (provider_id, attempts) VALUES (?1, 1)
         ON CONFLICT(provider_id) DO UPDATE SET attempts = attempts + 1",
        params![provider_id],
    )
    .map_err(|_| denied())?;
    tx.commit().map_err(|_| denied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_core::StudioStore;
    use std::sync::{Arc, Barrier};

    const TASK_A: &str = "11111111-1111-4111-8111-111111111111";
    const TASK_B: &str = "22222222-2222-4222-8222-222222222222";
    const PROVIDER_A: &str = "33333333-3333-4333-8333-333333333333";

    fn create_core_database(path: &Path) {
        drop(StudioStore::open(path).expect("real Core schema is required before model use"));
    }

    #[test]
    fn usage_is_read_only_and_matches_durable_reservations() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        assert!(request_usage(&db, TASK_A, PROVIDER_A).is_err());
        assert!(!db.exists(), "a status query must not create the DB");
        create_core_database(&db);
        let empty = request_usage(&db, TASK_A, PROVIDER_A).unwrap();
        assert_eq!((empty.task_used, empty.provider_used), (0, 0));
        reserve_request(&db, TASK_A, PROVIDER_A).unwrap();
        reserve_request(&db, TASK_A, PROVIDER_A).unwrap();
        let used = request_usage(&db, TASK_A, PROVIDER_A).unwrap();
        assert_eq!((used.task_used, used.provider_used), (2, 2));
        assert_eq!((used.task_limit, used.provider_limit), (16, 128));
        assert_eq!(request_usage(&db, TASK_B, PROVIDER_A).unwrap().task_used, 0);
        assert_eq!(request_usage(&db, TASK_A, PROVIDER_A).unwrap(), used);
    }

    #[test]
    fn usage_fails_on_partial_or_malformed_counter_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        let conn = Connection::open(&db).unwrap();
        conn.execute(
            "CREATE TABLE file_proposal_task_budget (task_id TEXT, attempts INTEGER)",
            [],
        )
        .unwrap();
        assert!(request_usage(&db, TASK_A, PROVIDER_A).is_err());
        conn.execute(
            "CREATE TABLE file_proposal_provider_budget (provider_id TEXT, attempts INTEGER)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO file_proposal_task_budget VALUES (?1, 999)",
            [TASK_A],
        )
        .unwrap();
        assert!(request_usage(&db, TASK_A, PROVIDER_A).is_err());
    }

    #[test]
    fn attempt_reservations_survive_restart_and_exhaustion_is_irreversible() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        create_core_database(&db);
        for _ in 0..MAX_MODEL_ATTEMPTS {
            reserve_request(&db, TASK_A, PROVIDER_A).unwrap();
        }
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        // A new connection simulates a restarted desktop application.
        let before = Connection::open(&db).unwrap();
        assert_eq!(
            before
                .query_row(
                    "SELECT attempts FROM file_proposal_task_budget WHERE task_id = ?1",
                    [TASK_A],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            MAX_MODEL_ATTEMPTS as i64
        );
        drop(before);
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        reserve_request(&db, TASK_B, PROVIDER_A).unwrap();
        let after = Connection::open(&db).unwrap();
        assert_eq!(
            after
                .query_row(
                    "SELECT attempts FROM file_proposal_task_budget WHERE task_id = ?1",
                    [TASK_A],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            MAX_MODEL_ATTEMPTS as i64
        );
    }

    #[test]
    fn concurrent_attempts_cannot_overrun_task_quota() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(dir.path().join("studio.sqlite3"));
        create_core_database(&db);
        // Provision tables before racing to isolate the transactional check.
        reserve_request(&db, TASK_A, PROVIDER_A).unwrap();
        let workers = 6;
        let gate = Arc::new(Barrier::new(workers));
        let joins: Vec<_> = (0..workers)
            .map(|_| {
                let db = Arc::clone(&db);
                let gate = Arc::clone(&gate);
                std::thread::spawn(move || {
                    gate.wait();
                    (0..8)
                        .filter(|_| reserve_request(&db, TASK_A, PROVIDER_A).is_ok())
                        .count()
                })
            })
            .collect();
        let admitted: usize = joins.into_iter().map(|worker| worker.join().unwrap()).sum();
        assert_eq!(admitted, usize::from(MAX_MODEL_ATTEMPTS - 1));
        let db = Connection::open(&*db).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT attempts FROM file_proposal_task_budget WHERE task_id = ?1",
                [TASK_A],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            MAX_MODEL_ATTEMPTS as i64
        );
    }

    #[test]
    fn provider_cap_is_durable_across_multiple_distinct_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        create_core_database(&db);
        for n in 0..MAX_PROVIDER_PROPOSAL_REQUESTS {
            let task = format!("{n:08x}-0000-4000-8000-000000000000");
            reserve_request(&db, &task, PROVIDER_A).unwrap();
        }
        assert!(reserve_request(&db, TASK_B, PROVIDER_A).is_err());
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT attempts FROM file_proposal_provider_budget WHERE provider_id = ?1",
                [PROVIDER_A],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            MAX_PROVIDER_PROPOSAL_REQUESTS
        );
    }

    #[test]
    fn rejected_first_reservation_does_not_leave_half_created_budget_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE file_proposal_task_budget (
                task_id TEXT PRIMARY KEY NOT NULL,
                attempts INTEGER NOT NULL
             );
             INSERT INTO file_proposal_task_budget VALUES (
                '11111111-1111-4111-8111-111111111111', 999
             );",
        )
        .unwrap();
        drop(conn);
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        let conn = Connection::open(&db).unwrap();
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'
                   AND name='file_proposal_provider_budget'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            tables, 0,
            "a refused reservation may not commit half the schema"
        );
        assert_eq!(
            conn.query_row(
                "SELECT attempts FROM file_proposal_task_budget WHERE task_id=?1",
                [TASK_A],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            999
        );
    }

    #[test]
    fn real_core_workspace_reopens_after_provider_reservation_without_losing_task() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        let core = StudioStore::open(&db).unwrap();
        let room = core.create_room("Before billable request").unwrap();
        drop(core);

        reserve_request(&db, TASK_A, PROVIDER_A).unwrap();
        let reopened = StudioStore::open(&db)
            .expect("desktop must reopen the actual Core database after reservation");
        assert!(reopened.snapshot().unwrap().rooms.contains(&room));
        drop(reopened);
        assert_eq!(request_usage(&db, TASK_A, PROVIDER_A).unwrap().task_used, 1);

        // An older, unversioned real Core store follows the same migration.
        let direct = Connection::open(&db).unwrap();
        direct.pragma_update(None, "user_version", 0).unwrap();
        drop(direct);
        StudioStore::open(&db).expect("legacy Core store with budgets is migratable");
        assert_eq!(request_usage(&db, TASK_A, PROVIDER_A).unwrap().task_used, 1);
    }

    #[test]
    fn lookalike_budget_schema_cannot_bypass_billed_request_limits() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        create_core_database(&db);
        reserve_request(&db, TASK_A, PROVIDER_A).unwrap();

        // Existing table names and valid-looking rows are insufficient. A
        // different process can replace the CHECK DDL between app startup
        // and the next provider request; the network request must be denied.
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "DROP TABLE file_proposal_task_budget;
             CREATE TABLE file_proposal_task_budget (
               task_id TEXT PRIMARY KEY NOT NULL,
               attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 999)
             );
             INSERT INTO file_proposal_task_budget VALUES (
               '11111111-1111-4111-8111-111111111111', 1
             );",
        )
        .unwrap();
        assert!(request_usage(&db, TASK_A, PROVIDER_A).is_err());
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT attempts FROM file_proposal_provider_budget WHERE provider_id=?1",
                [PROVIDER_A],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "a rejected request cannot charge or reset the provider budget"
        );
    }

    #[test]
    fn empty_foreign_and_unmigrated_databases_never_authorize_provider_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("studio.sqlite3");
        Connection::open(&db).unwrap();
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        assert!(request_usage(&db, TASK_A, PROVIDER_A).is_err());
        create_core_database(&db);
        let conn = Connection::open(&db).unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        drop(conn);
        create_core_database(&db); // Explicit audited migration path.
        reserve_request(&db, TASK_A, PROVIDER_A).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE foreign_ledger(data TEXT)")
            .unwrap();
        assert!(reserve_request(&db, TASK_A, PROVIDER_A).is_err());
        assert!(request_usage(&db, TASK_A, PROVIDER_A).is_err());
    }
}
