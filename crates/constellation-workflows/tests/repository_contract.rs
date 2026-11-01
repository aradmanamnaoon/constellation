use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

use constellation_workflows::{
    CompletionOutcome, Repository, RepositoryError, StepDefinition, StepStatus, WorkflowDefinition,
    WorkflowRun, MAX_JSON_BYTES, MAX_RESULT_BYTES, SCHEMA_VERSION,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

fn make_run(id: &str) -> WorkflowRun {
    WorkflowRun::new(WorkflowDefinition {
        schema_version: SCHEMA_VERSION,
        workflow_id: id.to_owned(),
        steps: vec![
            StepDefinition {
                id: "research".into(),
                role: "researcher".into(),
                agent_id: "agent.research".into(),
                provider_id: "anthropic.explicit".into(),
                instruction: "Collect evidence".into(),
                depends_on: vec![],
                max_attempts: 2,
            },
            StepDefinition {
                id: "write".into(),
                role: "writer".into(),
                agent_id: "agent.writer".into(),
                provider_id: "local.explicit".into(),
                instruction: "Write evidence-based summary".into(),
                depends_on: vec!["research".into()],
                max_attempts: 1,
            },
        ],
    })
    .unwrap()
}

fn db_path(dir: &Path) -> std::path::PathBuf {
    dir.join("workflow-state.sqlite3")
}

#[test]
fn restart_restores_validated_snapshots_and_stable_bounded_pages() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    {
        let repo = Repository::new(&path).unwrap();
        assert!(repo.get("missing").unwrap().is_none());
        assert_eq!(repo.create(&make_run("run-b")).unwrap().revision, 1);
        assert_eq!(repo.create(&make_run("run-a")).unwrap().revision, 1);
        let names: Vec<String> = repo
            .list(100, 0)
            .unwrap()
            .iter()
            .map(|item| item.run.definition().workflow_id.clone())
            .collect();
        assert_eq!(names, ["run-a", "run-b"]);
        assert_eq!(
            repo.list(1, 1).unwrap()[0].run.definition().workflow_id,
            "run-b"
        );
        assert!(repo.list(1, 2).unwrap().is_empty());
        assert!(matches!(
            repo.list(101, 0),
            Err(RepositoryError::InvalidPage)
        ));
        assert!(matches!(repo.list(0, 0), Err(RepositoryError::InvalidPage)));
        assert!(matches!(
            repo.list(1, usize::MAX),
            Err(RepositoryError::InvalidPage)
        ));
        assert!(matches!(
            repo.create(&make_run("run-a")),
            Err(RepositoryError::AlreadyExists)
        ));
    }
    assert!(path.exists());
    let db = Connection::open(&path).unwrap();
    let mode: String = db
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    drop(db);
    let reopened = Repository::open(&path).unwrap();
    assert_eq!(reopened.get("run-a").unwrap().unwrap().revision, 1);
    assert_eq!(reopened.list(100, 0).unwrap().len(), 2);
}

#[test]
fn restart_blocks_inflight_attempt_without_replay_or_automatic_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    let persisted_running = {
        let repo = Repository::open(&path).unwrap();
        let created = repo.create(&make_run("run-restart")).unwrap();
        let mut run = created.run;
        let dispatch = run.start("research").unwrap();
        assert_eq!(dispatch.attempt, 1);
        let saved = repo.update(created.revision, &run).unwrap();
        assert_eq!(saved.revision, 2);
        saved
    };
    let restarted = Repository::open(&path).unwrap();
    let blocked = restarted.get("run-restart").unwrap().unwrap();
    assert_eq!(blocked.revision, 2);
    assert_eq!(
        blocked.run.progress("research").unwrap().status,
        StepStatus::Blocked
    );
    assert!(blocked.run.ready_steps().is_empty());
    let mut from_restart = blocked.run.clone();
    assert!(from_restart.start("research").is_err());
    assert!(matches!(
        restarted.complete(&blocked, "research", 1, CompletionOutcome::Succeeded),
        Err(RepositoryError::Workflow(_))
    ));
    // Restarted state can be explicitly abandoned by trusted host decision.
    assert!(from_restart.skip("research").unwrap());
    let abandoned = restarted.update(blocked.revision, &from_restart).unwrap();
    assert_eq!(abandoned.revision, 3);
    assert_eq!(
        abandoned.run.progress("write").unwrap().status,
        StepStatus::Skipped
    );
    assert_eq!(
        persisted_running.run.progress("research").unwrap().status,
        StepStatus::Running
    );
}

#[test]
fn independent_connections_compete_and_exactly_one_cas_writer_wins() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    Repository::open(&path)
        .unwrap()
        .create(&make_run("race"))
        .unwrap();
    let gate = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|i| {
            let path = path.clone();
            let gate = Arc::clone(&gate);
            thread::spawn(move || {
                let repo = Repository::open(path).unwrap();
                let snapshot = repo.get("race").unwrap().unwrap();
                let mut run = snapshot.run;
                if i == 0 {
                    run.skip("research").unwrap();
                } else {
                    run.cancel_step("research").unwrap();
                }
                gate.wait();
                repo.update(snapshot.revision, &run)
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(RepositoryError::RevisionConflict)))
            .count(),
        1
    );
    let final_run = Repository::open(&path)
        .unwrap()
        .get("race")
        .unwrap()
        .unwrap();
    assert_eq!(final_run.revision, 2);
    assert!(matches!(
        final_run.run.progress("research").unwrap().status,
        StepStatus::Skipped | StepStatus::Cancelled
    ));
    assert_eq!(
        final_run.run.progress("write").unwrap().status,
        StepStatus::Skipped
    );
}

#[test]
fn completion_is_atomic_idempotent_and_stale_attempts_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::open(db_path(dir.path())).unwrap();
    let created = repo.create(&make_run("complete")).unwrap();
    let mut running = created.run;
    let proposed = running.start("research").unwrap();
    let started = repo.update(created.revision, &running).unwrap();
    let first = repo
        .complete(
            &started,
            "research",
            proposed.attempt,
            CompletionOutcome::Succeeded,
        )
        .unwrap();
    assert_eq!(first.revision, 3);
    assert_eq!(first.run.ready_steps(), ["write"]);
    // Same client snapshot, duplicate callback: checks DB and returns the
    // committed revision without a second update.
    let duplicate = repo
        .complete(&started, "research", 1, CompletionOutcome::Succeeded)
        .unwrap();
    assert_eq!(duplicate.revision, first.revision);
    let completed_duplicate = repo
        .complete(&first, "research", 1, CompletionOutcome::Succeeded)
        .unwrap();
    assert_eq!(completed_duplicate.revision, 3);
    assert!(matches!(
        repo.complete(&started, "research", 1, CompletionOutcome::Failed),
        Err(RepositoryError::RevisionConflict)
    ));
    assert_eq!(repo.get("complete").unwrap().unwrap().revision, 3);

    let created = repo.create(&make_run("retry")).unwrap();
    let mut running = created.run;
    running.start("research").unwrap();
    let started = repo.update(created.revision, &running).unwrap();
    let failed = repo
        .complete(&started, "research", 1, CompletionOutcome::Failed)
        .unwrap();
    assert_eq!(
        failed.run.progress("research").unwrap().status,
        StepStatus::AwaitingRetry
    );
    let mut next = failed.run;
    next.retry("research").unwrap();
    // Persist explicit retry before assigning another attempt.
    let retried = repo.update(failed.revision, &next).unwrap();
    next.start("research").unwrap();
    let second = repo.update(retried.revision, &next).unwrap();
    assert!(matches!(
        repo.complete(&second, "research", 1, CompletionOutcome::Succeeded),
        Err(RepositoryError::Workflow(_))
    ));
    assert_eq!(
        repo.get("retry").unwrap().unwrap().revision,
        second.revision
    );
}

#[test]
fn successful_text_result_is_durable_atomic_bounded_and_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::open(db_path(dir.path())).unwrap();
    let initial = repo.create(&make_run("with-result")).unwrap();
    let mut active = initial.run;
    active.start("research").unwrap();
    let started = repo.update(initial.revision, &active).unwrap();
    let mut outcome = started.run.clone();
    assert!(outcome
        .complete_with_result(
            "research",
            1,
            CompletionOutcome::Succeeded,
            Some("x".repeat(MAX_RESULT_BYTES + 1))
        )
        .is_err());
    assert_eq!(repo.get("with-result").unwrap().unwrap().revision, 2);
    outcome
        .complete_with_result(
            "research",
            1,
            CompletionOutcome::Succeeded,
            Some("Evidence from fixture".to_owned()),
        )
        .unwrap();
    let completed = repo.update(started.revision, &outcome).unwrap();
    assert_eq!(completed.revision, 3);
    assert_eq!(
        repo.get("with-result")
            .unwrap()
            .unwrap()
            .run
            .progress("research")
            .unwrap()
            .result_text
            .as_deref(),
        Some("Evidence from fixture")
    );
    let mut forged = completed.run;
    let forged_json = forged.to_json().unwrap();
    let mut forged_value: serde_json::Value = serde_json::from_slice(&forged_json).unwrap();
    forged_value["states"]["research"]["result_text"] = serde_json::json!("different result");
    forged = WorkflowRun::from_json_bounded(&serde_json::to_vec(&forged_value).unwrap()).unwrap();
    assert!(matches!(
        repo.update(completed.revision, &forged),
        Err(RepositoryError::InvalidTransition)
    ));
    let mut bad = serde_json::from_slice::<serde_json::Value>(&forged_json).unwrap();
    bad["states"]["write"]["result_text"] = serde_json::json!("result before execution");
    assert!(WorkflowRun::from_json_bounded(&serde_json::to_vec(&bad).unwrap()).is_err());
    bad["states"]["write"]["result_text"] = serde_json::Value::Null;
    bad["states"]["research"]["result_text"] = serde_json::json!("Z".repeat(MAX_RESULT_BYTES + 1));
    assert!(WorkflowRun::from_json_bounded(&serde_json::to_vec(&bad).unwrap()).is_err());
}

#[test]
fn failed_database_write_preserves_prior_revision_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    let repo = Repository::open(&path).unwrap();
    let original = repo.create(&make_run("atomic")).unwrap();
    let mut changed = original.run.clone();
    changed.skip("research").unwrap();
    let raw = Connection::open(&path).unwrap();
    raw.execute_batch(
        "CREATE TRIGGER fail_updates BEFORE UPDATE ON workflow_runs
         BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )
    .unwrap();
    assert!(matches!(
        repo.update(original.revision, &changed),
        Err(RepositoryError::Database(_))
    ));
    let unchanged = repo.get("atomic").unwrap().unwrap();
    assert_eq!(unchanged, original);
    raw.execute_batch("DROP TRIGGER fail_updates").unwrap();
    assert_eq!(
        repo.update(original.revision, &changed).unwrap().revision,
        2
    );
    assert!(matches!(
        repo.update(original.revision, &changed),
        Err(RepositoryError::RevisionConflict)
    ));
}

#[test]
fn altered_or_invalid_json_records_fail_closed_even_with_matching_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    let repo = Repository::open(&path).unwrap();
    let created = repo.create(&make_run("tamper")).unwrap();
    let raw = Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE workflow_runs SET payload = ?1 WHERE id = ?2",
        params![b"invalid".as_slice(), "tamper"],
    )
    .unwrap();
    assert!(matches!(
        repo.get("tamper"),
        Err(RepositoryError::CorruptRecord)
    ));
    assert!(matches!(
        repo.list(10, 0),
        Err(RepositoryError::CorruptRecord)
    ));
    let initial = created.run.to_json().unwrap();
    let mut altered: serde_json::Value = serde_json::from_slice(&initial).unwrap();
    altered["states"]["write"]["status"] = serde_json::json!("succeeded");
    altered["states"]["write"]["attempts"] = serde_json::json!(1);
    altered["states"]["write"]["last_completion"] =
        serde_json::json!({"attempt":1,"outcome":"succeeded"});
    let bytes = serde_json::to_vec(&altered).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    raw.execute(
        "UPDATE workflow_runs SET payload = ?1, sha256 = ?2 WHERE id = ?3",
        params![bytes, hash, "tamper"],
    )
    .unwrap();
    assert!(matches!(
        repo.get("tamper"),
        Err(RepositoryError::CorruptRecord)
    ));
    raw.execute(
        "UPDATE workflow_runs SET payload = ?1, sha256 = ?2 WHERE id = ?3",
        params![
            initial.clone(),
            format!("{:x}", Sha256::digest(&initial)),
            "tamper"
        ],
    )
    .unwrap();
    assert_eq!(repo.get("tamper").unwrap().unwrap(), created);
}

#[test]
fn oversized_blob_is_rejected_before_loading_and_unknown_schema_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    let repo = Repository::open(&path).unwrap();
    repo.create(&make_run("oversized")).unwrap();
    let raw = Connection::open(&path).unwrap();
    raw.execute_batch("PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    raw.execute(
        "UPDATE workflow_runs SET payload=zeroblob(?1) WHERE id=?2",
        params![MAX_JSON_BYTES as i64 + 1, "oversized"],
    )
    .unwrap();
    assert!(matches!(
        repo.get("oversized"),
        Err(RepositoryError::CorruptRecord)
    ));
    drop(repo);
    raw.execute_batch("PRAGMA user_version=999;").unwrap();
    drop(raw);
    assert!(matches!(
        Repository::open(&path),
        Err(RepositoryError::UnsupportedSchema(999))
    ));
    assert!(matches!(
        Repository::open("relative.sqlite3"),
        Err(RepositoryError::InvalidPath)
    ));
    assert!(fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn unrelated_existing_database_without_schema_version_is_not_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let path = db_path(dir.path());
    let existing = Connection::open(&path).unwrap();
    existing
        .execute_batch("CREATE TABLE unrelated_data (secret TEXT NOT NULL); INSERT INTO unrelated_data VALUES ('keep');")
        .unwrap();
    drop(existing);
    assert!(matches!(
        Repository::open(&path),
        Err(RepositoryError::CorruptRecord)
    ));
    let verify = Connection::open(&path).unwrap();
    let value: String = verify
        .query_row("SELECT secret FROM unrelated_data", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "keep");
    let count: i64 = verify
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='workflow_runs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn cas_rejects_terminal_reset_and_completion_without_persisted_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::open(db_path(dir.path())).unwrap();
    let initial = repo.create(&make_run("no-reset")).unwrap();
    let mut shortcut = initial.run.clone();
    shortcut.start("research").unwrap();
    shortcut
        .complete("research", 1, CompletionOutcome::Succeeded)
        .unwrap();
    // A workflow must persist Running before its effect can be accepted.
    assert!(matches!(
        repo.update(initial.revision, &shortcut),
        Err(RepositoryError::InvalidTransition)
    ));
    assert_eq!(repo.get("no-reset").unwrap().unwrap().revision, 1);

    let mut running = initial.run.clone();
    running.start("research").unwrap();
    let started = repo.update(initial.revision, &running).unwrap();
    let completed = repo
        .complete(&started, "research", 1, CompletionOutcome::Succeeded)
        .unwrap();
    assert_eq!(completed.revision, 3);
    let reset = make_run("no-reset");
    assert!(matches!(
        repo.update(completed.revision, &reset),
        Err(RepositoryError::InvalidTransition)
    ));
    assert_eq!(
        repo.get("no-reset")
            .unwrap()
            .unwrap()
            .run
            .progress("research")
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
}
