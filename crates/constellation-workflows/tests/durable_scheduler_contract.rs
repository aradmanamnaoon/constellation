use std::{
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

use constellation_workflows::{
    CompletionOutcome as Outcome, Repository, RepositoryError, StepDefinition, StepStatus,
    WorkflowDefinition, WorkflowRun, SCHEMA_VERSION,
};
use rusqlite::Connection;

fn definition(id: &str) -> WorkflowRun {
    let step = |name: &str, deps: &[&str], attempts| StepDefinition {
        id: name.into(),
        role: "researcher".into(),
        agent_id: "agent.r".into(),
        provider_id: "local.explicit".into(),
        instruction: "Produce evidence".into(),
        depends_on: deps.iter().map(|dep| (*dep).into()).collect(),
        max_attempts: attempts,
    };
    WorkflowRun::new(WorkflowDefinition {
        schema_version: SCHEMA_VERSION,
        workflow_id: id.into(),
        steps: vec![
            step("a", &[], 2),
            step("b", &[], 1),
            step("join", &["a", "b"], 1),
        ],
    })
    .unwrap()
}

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("dag.sqlite3")
}

#[test]
fn claim_is_durable_ordered_and_refuses_stale_or_unready_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::open(path(dir.path())).unwrap();
    repo.create(&definition("schedule")).unwrap();

    let first = repo.claim_next_ready("schedule", 1).unwrap().unwrap();
    assert_eq!(first.dispatch.step_id, "a");
    assert_eq!(first.dispatch.attempt, 1);
    assert_eq!(first.stored.revision, 2);
    assert!(matches!(
        repo.claim_ready("schedule", "b", 1),
        Err(RepositoryError::RevisionConflict)
    ));
    assert!(matches!(
        repo.claim_ready("schedule", "join", 2),
        Err(RepositoryError::Workflow(_))
    ));
    assert_eq!(repo.get("schedule").unwrap().unwrap().revision, 2);

    let second = repo.claim_next_ready("schedule", 2).unwrap().unwrap();
    assert_eq!(second.dispatch.step_id, "b");
    assert_eq!(second.stored.revision, 3);
    assert!(repo.claim_next_ready("schedule", 3).unwrap().is_none());
    assert_eq!(repo.get("schedule").unwrap().unwrap().revision, 3);
    assert!(matches!(
        repo.claim_ready("schedule", "unknown", 3),
        Err(RepositoryError::Workflow(_))
    ));
}

#[test]
fn independent_connections_can_commit_both_parallel_completions_without_lost_results() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    let bootstrap = Repository::open(&db).unwrap();
    bootstrap.create(&definition("parallel")).unwrap();
    let a = bootstrap.claim_ready("parallel", "a", 1).unwrap();
    let b = bootstrap.claim_ready("parallel", "b", 2).unwrap();
    let gate = Arc::new(Barrier::new(2));
    // Construct independent SQLite connections before releasing the workers.
    let connections = [
        Repository::open(&db).unwrap(),
        Repository::open(&db).unwrap(),
    ];
    let threads: Vec<_> = connections
        .into_iter()
        .zip([a, b])
        .map(|(repo, claim)| {
            let gate = Arc::clone(&gate);
            thread::spawn(move || {
                gate.wait();
                repo.complete(
                    &claim.stored,
                    &claim.dispatch.step_id,
                    claim.dispatch.attempt,
                    Outcome::Succeeded,
                )
            })
        })
        .collect();
    let completed: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap().unwrap())
        .collect();
    assert_ne!(completed[0].revision, completed[1].revision);
    let state = Repository::open(&db)
        .unwrap()
        .get("parallel")
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 5);
    assert_eq!(
        state.run.progress("a").unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(
        state.run.progress("b").unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(state.run.ready_steps(), ["join"]);
}

#[test]
fn independent_parallel_results_commit_atomically_with_exact_duplicate_matching() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    let bootstrap = Repository::open(&db).unwrap();
    bootstrap.create(&definition("results")).unwrap();
    let a = bootstrap.claim_ready("results", "a", 1).unwrap();
    let b = bootstrap.claim_ready("results", "b", 2).unwrap();
    let gate = Arc::new(Barrier::new(2));
    let connections = [
        Repository::open(&db).unwrap(),
        Repository::open(&db).unwrap(),
    ];
    let threads: Vec<_> = connections
        .into_iter()
        .zip([(a.clone(), "evidence A"), (b.clone(), "evidence B")])
        .map(|(repo, (claim, output))| {
            let gate = Arc::clone(&gate);
            thread::spawn(move || {
                gate.wait();
                repo.complete_with_result(
                    &claim.stored,
                    &claim.dispatch.step_id,
                    claim.dispatch.attempt,
                    Outcome::Succeeded,
                    Some(output.into()),
                )
            })
        })
        .collect();
    for handle in threads {
        handle.join().unwrap().unwrap();
    }
    let state = bootstrap.get("results").unwrap().unwrap();
    assert_eq!(state.revision, 5);
    assert_eq!(
        state.run.progress("a").unwrap().result_text.as_deref(),
        Some("evidence A")
    );
    assert_eq!(
        state.run.progress("b").unwrap().result_text.as_deref(),
        Some("evidence B")
    );
    assert_eq!(state.run.ready_steps(), ["join"]);

    let duplicate = bootstrap
        .complete_with_result(
            &a.stored,
            "a",
            1,
            Outcome::Succeeded,
            Some("evidence A".into()),
        )
        .unwrap();
    assert_eq!(duplicate.revision, 5);
    assert!(matches!(
        bootstrap.complete_with_result(
            &a.stored,
            "a",
            1,
            Outcome::Succeeded,
            Some("forged".into())
        ),
        Err(RepositoryError::RevisionConflict)
    ));
    assert!(matches!(
        bootstrap.complete_with_result(&b.stored, "b", 1, Outcome::Failed, Some("forged".into())),
        Err(RepositoryError::Workflow(_))
    ));
    assert_eq!(bootstrap.get("results").unwrap().unwrap(), state);
}

#[test]
fn concurrent_claims_from_two_connections_never_dispatch_same_step_twice() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    Repository::open(&db)
        .unwrap()
        .create(&definition("compete"))
        .unwrap();
    let gate = Arc::new(Barrier::new(2));
    let connections = [
        Repository::open(&db).unwrap(),
        Repository::open(&db).unwrap(),
    ];
    let handles: Vec<_> = connections
        .into_iter()
        .map(|repo| {
            let gate = Arc::clone(&gate);
            thread::spawn(move || {
                gate.wait();
                repo.claim_ready("compete", "a", 1)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(RepositoryError::RevisionConflict)))
            .count(),
        1
    );
    let state = Repository::open(&db)
        .unwrap()
        .get("compete")
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 2);
    assert_eq!(state.run.progress("a").unwrap().attempts, 1);
    assert_eq!(state.run.progress("a").unwrap().status, StepStatus::Blocked);
}

#[test]
fn cancellation_and_completion_race_preserves_exactly_one_terminal_decision() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    let repo = Repository::open(&db).unwrap();
    repo.create(&definition("cancel-race")).unwrap();
    let running = repo.claim_ready("cancel-race", "a", 1).unwrap();
    let mut cancel = running.stored.run.clone();
    cancel.cancel_step("a").unwrap();
    let revision = running.stored.revision;
    let gate = Arc::new(Barrier::new(2));
    let complete_repo = Repository::open(&db).unwrap();
    let cancel_repo = Repository::open(&db).unwrap();
    let start = Arc::clone(&gate);
    let completion = thread::spawn(move || {
        start.wait();
        complete_repo.complete(&running.stored, "a", 1, Outcome::Succeeded)
    });
    let cancellation = thread::spawn(move || {
        gate.wait();
        cancel_repo.update(revision, &cancel)
    });
    let first = completion.join().unwrap();
    let second = cancellation.join().unwrap();
    assert_eq!(u8::from(first.is_ok()) + u8::from(second.is_ok()), 1);
    assert!(first.is_ok() || matches!(first, Err(RepositoryError::RevisionConflict)));
    assert!(second.is_ok() || matches!(second, Err(RepositoryError::RevisionConflict)));
    let final_state = Repository::open(&db)
        .unwrap()
        .get("cancel-race")
        .unwrap()
        .unwrap();
    assert_eq!(final_state.revision, 3);
    let status = final_state.run.progress("a").unwrap().status;
    assert!(matches!(
        status,
        StepStatus::Succeeded | StepStatus::Cancelled
    ));
    assert_eq!(final_state.run.progress("a").unwrap().attempts, 1);
    assert_eq!(
        final_state.run.progress("join").unwrap().status,
        if status == StepStatus::Cancelled {
            StepStatus::Skipped
        } else {
            StepStatus::Pending
        }
    );
}

#[test]
fn failed_attempt_requires_explicit_durable_retry_and_rejects_stale_callbacks() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::open(path(dir.path())).unwrap();
    repo.create(&definition("retry")).unwrap();
    let first = repo.claim_ready("retry", "a", 1).unwrap();
    let failed = repo
        .complete(&first.stored, "a", 1, Outcome::Failed)
        .unwrap();
    assert_eq!(
        failed.run.progress("a").unwrap().status,
        StepStatus::AwaitingRetry
    );
    assert!(matches!(
        repo.claim_ready("retry", "a", failed.revision),
        Err(RepositoryError::Workflow(_))
    ));
    let mut retried = failed.run.clone();
    retried.retry("a").unwrap();
    let ready = repo.update(failed.revision, &retried).unwrap();
    let second = repo.claim_ready("retry", "a", ready.revision).unwrap();
    assert_eq!(second.dispatch.attempt, 2);
    assert!(matches!(
        repo.complete(&first.stored, "a", 1, Outcome::Succeeded),
        Err(RepositoryError::RevisionConflict)
    ));
    assert!(matches!(
        repo.complete(&second.stored, "a", 1, Outcome::Succeeded),
        Err(RepositoryError::Workflow(_))
    ));
    let terminal = repo
        .complete(&second.stored, "a", 2, Outcome::Failed)
        .unwrap();
    assert_eq!(
        terminal.run.progress("a").unwrap().status,
        StepStatus::Failed
    );
    assert_eq!(
        terminal.run.progress("join").unwrap().status,
        StepStatus::Skipped
    );
    assert!(matches!(
        repo.claim_ready("retry", "a", terminal.revision),
        Err(RepositoryError::Workflow(_))
    ));
    let duplicate = repo
        .complete(&second.stored, "a", 2, Outcome::Failed)
        .unwrap();
    assert_eq!(duplicate.revision, terminal.revision);
}

#[test]
fn restart_quarantines_unfinished_branch_but_independent_branch_can_finish() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    {
        let repo = Repository::open(&db).unwrap();
        repo.create(&definition("restart")).unwrap();
        let claim = repo.claim_ready("restart", "a", 1).unwrap();
        assert_eq!(claim.stored.revision, 2);
    }
    let restarted = Repository::open(&db).unwrap();
    let stuck = restarted.get("restart").unwrap().unwrap();
    assert_eq!(stuck.run.progress("a").unwrap().status, StepStatus::Blocked);
    assert!(matches!(
        restarted.complete(&stuck, "a", 1, Outcome::Succeeded),
        Err(RepositoryError::Workflow(_))
    ));
    assert!(matches!(
        restarted.claim_ready("restart", "a", 2),
        Err(RepositoryError::Workflow(_))
    ));
    let independent = restarted.claim_next_ready("restart", 2).unwrap().unwrap();
    assert_eq!(independent.dispatch.step_id, "b");
    restarted
        .complete(&independent.stored, "b", 1, Outcome::Succeeded)
        .unwrap();
    let after = restarted.get("restart").unwrap().unwrap();
    assert_eq!(after.run.progress("a").unwrap().status, StepStatus::Blocked);
    assert!(after.run.ready_steps().is_empty());
    assert_eq!(after.revision, 4);
    let mut resolve = after.run;
    resolve.skip("a").unwrap();
    let result = restarted.update(4, &resolve).unwrap();
    assert_eq!(
        result.run.progress("join").unwrap().status,
        StepStatus::Skipped
    );
    assert_eq!(result.run.progress("a").unwrap().attempts, 1);
}

#[test]
fn sqlite_write_failure_returns_no_dispatch_and_preserves_revision() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    let repo = Repository::open(&db).unwrap();
    let initial = repo.create(&definition("io-failure")).unwrap();
    let raw = Connection::open(&db).unwrap();
    raw.execute_batch(
        "CREATE TRIGGER reject_claim BEFORE UPDATE ON workflow_runs
         BEGIN SELECT RAISE(ABORT, 'simulated disk failure'); END;",
    )
    .unwrap();
    assert!(matches!(
        repo.claim_next_ready("io-failure", 1),
        Err(RepositoryError::Database(_))
    ));
    assert_eq!(repo.get("io-failure").unwrap().unwrap(), initial);
    raw.execute_batch("DROP TRIGGER reject_claim").unwrap();
    let claimed = repo.claim_next_ready("io-failure", 1).unwrap().unwrap();
    assert_eq!(claimed.dispatch.attempt, 1);
    assert_eq!(claimed.stored.revision, 2);
}

#[test]
fn sqlite_write_failure_during_rebased_completion_is_atomic_and_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let db = path(dir.path());
    let repo = Repository::open(&db).unwrap();
    repo.create(&definition("commit-failure")).unwrap();
    let first = repo.claim_ready("commit-failure", "a", 1).unwrap();
    let second = repo.claim_ready("commit-failure", "b", 2).unwrap();
    let before = repo
        .complete(&first.stored, "a", 1, Outcome::Succeeded)
        .unwrap();
    assert_eq!(before.revision, 4);

    let raw = Connection::open(&db).unwrap();
    raw.execute_batch(
        "CREATE TRIGGER reject_complete BEFORE UPDATE ON workflow_runs
         BEGIN SELECT RAISE(ABORT, 'simulated commit failure'); END;",
    )
    .unwrap();
    assert!(matches!(
        repo.complete(&second.stored, "b", 1, Outcome::Succeeded),
        Err(RepositoryError::Database(_))
    ));
    assert_eq!(repo.get("commit-failure").unwrap().unwrap(), before);
    raw.execute_batch("DROP TRIGGER reject_complete").unwrap();
    let completed = repo
        .complete(&second.stored, "b", 1, Outcome::Succeeded)
        .unwrap();
    assert_eq!(completed.revision, 5);
    assert_eq!(completed.run.ready_steps(), ["join"]);
}
