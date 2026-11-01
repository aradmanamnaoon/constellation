//! Previously unversioned exact Core stores are migrated without data loss;
//! foreign/future layouts must be rejected without changing their schema.
use std::sync::{Arc, Barrier};

use constellation_core::{
    verify_initialized_schema, CoreError, MessageSender, OperationId, RoomId, StudioStore,
    EVIDENCE_LIBRARY_DDL, PROPOSAL_BUDGET_DDL,
};
use rusqlite::Connection;

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

fn schema_snapshot(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut stmt = conn
        .prepare(
            "SELECT type, name, sql FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .unwrap();
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn fresh_database_is_versioned_and_reopen_does_not_rewrite_schema_or_events() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state.db");
    let store = StudioStore::open(&path).unwrap();
    let room = store.create_room("Persisted").unwrap();
    let message = store
        .append_message(room.id, MessageSender::User, "Preserved content")
        .unwrap();
    let before = store.snapshot().unwrap();
    let events = store.events().unwrap();
    drop(store);
    let direct = Connection::open(&path).unwrap();
    assert_eq!(user_version(&direct), 1);
    let schema = schema_snapshot(&direct);
    drop(direct);
    let reopened = StudioStore::open(&path).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), before);
    assert_eq!(reopened.events().unwrap(), events);
    assert_eq!(reopened.snapshot().unwrap().messages[0], message);
    drop(reopened);
    let direct = Connection::open(&path).unwrap();
    assert_eq!(schema_snapshot(&direct), schema);
    assert_eq!(user_version(&direct), 1);
}

#[test]
fn exact_unversioned_legacy_store_is_upgraded_in_place_with_rows_and_audit_history() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.db");
    let legacy = StudioStore::open(&path).unwrap();
    let room = legacy.create_room("Unversioned document").unwrap();
    let agent = legacy.add_agent("Researcher", "Agent").unwrap();
    legacy
        .append_message(room.id, MessageSender::Agent(agent.id), "Original")
        .unwrap();
    legacy
        .create_task(room.id, agent.id, "Keep my task")
        .unwrap();
    let snapshot = legacy.snapshot().unwrap();
    let events = legacy.events().unwrap();
    drop(legacy);
    let direct = Connection::open(&path).unwrap();
    let prior_schema = schema_snapshot(&direct);
    direct.pragma_update(None, "user_version", 0).unwrap();
    assert_eq!(user_version(&direct), 0);
    drop(direct);
    let migrated = StudioStore::open(&path).unwrap();
    assert_eq!(migrated.snapshot().unwrap(), snapshot);
    assert_eq!(migrated.events().unwrap(), events);
    drop(migrated);
    let direct = Connection::open(&path).unwrap();
    assert_eq!(user_version(&direct), 1);
    assert_eq!(schema_snapshot(&direct), prior_schema);
    drop(direct);
    // Repeated opens cannot remigrate or duplicate any state/audit rows.
    let reopened = StudioStore::open(&path).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), snapshot);
    assert_eq!(reopened.events().unwrap(), events);
}

#[test]
fn unversioned_foreign_and_partial_schemas_are_rejected_without_wal_or_ddl() {
    for sql in [
        "CREATE TABLE stranger (id INTEGER PRIMARY KEY, secret TEXT);
         INSERT INTO stranger VALUES (1, 'sensitive');",
        "CREATE TABLE rooms (id TEXT PRIMARY KEY NOT NULL, data TEXT NOT NULL);",
        "CREATE TABLE messages (id TEXT PRIMARY KEY NOT NULL);",
        // A dropped foreign AUTOINCREMENT table can leave sqlite_sequence
        // behind. No user-visible tables does not mean this file is new.
        "CREATE TABLE dropped (id INTEGER PRIMARY KEY AUTOINCREMENT);
         DROP TABLE dropped;",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("foreign.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(sql).unwrap();
        let prior_schema = schema_snapshot(&conn);
        let prior_journal: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(prior_journal, "delete");
        assert_eq!(user_version(&conn), 0);
        drop(conn);

        assert!(matches!(
            StudioStore::open(&path),
            Err(CoreError::UnrecognizedSchema)
        ));
        let reopened = Connection::open(&path).unwrap();
        assert_eq!(schema_snapshot(&reopened), prior_schema);
        assert_eq!(user_version(&reopened), 0);
        let journal: String = reopened
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(journal, prior_journal);
        if sql.contains("stranger") {
            let value: String = reopened
                .query_row("SELECT secret FROM stranger WHERE id=1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(value, "sensitive");
        }
    }
}

#[test]
fn future_version_and_modified_v1_layout_are_denied_without_mutations() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("future.db");
    let store = StudioStore::open(&path).unwrap();
    let room = store.create_room("Do not rewrite").unwrap();
    drop(store);
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 88).unwrap();
    let schema = schema_snapshot(&conn);
    assert!(matches!(
        StudioStore::open(&path),
        Err(CoreError::UnsupportedSchemaVersion(88))
    ));
    assert_eq!(user_version(&conn), 88);
    assert_eq!(schema_snapshot(&conn), schema);
    let data: String = conn
        .query_row(
            "SELECT data FROM rooms WHERE id=?1",
            [room.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(data.contains("Do not rewrite"));

    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute_batch("DROP INDEX messages_room_index")
        .unwrap();
    let changed = schema_snapshot(&conn);
    assert!(matches!(
        StudioStore::open(&path),
        Err(CoreError::UnrecognizedSchema)
    ));
    assert_eq!(user_version(&conn), 1);
    assert_eq!(schema_snapshot(&conn), changed);
}

#[test]
fn concurrent_legacy_upgrade_and_idempotent_mutations_converge() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("concurrent.db");
    let store = StudioStore::open(&path).unwrap();
    let existing = store.create_room("Existing").unwrap();
    drop(store);
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 0).unwrap();
    drop(conn);
    let room_id = RoomId::new();
    let operation_id = OperationId::new();
    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let store = StudioStore::open(&path).unwrap();
                store
                    .create_room_with_operation(room_id, "Same new room", operation_id)
                    .unwrap()
            })
        })
        .collect();
    let rooms: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(rooms.iter().all(|room| room == &rooms[0]));
    let reopened = StudioStore::open(&path).unwrap();
    let snapshot = reopened.snapshot().unwrap();
    assert_eq!(snapshot.rooms.len(), 2);
    assert!(snapshot.rooms.contains(&existing));
    assert!(snapshot.rooms.contains(&rooms[0]));
    assert_eq!(reopened.events().unwrap().len(), 2);
    let direct = Connection::open(&path).unwrap();
    assert_eq!(user_version(&direct), 1);
}

#[test]
fn blocked_legacy_migration_exhausts_bounded_busy_policy_without_partial_version() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("writer-locked.db");
    let store = StudioStore::open(&path).unwrap();
    let existing = store.create_room("Do not lose me").unwrap();
    drop(store);
    let lock_holder = Connection::open(&path).unwrap();
    lock_holder.pragma_update(None, "user_version", 0).unwrap();
    lock_holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    // The first failed migration must not claim version 1 or change rows
    // while another independent writer is holding the SQLite write lock.
    assert!(matches!(
        StudioStore::open(&path),
        Err(CoreError::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked,
                ..
            },
            _
        )))
    ));
    assert_eq!(user_version(&lock_holder), 0);
    let name: String = lock_holder
        .query_row(
            "SELECT data FROM rooms WHERE id = ?1",
            [existing.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(name.contains("Do not lose me"));
    lock_holder.execute_batch("ROLLBACK").unwrap();
    drop(lock_holder);
    let reopened = StudioStore::open(&path).unwrap();
    assert_eq!(reopened.snapshot().unwrap().rooms, vec![existing]);
    drop(reopened);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), 1);
}

#[test]
fn desktop_proposal_budget_tables_survive_restart_and_legacy_upgrade() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget-restart.db");
    let store = StudioStore::open(&path).unwrap();
    let room = store.create_room("Keep durable task budget").unwrap();
    drop(store);
    // These two exact tables are created by the native model proposal budget
    // before a billable provider request, in Core's *same* SQLite file.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS file_proposal_task_budget (
           task_id TEXT PRIMARY KEY NOT NULL,
           attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 16)
         );
         CREATE TABLE IF NOT EXISTS file_proposal_provider_budget (
           provider_id TEXT PRIMARY KEY NOT NULL,
           attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 128)
         );
         INSERT INTO file_proposal_task_budget VALUES ('test-task', 3);
         INSERT INTO file_proposal_provider_budget VALUES ('test-provider', 4);",
    )
    .unwrap();
    drop(conn);
    let reopened =
        StudioStore::open(&path).expect("valid desktop budget tables must not brick startup");
    assert!(reopened.snapshot().unwrap().rooms.contains(&room));
    drop(reopened);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT attempts FROM file_proposal_task_budget WHERE task_id='test-task'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    conn.pragma_update(None, "user_version", 0).unwrap();
    drop(conn);
    StudioStore::open(&path).expect("real unversioned desktop database also migrates");
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), 1);
    assert_eq!(
        conn.query_row(
            "SELECT attempts FROM file_proposal_provider_budget WHERE provider_id='test-provider'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
}

#[test]
fn partial_or_redefined_desktop_budget_extension_is_not_accepted_as_known_schema() {
    for extension in [
        "CREATE TABLE file_proposal_task_budget (
            task_id TEXT PRIMARY KEY NOT NULL,
            attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 16)
         );",
        "CREATE TABLE file_proposal_task_budget (
            task_id TEXT PRIMARY KEY NOT NULL,
            attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 999)
         );
         CREATE TABLE file_proposal_provider_budget (
            provider_id TEXT PRIMARY KEY NOT NULL,
            attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 128)
         );",
        "CREATE VIEW file_proposal_task_budget AS SELECT 1 attempts, 'x' task_id;
         CREATE TABLE file_proposal_provider_budget (
            provider_id TEXT PRIMARY KEY NOT NULL,
            attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 128)
         );",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("modified-budget.db");
        let store = StudioStore::open(&path).unwrap();
        drop(store);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(extension).unwrap();
        let previous = schema_snapshot(&conn);
        drop(conn);
        assert!(matches!(
            StudioStore::open(&path),
            Err(CoreError::UnrecognizedSchema)
        ));
        let reopened = Connection::open(&path).unwrap();
        assert_eq!(schema_snapshot(&reopened), previous);
        assert_eq!(user_version(&reopened), 1);
    }
}

#[test]
fn saved_evidence_and_request_budget_extensions_survive_restart_and_legacy_migration() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("evidence-budget.db");
    let core = StudioStore::open(&path).unwrap();
    let room = core
        .create_room("Keep research and request limits")
        .unwrap();
    drop(core);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(EVIDENCE_LIBRARY_DDL).unwrap();
    conn.execute(
        "INSERT INTO evidence_sources
         (source_id,title,source_format,source_text,source_sha256,source_bytes)
         VALUES('research-1','Notes','markdown','A citation.','fixture-digest',11)",
        [],
    )
    .unwrap();
    verify_initialized_schema(&conn).unwrap();
    drop(conn);
    let core = StudioStore::open(&path).unwrap();
    assert!(core.snapshot().unwrap().rooms.contains(&room));
    drop(core);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(PROPOSAL_BUDGET_DDL).unwrap();
    conn.execute(
        "INSERT INTO file_proposal_task_budget VALUES('request-1', 7)",
        [],
    )
    .unwrap();
    verify_initialized_schema(&conn).unwrap();
    conn.pragma_update(None, "user_version", 0).unwrap();
    drop(conn);
    let core = StudioStore::open(&path).expect("both optional tables must survive v0 migration");
    assert!(core.snapshot().unwrap().rooms.contains(&room));
    drop(core);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), 1);
    assert_eq!(
        conn.query_row("SELECT source_text FROM evidence_sources", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "A citation."
    );
    assert_eq!(
        conn.query_row("SELECT attempts FROM file_proposal_task_budget", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        7
    );
}

#[test]
fn malformed_evidence_extension_cannot_be_adopted_even_with_valid_budget_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("untrusted-evidence.db");
    drop(StudioStore::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(PROPOSAL_BUDGET_DDL).unwrap();
    conn.execute_batch(
        "CREATE TABLE evidence_sources (
           source_id TEXT PRIMARY KEY NOT NULL,
           title TEXT NOT NULL,
           source_format TEXT,
           source_text TEXT,
           source_sha256 TEXT,
           source_bytes INTEGER
         );",
    )
    .unwrap();
    let before = schema_snapshot(&conn);
    assert!(matches!(
        verify_initialized_schema(&conn),
        Err(CoreError::UnrecognizedSchema)
    ));
    drop(conn);
    assert!(matches!(
        StudioStore::open(&path),
        Err(CoreError::UnrecognizedSchema)
    ));
    let conn = Connection::open(&path).unwrap();
    assert_eq!(schema_snapshot(&conn), before);
}
