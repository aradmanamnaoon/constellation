//! Opening a database must not leave partial migrations after a late DDL error.
use std::sync::{Arc, Barrier};

use constellation_core::{CoreError, MessageId, OperationId, RoomId, StudioStore, TaskState};
use rusqlite::Connection;

fn schema_objects(conn: &Connection) -> Vec<(String, String)> {
    let mut statement = conn
        .prepare(
            "SELECT type, name FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn late_schema_failure_rolls_back_every_new_table_and_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.sqlite");
    let connection = Connection::open(&path).unwrap();
    // Emulate an incompatible legacy schema: the first CREATE TABLE statements
    // succeed, but the final messages(room_id) index cannot be created.
    connection
        .execute_batch("CREATE TABLE messages (id TEXT PRIMARY KEY NOT NULL)")
        .unwrap();
    let original = schema_objects(&connection);
    assert_eq!(original, vec![("table".into(), "messages".into())]);

    assert!(matches!(
        StudioStore::open(&path),
        Err(CoreError::UnrecognizedSchema)
    ));
    assert_eq!(
        schema_objects(&connection),
        original,
        "schema failure must not persist earlier tables, events, or indexes"
    );
    // A later repaired startup must still be possible using the same file.
    connection.execute_batch("DROP TABLE messages").unwrap();
    let recovered = StudioStore::open(&path).unwrap();
    assert_eq!(recovered.snapshot().unwrap().rooms.len(), 0);
    assert_eq!(recovered.events().unwrap().len(), 0);
}

#[test]
fn independent_first_openers_create_one_schema_and_one_idempotent_event() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.sqlite");
    let room_id = RoomId::new();
    let operation_id = OperationId::new();
    let barrier = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let db = StudioStore::open(&path).unwrap();
                db.create_room_with_operation(room_id, "Concurrent room", operation_id)
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|task| task.join().unwrap())
        .collect();
    assert!(results.iter().all(|room| room == &results[0]));
    let reopened = StudioStore::open(path).unwrap();
    assert_eq!(reopened.snapshot().unwrap().rooms, vec![results[0].clone()]);
    assert_eq!(reopened.events().unwrap().len(), 1);
}

#[test]
fn simultaneous_first_openers_never_observe_a_mixed_preflight_schema_snapshot() {
    // Different files avoid a warmed connection or WAL cache hiding the
    // cross-statement race. Each round starts six completely independent
    // connections at the same barrier, while every opener must recognize the
    // real schema and commit exactly one idempotent room event overall.
    const ROUNDS: usize = 24;
    const OPENERS: usize = 6;
    let root = tempfile::tempdir().unwrap();
    for round in 0..ROUNDS {
        let path = root
            .path()
            .join(format!("parallel-first-open-{round}.sqlite"));
        let barrier = Arc::new(Barrier::new(OPENERS));
        let room_id = RoomId::new();
        let operation_id = OperationId::new();
        let workers: Vec<_> = (0..OPENERS)
            .map(|_| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let store = StudioStore::open(path)?;
                    store.create_room_with_operation(
                        room_id,
                        "Stable concurrent schema",
                        operation_id,
                    )
                })
            })
            .collect();
        let mut result = None;
        for worker in workers {
            let opened = worker.join().expect("opener thread must not panic");
            let room = opened.expect("a legitimate concurrent first open must not fail closed");
            if let Some(first) = &result {
                assert_eq!(first, &room);
            } else {
                result = Some(room);
            }
        }
        let reopened = StudioStore::open(&path).unwrap();
        assert_eq!(reopened.events().unwrap().len(), 1);
        assert_eq!(reopened.snapshot().unwrap().rooms, vec![result.unwrap()]);
    }
}

#[test]
fn independent_connections_cannot_both_cancel_and_complete_one_running_task() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("studio.sqlite");
    let setup = StudioStore::open(&path).unwrap();
    let room = setup.create_room("Race").unwrap();
    let agent = setup.add_agent("Writer", "A").unwrap();
    let task = setup.create_task(room.id, agent.id, "Research").unwrap();
    setup
        .transition_task(
            task.id,
            TaskState::Pending,
            TaskState::Queued,
            OperationId::new(),
            None,
        )
        .unwrap();
    setup
        .transition_task(
            task.id,
            TaskState::Queued,
            TaskState::Running,
            OperationId::new(),
            None,
        )
        .unwrap();
    let before_events = setup.events().unwrap().len();
    drop(setup);

    let barrier = Arc::new(Barrier::new(2));
    let cancel_operation = OperationId::new();
    let message_id = MessageId::new();
    let reply_operation = OperationId::new();
    let cancel_path = path.clone();
    let cancel_barrier = Arc::clone(&barrier);
    let cancel = std::thread::spawn(move || {
        let store = StudioStore::open(cancel_path).unwrap();
        cancel_barrier.wait();
        store.transition_task(
            task.id,
            TaskState::Running,
            TaskState::Cancelled,
            cancel_operation,
            None,
        )
    });
    let complete = std::thread::spawn(move || {
        let store = StudioStore::open(&path).unwrap();
        barrier.wait();
        store.complete_task_with_message(task.id, message_id, reply_operation, "Approved result")
    });
    let cancelled = cancel.join().unwrap();
    let completed = complete.join().unwrap();
    assert_ne!(
        cancelled.is_ok(),
        completed.is_ok(),
        "exactly one terminal commit"
    );
    if let Err(error) = &cancelled {
        assert!(matches!(
            error,
            CoreError::StateConflict {
                actual: TaskState::Succeeded,
                ..
            }
        ));
    }
    if let Err(error) = &completed {
        assert!(matches!(
            error,
            CoreError::StateConflict {
                actual: TaskState::Cancelled,
                ..
            }
        ));
    }
    let reopened = StudioStore::open(dir.path().join("studio.sqlite")).unwrap();
    let state = reopened.snapshot().unwrap();
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(reopened.events().unwrap().len(), before_events + 1);
    if cancelled.is_ok() {
        assert_eq!(state.tasks[0].state, TaskState::Cancelled);
        assert!(state.messages.is_empty());
    } else {
        assert_eq!(state.tasks[0].state, TaskState::Succeeded);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, "Approved result");
    }
}
