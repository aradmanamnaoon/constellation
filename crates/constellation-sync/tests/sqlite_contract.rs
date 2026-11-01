use std::{
    sync::{Arc, Barrier},
    thread,
};

use constellation_sync::{
    CasResult, DeviceId, DeviceRoster, PublishOutcome, PutResult, RevisionStore, SqliteStore,
    StoreError, SyncCoordinator, SyncError,
};
use rusqlite::Connection;

const PASSPHRASE: &[u8] = b"high entropy local offline encrypted revision passphrase";

#[test]
fn sqlite_restart_and_two_independent_devices_preserve_both_conflict_branches() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("sync.sqlite3");
    let first = SyncCoordinator::new(SqliteStore::open(&db).unwrap());
    let device_a = DeviceId::new("device-a").unwrap();
    let device_b = DeviceId::new("device-b").unwrap();
    let original = first
        .prepare(&device_a, None, b"shared ancestor", PASSPHRASE)
        .unwrap();
    assert!(matches!(
        first.publish(&original, None, PASSPHRASE).unwrap(),
        PublishOutcome::Committed { .. }
    ));
    let base = original.id();
    let a = first
        .prepare(&device_a, Some(base), b"independent edit a", PASSPHRASE)
        .unwrap();
    let b = first
        .prepare(&device_b, Some(base), b"independent edit b", PASSPHRASE)
        .unwrap();
    drop(first);

    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [a.clone(), b.clone()]
        .into_iter()
        .map(|candidate| {
            let path = db.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let store = SqliteStore::open(path).unwrap();
                let coordinator = SyncCoordinator::new(store);
                barrier.wait();
                coordinator
                    .publish(&candidate, Some(base), PASSPHRASE)
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, PublishOutcome::Committed { .. }))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, PublishOutcome::Conflict { .. }))
            .count(),
        1
    );

    let reopened = SyncCoordinator::new(SqliteStore::open(&db).unwrap());
    assert_eq!(reopened.storage().revisions_len().unwrap(), 3);
    let head = reopened.storage().head().unwrap().unwrap();
    assert!(head == a.id() || head == b.id());
    assert_eq!(
        reopened
            .fetch_revision(base, PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        b"shared ancestor"
    );
    assert_eq!(
        reopened
            .fetch_revision(a.id(), PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        b"independent edit a"
    );
    assert_eq!(
        reopened
            .fetch_revision(b.id(), PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        b"independent edit b"
    );
    assert!(reopened.verified_head(Some(base), PASSPHRASE).is_err());
    // The trusted checkpoint is stored by the caller, not in the remote head.
    // Validate a real two-connection race result after reopening SQLite.
    let mut roster = DeviceRoster::new();
    roster.authorize(device_a.clone()).unwrap();
    roster.authorize(device_b.clone()).unwrap();
    let recovered = reopened
        .recover_after_checkpoint(base, &roster, PASSPHRASE, 1)
        .unwrap();
    assert_eq!(recovered.revisions_since_checkpoint, vec![head]);
    assert_eq!(recovered.current.id, head);
    let winning_device = recovered.current.device.clone();
    assert!(roster.revoke(&winning_device));
    assert!(matches!(
        reopened.recover_after_checkpoint(base, &roster, PASSPHRASE, 1),
        Err(SyncError::UntrustedDevice)
    ));
    roster.authorize(winning_device).unwrap();
    assert_eq!(
        reopened
            .recover_after_checkpoint(base, &roster, PASSPHRASE, 1)
            .unwrap()
            .current
            .id,
        head
    );
    assert_eq!(reopened.storage().head().unwrap(), Some(head));
    // A server/storage layer can retain a readable head while damaging the
    // archived trusted ancestor. Checked recovery authenticates the anchor.
    let sabotage = Connection::open(&db).unwrap();
    sabotage
        .execute(
            "UPDATE sync_revisions SET envelope=x'00010203' WHERE id=?1",
            [base.to_hex()],
        )
        .unwrap();
    assert!(matches!(
        reopened.recover_after_checkpoint(base, &roster, PASSPHRASE, 1),
        Err(SyncError::Store(StoreError::Failed))
    ));
}

#[test]
fn sqlite_put_is_immutable_bounded_and_head_compare_and_swap_rejects_stale_writer() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("sync.sqlite3");
    let coordinator = SyncCoordinator::new(SqliteStore::open(&db).unwrap());
    let revision = coordinator
        .prepare(
            &DeviceId::new("offline-test").unwrap(),
            None,
            b"sealed research",
            PASSPHRASE,
        )
        .unwrap();
    let bytes = revision.to_bytes();
    assert_eq!(
        coordinator
            .storage()
            .put_if_absent(revision.id(), &bytes)
            .unwrap(),
        PutResult::Created
    );
    assert_eq!(
        coordinator
            .storage()
            .put_if_absent(revision.id(), &bytes)
            .unwrap(),
        PutResult::AlreadyPresent
    );
    assert!(matches!(
        coordinator
            .storage()
            .get_revision(revision.id(), bytes.len() - 1),
        Err(StoreError::SizeLimit)
    ));
    assert!(coordinator
        .storage()
        .put_if_absent(revision.id(), b"different data")
        .is_err());
    assert_eq!(
        coordinator
            .storage()
            .compare_and_swap_head(None, revision.id())
            .unwrap(),
        CasResult::Swapped
    );
    assert_eq!(
        coordinator
            .storage()
            .compare_and_swap_head(None, revision.id())
            .unwrap(),
        CasResult::Conflict {
            current: Some(revision.id())
        }
    );
    drop(coordinator);

    // Both the immutable bytes and the committed head survive reopening.
    let reopened = SyncCoordinator::new(SqliteStore::open(&db).unwrap());
    assert_eq!(reopened.storage().head().unwrap(), Some(revision.id()));
    assert_eq!(
        reopened
            .fetch_revision(revision.id(), PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        b"sealed research"
    );
    drop(reopened);

    // Corrupted bytes must never be silently returned as a valid revision.
    let direct = Connection::open(&db).unwrap();
    direct
        .execute(
            "UPDATE sync_revisions SET envelope=x'00010203' WHERE id=?1",
            [revision.id().to_hex()],
        )
        .unwrap();
    drop(direct);
    let damaged = SqliteStore::open(&db).unwrap();
    assert!(matches!(
        damaged.get_revision(revision.id(), 1024),
        Err(StoreError::Failed)
    ));
}

#[test]
fn sqlite_store_refuses_relative_paths_unknown_schema_and_existing_foreign_tables() {
    assert!(SqliteStore::open("sync.sqlite3").is_err());
    let temp = tempfile::tempdir().unwrap();
    let unknown = temp.path().join("unknown.sqlite3");
    let connection = Connection::open(&unknown).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert!(SqliteStore::open(&unknown).is_err());

    let foreign = temp.path().join("foreign.sqlite3");
    let connection = Connection::open(&foreign).unwrap();
    connection
        .execute_batch("CREATE TABLE unrelated(data TEXT NOT NULL);")
        .unwrap();
    drop(connection);
    assert!(SqliteStore::open(&foreign).is_err());
}
