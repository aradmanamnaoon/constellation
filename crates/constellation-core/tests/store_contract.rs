use constellation_core::{
    AgentId, CoreError, CredentialSource, EventKind, MessageId, MessageSender, OperationId,
    ProviderId, ProviderKind, ProviderProfile, RoomId, StudioStore, TaskId, TaskState,
};

fn store() -> (tempfile::TempDir, StudioStore) {
    let temp = tempfile::tempdir().expect("temp dir");
    let database = temp.path().join("config").join("studio.sqlite");
    let store = StudioStore::open(database).expect("open");
    (temp, store)
}

#[test]
fn room_team_members_are_durable_audited_idempotent_and_legacy_compatible() {
    let (temp, db) = store();
    let room = db.create_room("Research").unwrap();
    assert_eq!(room.member_ids, None);
    let a = db.add_agent("Reviewer", "Alice").unwrap();
    let b = db.add_agent("Planner", "Bob").unwrap();
    let op = OperationId::new();
    let updated = db
        .set_room_members(room.id, vec![a.id, b.id], None, op)
        .unwrap();
    assert_eq!(updated.member_ids, Some(vec![a.id, b.id]));
    assert_eq!(
        db.set_room_members(room.id, vec![a.id, b.id], None, op)
            .unwrap(),
        updated
    );
    assert!(matches!(
        db.set_room_members(room.id, vec![b.id], None, OperationId::new()),
        Err(CoreError::OperationConflict)
    ));
    assert!(matches!(
        db.set_room_members(
            room.id,
            vec![a.id, a.id],
            Some(vec![a.id, b.id]),
            OperationId::new()
        ),
        Err(CoreError::InvalidInput(_))
    ));
    assert!(matches!(
        db.set_room_members(
            room.id,
            vec![AgentId::new()],
            Some(vec![a.id, b.id]),
            OperationId::new()
        ),
        Err(CoreError::NotFound("agent"))
    ));
    assert!(matches!(db.events().unwrap().last().unwrap().kind,
        EventKind::RoomMembersChanged { room_id, ref member_ids }
        if room_id == room.id && member_ids == &vec![a.id, b.id]));
    drop(db);
    let reopened = StudioStore::open(temp.path().join("config/studio.sqlite")).unwrap();
    assert_eq!(
        reopened.snapshot().unwrap().rooms[0].member_ids,
        Some(vec![a.id, b.id])
    );
    let cleared = reopened
        .set_room_members(room.id, vec![], Some(vec![a.id, b.id]), OperationId::new())
        .unwrap();
    assert_eq!(cleared.member_ids, Some(vec![]));
    assert!(matches!(
        reopened.set_room_members(
            room.id,
            vec![b.id],
            Some(vec![a.id, b.id]),
            OperationId::new()
        ),
        Err(CoreError::OperationConflict)
    ));
}

#[test]
fn unused_credentialless_provider_removal_is_audited_exactly_once() {
    let (temp, db) = store();
    let provider = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Disposable test".into(),
        model_id: "test-model".into(),
        base_url: "http://localhost:11434/v1".into(),
        kind: ProviderKind::OpenaiChat,
        credentials: CredentialSource::None,
    };
    db.save_provider_profile(provider.clone()).unwrap();
    let op = OperationId::new();
    db.remove_unused_provider(provider.clone(), op).unwrap();
    db.remove_unused_provider(provider.clone(), op).unwrap();
    assert!(db.snapshot().unwrap().providers.is_empty());
    assert!(matches!(db.events().unwrap().last().unwrap().kind,
        EventKind::ProviderRemoved { provider_id } if provider_id == provider.id));
    assert!(matches!(
        db.remove_unused_provider(provider.clone(), OperationId::new()),
        Err(CoreError::NotFound("provider"))
    ));
    drop(db);
    let reopened = StudioStore::open(temp.path().join("config/studio.sqlite")).unwrap();
    assert!(reopened.snapshot().unwrap().providers.is_empty());
}

#[test]
fn unused_provider_removal_rejects_stale_credential_and_referenced_profiles() {
    let (_, db) = store();
    let profile = ProviderProfile {
        id: ProviderId::new(),
        display_name: "Keep original".into(),
        model_id: "test".into(),
        base_url: "http://localhost:11434/v1".into(),
        kind: ProviderKind::OpenaiChat,
        credentials: CredentialSource::None,
    };
    db.save_provider_profile(profile.clone()).unwrap();
    let before = db.events().unwrap().len();
    let mut stale = profile.clone();
    stale.model_id = "old-model".into();
    assert!(matches!(
        db.remove_unused_provider(stale, OperationId::new()),
        Err(CoreError::OperationConflict)
    ));
    let mut secret = profile.clone();
    secret.credentials = CredentialSource::OsKeyring {
        service: "example".into(),
        account: "example".into(),
    };
    assert!(matches!(
        db.remove_unused_provider(secret, OperationId::new()),
        Err(CoreError::InvalidInput(_))
    ));
    assert_eq!(db.events().unwrap().len(), before);

    let agent = db.add_agent("Reviewer", "Researcher").unwrap();
    db.set_agent_provider(agent.id, Some(profile.id), OperationId::new())
        .unwrap();
    assert!(matches!(
        db.remove_unused_provider(profile.clone(), OperationId::new()),
        Err(CoreError::InvalidInput(_))
    ));
    db.set_agent_provider(agent.id, None, OperationId::new())
        .unwrap();
    let room = db.create_room("Prior work").unwrap();
    db.append_message(room.id, MessageSender::User, "Historical question")
        .unwrap();
    assert!(matches!(
        db.remove_unused_provider(profile.clone(), OperationId::new()),
        Err(CoreError::InvalidInput(_))
    ));
    assert_eq!(db.snapshot().unwrap().providers, vec![profile]);
}

#[test]
fn agent_provider_preference_is_durable_validated_and_idempotent() {
    let (dir, db) = store();
    let agent = db.add_agent("Researcher", "Scout").unwrap();
    assert_eq!(agent.provider_id, None);
    let nonexistent = ProviderId::new();
    let before = db.events().unwrap().len();
    assert!(matches!(
        db.set_agent_provider(agent.id, Some(nonexistent), OperationId::new()),
        Err(CoreError::NotFound("provider"))
    ));
    assert_eq!(db.events().unwrap().len(), before);
    let provider_id = ProviderId::new();
    db.save_provider_profile(ProviderProfile {
        id: provider_id,
        display_name: "Private local".into(),
        model_id: "test".into(),
        base_url: "http://127.0.0.1:18767/v1".into(),
        kind: ProviderKind::OpenaiChat,
        credentials: CredentialSource::None,
    })
    .unwrap();
    let op = OperationId::new();
    let assigned = db
        .set_agent_provider(agent.id, Some(provider_id), op)
        .unwrap();
    assert_eq!(assigned.provider_id, Some(provider_id));
    assert_eq!(
        db.set_agent_provider(agent.id, Some(provider_id), op)
            .unwrap(),
        assigned
    );
    assert!(matches!(
        db.set_agent_provider(agent.id, None, op),
        Err(CoreError::OperationConflict)
    ));
    assert!(matches!(db.events().unwrap().last().unwrap().kind,
        EventKind::AgentProviderChanged { agent_id, provider_id: Some(id) }
        if agent_id == agent.id && id == provider_id));
    drop(db);
    let reopened = StudioStore::open(dir.path().join("config/studio.sqlite")).unwrap();
    assert_eq!(
        reopened.snapshot().unwrap().agents[0].provider_id,
        Some(provider_id)
    );
    let cleared = reopened
        .set_agent_provider(agent.id, None, OperationId::new())
        .unwrap();
    assert_eq!(cleared.provider_id, None);
}

#[test]
fn snapshot_and_events_survive_reopen() {
    let (temp, db) = store();
    let room = db.create_room("Engineering").unwrap();
    let agent = db.add_agent("Code reviewer", "Reviewer").unwrap();
    let message = db
        .append_message(room.id, MessageSender::Agent(agent.id), "I can review this")
        .unwrap();
    let created = db
        .create_task(room.id, agent.id, "Review src/lib.rs")
        .unwrap();
    let queued = db
        .transition_task(
            created.id,
            TaskState::Pending,
            TaskState::Queued,
            OperationId::new(),
            None,
        )
        .unwrap();
    assert_eq!(queued.state, TaskState::Queued);
    assert_eq!(db.events().unwrap().len(), 5);
    drop(db);

    let reopened = StudioStore::open(temp.path().join("config/studio.sqlite")).unwrap();
    let snapshot = reopened.snapshot().unwrap();
    assert_eq!(snapshot.rooms, vec![room]);
    assert_eq!(snapshot.agents, vec![agent]);
    assert_eq!(snapshot.messages, vec![message]);
    assert_eq!(snapshot.tasks, vec![queued]);
    assert_eq!(reopened.events().unwrap().len(), 5);
    assert!(matches!(
        reopened.events().unwrap().last().unwrap().kind,
        EventKind::TaskTransitioned {
            from: TaskState::Pending,
            to: TaskState::Queued,
            ..
        }
    ));
}

#[test]
fn rejected_input_has_no_mutation_or_event() {
    let (_dir, db) = store();
    assert!(matches!(
        db.append_message(RoomId::new(), MessageSender::User, "hello"),
        Err(CoreError::NotFound("room"))
    ));
    let room = db.create_room("Private").unwrap();
    assert!(matches!(
        db.append_message(room.id, MessageSender::Agent(AgentId::new()), "hello"),
        Err(CoreError::NotFound("agent"))
    ));
    assert!(db.create_room("  ").is_err());
    assert_eq!(db.snapshot().unwrap().messages.len(), 0);
    assert_eq!(db.events().unwrap().len(), 1);
}

#[test]
fn operation_retries_are_exactly_once_and_preserve_original_result() {
    let (_dir, db) = store();
    let room = db.create_room("Session").unwrap();
    let agent = db.add_agent("Analyst", "A").unwrap();
    let task_id = TaskId::new();
    let create_operation = OperationId::new();
    let created = db
        .create_task_with_operation(task_id, room.id, agent.id, "Run test", create_operation)
        .unwrap();
    let events_at_create = db.events().unwrap().len();
    assert_eq!(
        db.create_task_with_operation(task_id, room.id, agent.id, "Run test", create_operation)
            .unwrap(),
        created
    );
    assert_eq!(db.events().unwrap().len(), events_at_create);
    assert!(matches!(
        db.create_task_with_operation(
            task_id,
            room.id,
            agent.id,
            "Different test",
            create_operation
        ),
        Err(CoreError::OperationConflict)
    ));

    let queued_operation = OperationId::new();
    let queued = db
        .transition_task(
            task_id,
            TaskState::Pending,
            TaskState::Queued,
            queued_operation,
            None,
        )
        .unwrap();
    let running = db
        .transition_task(
            task_id,
            TaskState::Queued,
            TaskState::Running,
            OperationId::new(),
            None,
        )
        .unwrap();
    assert_eq!(running.state, TaskState::Running);
    let count = db.events().unwrap().len();
    assert_eq!(
        db.transition_task(
            task_id,
            TaskState::Pending,
            TaskState::Queued,
            queued_operation,
            None,
        )
        .unwrap(),
        queued
    );
    assert_eq!(db.events().unwrap().len(), count);
    assert!(matches!(
        db.transition_task(
            task_id,
            TaskState::Pending,
            TaskState::Cancelled,
            queued_operation,
            None
        ),
        Err(CoreError::OperationConflict)
    ));
    assert_eq!(db.snapshot().unwrap().tasks[0].state, TaskState::Running);
}

#[test]
fn cancellation_is_terminal_and_stale_completion_is_rejected() {
    let (_dir, db) = store();
    let room = db.create_room("Lab").unwrap();
    let agent = db.add_agent("Researcher", "A").unwrap();
    let created = db.create_task(room.id, agent.id, "Analyze").unwrap();
    assert!(matches!(
        db.transition_task(
            created.id,
            TaskState::Pending,
            TaskState::Succeeded,
            OperationId::new(),
            None
        ),
        Err(CoreError::InvalidTransition { .. })
    ));
    let cancelled = db
        .transition_task(
            created.id,
            TaskState::Pending,
            TaskState::Cancelled,
            OperationId::new(),
            None,
        )
        .unwrap();
    assert!(cancelled.state.is_terminal());
    assert!(matches!(
        db.transition_task(
            created.id,
            TaskState::Pending,
            TaskState::Queued,
            OperationId::new(),
            None
        ),
        Err(CoreError::StateConflict {
            actual: TaskState::Cancelled,
            ..
        })
    ));
    assert!(matches!(
        db.transition_task(
            created.id,
            TaskState::Cancelled,
            TaskState::Succeeded,
            OperationId::new(),
            None
        ),
        Err(CoreError::InvalidTransition { .. })
    ));
    assert_eq!(db.events().unwrap().len(), 4);
}

#[test]
fn duplicate_concurrent_transition_has_one_winner() {
    let (_dir, db) = store();
    let room = db.create_room("QA").unwrap();
    let agent = db.add_agent("Worker", "T").unwrap();
    let task = db.create_task(room.id, agent.id, "Check").unwrap();
    let store = std::sync::Arc::new(db);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            std::thread::spawn(move || {
                store.transition_task(
                    task.id,
                    TaskState::Pending,
                    TaskState::Queued,
                    OperationId::new(),
                    None,
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .filter(|r| r.is_err())
        .all(|r| matches!(r, Err(CoreError::StateConflict { .. }))));
    assert_eq!(store.events().unwrap().len(), 4);
}

#[test]
fn failed_event_insert_rolls_back_state_transition() {
    let (temp, db) = store();
    let room = db.create_room("Transactions").unwrap();
    let agent = db.add_agent("Auditor", "A").unwrap();
    let created = db
        .create_task(room.id, agent.id, "Test audit failure")
        .unwrap();
    let original_event_count = db.events().unwrap().len();

    // Simulate an on-disk audit write failure. The task update and event must
    // be in the same database transaction so neither becomes visible.
    let connection = rusqlite::Connection::open(temp.path().join("config/studio.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER block_events BEFORE INSERT ON events
             BEGIN SELECT RAISE(ABORT, 'simulated audit write failure'); END;",
        )
        .unwrap();
    assert!(matches!(
        db.transition_task(
            created.id,
            TaskState::Pending,
            TaskState::Queued,
            OperationId::new(),
            None
        ),
        Err(CoreError::Database(_))
    ));
    assert_eq!(db.snapshot().unwrap().tasks, vec![created.clone()]);
    assert_eq!(db.events().unwrap().len(), original_event_count);

    connection
        .execute_batch("DROP TRIGGER block_events")
        .unwrap();
    assert!(db
        .transition_task(
            created.id,
            TaskState::Pending,
            TaskState::Queued,
            OperationId::new(),
            None,
        )
        .is_ok());
}

#[test]
fn retries_of_room_agent_and_message_commands_do_not_duplicate_records() {
    let (_dir, db) = store();
    let room_id = RoomId::new();
    let room_op = OperationId::new();
    let room = db
        .create_room_with_operation(room_id, "Task room", room_op)
        .unwrap();
    assert_eq!(
        db.create_room_with_operation(room_id, "Task room", room_op)
            .unwrap(),
        room
    );
    assert!(matches!(
        db.create_room_with_operation(room_id, "Different room", room_op),
        Err(CoreError::OperationConflict)
    ));

    let agent_id = AgentId::new();
    let agent_op = OperationId::new();
    let agent = db
        .add_agent_with_operation(agent_id, "Code reviewer", "A", agent_op)
        .unwrap();
    assert_eq!(
        db.add_agent_with_operation(agent_id, "Code reviewer", "A", agent_op)
            .unwrap(),
        agent
    );

    let message_id = MessageId::new();
    let message_op = OperationId::new();
    let sender = MessageSender::Agent(agent.id);
    let message = db
        .append_message_with_operation(message_id, room.id, sender.clone(), "Review", message_op)
        .unwrap();
    assert_eq!(
        db.append_message_with_operation(message_id, room.id, sender, "Review", message_op)
            .unwrap(),
        message
    );
    let snapshot = db.snapshot().unwrap();
    assert_eq!(snapshot.rooms.len(), 1);
    assert_eq!(snapshot.agents.len(), 1);
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(db.events().unwrap().len(), 3);
}

#[test]
fn task_completion_and_reply_are_atomic_and_idempotent() {
    let (temp, db) = store();
    let room = db.create_room("Task room").unwrap();
    let agent = db.add_agent("Researcher", "A").unwrap();
    let task = db
        .create_task(room.id, agent.id, "Summarize sources")
        .unwrap();
    db.transition_task(
        task.id,
        TaskState::Pending,
        TaskState::Queued,
        OperationId::new(),
        None,
    )
    .unwrap();
    db.transition_task(
        task.id,
        TaskState::Queued,
        TaskState::Running,
        OperationId::new(),
        None,
    )
    .unwrap();
    let reply_id = MessageId::new();
    let op = OperationId::new();
    let result = db
        .complete_task_with_message(task.id, reply_id, op, "Summary")
        .unwrap();
    assert_eq!(result.0.state, TaskState::Succeeded);
    assert_eq!(result.1.sender, MessageSender::Agent(agent.id));
    assert_eq!(
        db.complete_task_with_message(task.id, reply_id, op, "Summary")
            .unwrap(),
        result
    );
    assert_eq!(db.snapshot().unwrap().messages.len(), 1);
    assert!(matches!(
        db.complete_task_with_message(task.id, reply_id, op, "Other summary"),
        Err(CoreError::OperationConflict)
    ));
    assert!(matches!(db.events().unwrap().last().unwrap().kind,
        EventKind::TaskCompleted { task_id, message_id, .. } if task_id == task.id && message_id == reply_id));

    let other = db.create_task(room.id, agent.id, "Another task").unwrap();
    db.transition_task(
        other.id,
        TaskState::Pending,
        TaskState::Queued,
        OperationId::new(),
        None,
    )
    .unwrap();
    db.transition_task(
        other.id,
        TaskState::Queued,
        TaskState::Running,
        OperationId::new(),
        None,
    )
    .unwrap();
    let before = db.events().unwrap().len();
    let connection = rusqlite::Connection::open(temp.path().join("config/studio.sqlite")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_task_audit BEFORE INSERT ON events
         BEGIN SELECT RAISE(ABORT, 'audit failure'); END;",
        )
        .unwrap();
    assert!(db
        .complete_task_with_message(other.id, MessageId::new(), OperationId::new(), "Output")
        .is_err());
    assert_eq!(db.events().unwrap().len(), before);
    let after = db.snapshot().unwrap();
    assert_eq!(after.messages.len(), 1);
    assert_eq!(
        after
            .tasks
            .iter()
            .find(|item| item.id == other.id)
            .unwrap()
            .state,
        TaskState::Running
    );
}

#[test]
fn cancellation_wins_over_late_model_reply() {
    let (_temp, db) = store();
    let room = db.create_room("Cancelled work").unwrap();
    let agent = db.add_agent("Researcher", "A").unwrap();
    let task = db.create_task(room.id, agent.id, "Work").unwrap();
    db.transition_task(
        task.id,
        TaskState::Pending,
        TaskState::Queued,
        OperationId::new(),
        None,
    )
    .unwrap();
    db.transition_task(
        task.id,
        TaskState::Queued,
        TaskState::Running,
        OperationId::new(),
        None,
    )
    .unwrap();
    db.transition_task(
        task.id,
        TaskState::Running,
        TaskState::Cancelled,
        OperationId::new(),
        None,
    )
    .unwrap();
    assert!(matches!(
        db.complete_task_with_message(
            task.id,
            MessageId::new(),
            OperationId::new(),
            "Late model text"
        ),
        Err(CoreError::StateConflict {
            actual: TaskState::Cancelled,
            ..
        })
    ));
    assert!(db.snapshot().unwrap().messages.is_empty());
}
