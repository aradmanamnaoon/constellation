use constellation_workflows::{
    CompletionOutcome as Outcome, StepDefinition, StepStatus as Status, WorkflowDefinition,
    WorkflowError, WorkflowRun, MAX_JSON_BYTES, SCHEMA_VERSION,
};

fn step(
    id: &str,
    dependencies: &[&str],
    role: &str,
    provider: &str,
    attempts: u8,
) -> StepDefinition {
    StepDefinition {
        id: id.into(),
        role: role.into(),
        agent_id: format!("agent_{id}"),
        provider_id: provider.into(),
        instruction: format!("Complete the {id} task and submit its result."),
        depends_on: dependencies.iter().map(|s| (*s).into()).collect(),
        max_attempts: attempts,
    }
}

fn definition(steps: Vec<StepDefinition>) -> WorkflowDefinition {
    WorkflowDefinition {
        schema_version: SCHEMA_VERSION,
        workflow_id: "study-001".into(),
        steps,
    }
}

#[test]
fn researcher_writer_reviewer_full_chain_with_explicit_provider_dispatch() {
    let plan = definition(vec![
        step("reviewer", &["writer"], "reviewer", "local_ollama", 1),
        step("writer", &["researcher"], "writer", "gemini", 2),
        step("researcher", &[], "researcher", "anthropic", 1),
    ]);
    assert_eq!(
        plan.topological_order().unwrap(),
        ["researcher", "writer", "reviewer"]
    );
    let mut run = WorkflowRun::new(plan).unwrap();
    assert_eq!(run.ready_steps(), ["researcher"]);
    assert!(matches!(
        run.start("writer"),
        Err(WorkflowError::NotReady(_))
    ));

    let researcher = run.start("researcher").unwrap();
    assert_eq!(researcher.role, "researcher");
    assert_eq!(researcher.agent_id, "agent_researcher");
    assert_eq!(researcher.provider_id, "anthropic");
    assert_eq!(run.start("researcher").unwrap(), researcher);
    assert_eq!(
        run.complete("researcher", researcher.attempt, Outcome::Succeeded)
            .unwrap(),
        Status::Succeeded
    );
    assert_eq!(run.ready_steps(), ["writer"]);

    let writer = run.start("writer").unwrap();
    assert_eq!(writer.provider_id, "gemini");
    assert_eq!(writer.attempt, 1);
    assert_eq!(
        run.complete("writer", 1, Outcome::Failed).unwrap(),
        Status::AwaitingRetry
    );
    assert!(run.ready_steps().is_empty());
    assert!(!run.finished());
    assert!(run.retry("writer").unwrap());
    assert!(!run.retry("writer").unwrap());
    assert_eq!(run.ready_steps(), ["writer"]);

    let rerun = run.start("writer").unwrap();
    assert_eq!(rerun.attempt, 2);
    assert!(matches!(
        run.complete("writer", 1, Outcome::Failed),
        Err(WorkflowError::AttemptMismatch { .. })
    ));
    assert_eq!(
        run.complete("writer", 2, Outcome::Succeeded).unwrap(),
        Status::Succeeded
    );
    assert_eq!(run.ready_steps(), ["reviewer"]);
    let reviewer = run.start("reviewer").unwrap();
    assert_eq!(reviewer.provider_id, "local_ollama");
    run.complete("reviewer", reviewer.attempt, Outcome::Succeeded)
        .unwrap();
    assert!(run.finished());
    assert!(run.ready_steps().is_empty());

    let encoded = run.to_json().unwrap();
    let restored = WorkflowRun::from_json_bounded(&encoded).unwrap();
    assert_eq!(run, restored);
    assert_eq!(restored.definition().workflow_id, "study-001");
    assert_eq!(
        serde_json::from_slice::<WorkflowRun>(&encoded).unwrap(),
        run
    );
}

#[test]
fn parallel_coders_ready_order_independent_of_definition_order() {
    let mut plan = definition(vec![
        step("merge", &["coder_b", "coder_a"], "integrator", "gpt", 1),
        step("coder_b", &["plan"], "coder", "anthropic", 1),
        step("plan", &[], "architect", "gpt", 1),
        step("coder_a", &["plan"], "coder", "local", 1),
    ]);
    let first_order = plan.topological_order().unwrap();
    assert_eq!(first_order, ["plan", "coder_a", "coder_b", "merge"]);
    plan.steps.reverse();
    assert_eq!(plan.topological_order().unwrap(), first_order);
    let mut run = WorkflowRun::new(plan).unwrap();
    run.start("plan").unwrap();
    run.complete("plan", 1, Outcome::Succeeded).unwrap();
    assert_eq!(run.ready_steps(), ["coder_a", "coder_b"]);
    let a = run.start("coder_a").unwrap();
    let b = run.start("coder_b").unwrap();
    assert_eq!(a.provider_id, "local");
    assert_eq!(b.provider_id, "anthropic");
    run.complete("coder_b", 1, Outcome::Succeeded).unwrap();
    assert!(run.ready_steps().is_empty());
    run.complete("coder_a", 1, Outcome::Succeeded).unwrap();
    assert_eq!(run.ready_steps(), ["merge"]);
}

#[test]
fn terminal_failure_skips_all_downstream_but_allows_independent_branches() {
    let mut run = WorkflowRun::new(definition(vec![
        step("grandchild", &["child"], "reviewer", "local", 1),
        step("child", &["root"], "writer", "local", 1),
        step("root", &[], "researcher", "local", 2),
        step("parallel", &[], "coder", "local", 1),
    ]))
    .unwrap();
    assert_eq!(run.ready_steps(), ["parallel", "root"]);
    run.start("root").unwrap();
    assert_eq!(
        run.complete("root", 1, Outcome::Failed).unwrap(),
        Status::AwaitingRetry
    );
    assert_eq!(run.progress("child").unwrap().status, Status::Pending);
    run.retry("root").unwrap();
    run.start("root").unwrap();
    assert_eq!(
        run.complete("root", 2, Outcome::Failed).unwrap(),
        Status::Failed
    );
    assert_eq!(run.progress("child").unwrap().status, Status::Skipped);
    assert_eq!(run.progress("grandchild").unwrap().status, Status::Skipped);
    assert_eq!(run.ready_steps(), ["parallel"]);
    assert!(matches!(
        run.retry("root"),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert!(!run.finished());
    run.start("parallel").unwrap();
    run.complete("parallel", 1, Outcome::Succeeded).unwrap();
    assert!(run.finished());
}

#[test]
fn duplicate_completion_is_idempotent_conflict_is_refused_and_retry_is_explicit() {
    let mut run =
        WorkflowRun::new(definition(vec![step("only", &[], "writer", "local", 1)])).unwrap();
    let started = run.start("only").unwrap();
    assert_eq!(started.attempt, 1);
    assert_eq!(
        run.complete("only", 1, Outcome::Succeeded).unwrap(),
        Status::Succeeded
    );
    assert_eq!(
        run.complete("only", 1, Outcome::Succeeded).unwrap(),
        Status::Succeeded
    );
    assert!(matches!(
        run.complete("only", 1, Outcome::Failed),
        Err(WorkflowError::ConflictingCompletion(_))
    ));
    assert!(matches!(run.start("only"), Err(WorkflowError::NotReady(_))));
    assert!(matches!(
        run.retry("only"),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert!(matches!(
        run.complete("only", 2, Outcome::Succeeded),
        Err(WorkflowError::AttemptMismatch { .. })
    ));
}

#[test]
fn skip_cancel_and_global_cancel_propagate_without_reviving_tasks() {
    let mut run = WorkflowRun::new(definition(vec![
        step("a", &[], "researcher", "local", 2),
        step("b", &["a"], "writer", "local", 1),
        step("c", &["b"], "reviewer", "local", 1),
        step("independent", &[], "coder", "local", 1),
    ]))
    .unwrap();
    let started = run.start("a").unwrap();
    assert!(run.cancel_step("a").unwrap());
    assert!(!run.cancel_step("a").unwrap());
    assert_eq!(run.progress("b").unwrap().status, Status::Skipped);
    assert_eq!(run.progress("c").unwrap().status, Status::Skipped);
    assert!(matches!(
        run.complete("a", started.attempt, Outcome::Succeeded),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert_eq!(run.ready_steps(), ["independent"]);
    assert!(run.cancel());
    assert!(!run.cancel());
    assert_eq!(
        run.progress("independent").unwrap().status,
        Status::Cancelled
    );
    assert!(run.finished());
    assert!(run.ready_steps().is_empty());
    assert!(matches!(
        run.start("independent"),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert!(matches!(
        run.retry("a"),
        Err(WorkflowError::InvalidTransition(_))
    ));
    let restored = WorkflowRun::from_json_bounded(&run.to_json().unwrap()).unwrap();
    assert_eq!(restored, run);

    let mut skipped = WorkflowRun::new(definition(vec![
        step("parent", &[], "researcher", "local", 1),
        step("child", &["parent"], "writer", "local", 1),
    ]))
    .unwrap();
    assert!(skipped.skip("parent").unwrap());
    assert!(!skipped.skip("parent").unwrap());
    assert_eq!(skipped.progress("child").unwrap().status, Status::Skipped);
}

#[test]
fn invalid_dag_and_hostile_definitions_rejected_during_deserialization() {
    let mut bad = definition(vec![
        step("a", &["b"], "writer", "local", 1),
        step("b", &["a"], "reviewer", "local", 1),
    ]);
    assert!(matches!(bad.validate(), Err(WorkflowError::Cycle)));
    assert!(WorkflowDefinition::from_json_bounded(&serde_json::to_vec(&bad).unwrap()).is_err());
    bad.steps[1].depends_on = vec!["nonexistent".into()];
    assert!(matches!(
        bad.validate(),
        Err(WorkflowError::MissingDependency { .. })
    ));
    bad.steps[1].depends_on = vec!["a".into(), "a".into()];
    assert!(matches!(
        bad.validate(),
        Err(WorkflowError::DuplicateDependency { .. })
    ));
    bad.steps[1].id = "a".into();
    assert!(matches!(bad.validate(), Err(WorkflowError::DuplicateId(_))));
    bad.steps = vec![step("self", &["self"], "role", "provider", 1)];
    assert!(matches!(bad.validate(), Err(WorkflowError::Cycle)));
    bad.steps[0].depends_on.clear();
    bad.steps[0].provider_id.clear();
    assert!(matches!(
        bad.validate(),
        Err(WorkflowError::InvalidField("step.provider_id"))
    ));
    bad.steps[0].provider_id = " \n ".into();
    assert!(bad.validate().is_err());
    bad.steps[0].provider_id = "provider".into();
    bad.steps[0].role = "\0".into();
    assert!(bad.validate().is_err());
    bad.steps[0].role = "writer".into();
    bad.steps[0].instruction = "x".repeat(4097);
    assert!(bad.validate().is_err());
    bad.steps[0].instruction = "fine".into();
    bad.steps[0].id = "../escape".into();
    assert!(bad.validate().is_err());
    bad.steps[0].id = "safe".into();
    bad.steps[0].max_attempts = 0;
    assert!(bad.validate().is_err());
    bad.steps[0].max_attempts = 11;
    assert!(bad.validate().is_err());
    bad.steps[0].max_attempts = 1;
    bad.steps[0].instruction = "okay".into();
    bad.steps[0].depends_on.clear();
    assert!(bad.validate().is_ok());
    bad.schema_version = 999;
    assert!(matches!(
        bad.validate(),
        Err(WorkflowError::UnsupportedVersion(999))
    ));
    bad.schema_version = SCHEMA_VERSION;
    bad.steps = (0..65)
        .map(|i| step(&format!("s{i}"), &[], "role", "provider", 1))
        .collect();
    assert!(matches!(bad.validate(), Err(WorkflowError::StepCount)));

    let too_large = vec![b' '; MAX_JSON_BYTES + 1];
    assert!(matches!(
        WorkflowDefinition::from_json_bounded(&too_large),
        Err(WorkflowError::InputTooLarge)
    ));
    let with_unknown = br#"{"schema_version":1,"workflow_id":"a","steps":[],"auto_grants":true}"#;
    assert!(WorkflowDefinition::from_json_bounded(with_unknown).is_err());
}

#[test]
fn tampered_run_state_is_rejected_even_from_direct_serde() {
    let plan = definition(vec![
        step("root", &[], "researcher", "local", 1),
        step("child", &["root"], "writer", "local", 1),
    ]);
    let run = WorkflowRun::new(plan).unwrap();
    let mut raw: serde_json::Value = serde_json::from_slice(&run.to_json().unwrap()).unwrap();
    raw["states"]["child"]["status"] = serde_json::json!("succeeded");
    raw["states"]["child"]["attempts"] = serde_json::json!(1);
    raw["states"]["child"]["last_completion"] =
        serde_json::json!({"attempt":1,"outcome":"succeeded"});
    let malformed = serde_json::to_vec(&raw).unwrap();
    assert!(WorkflowRun::from_json_bounded(&malformed).is_err());
    assert!(serde_json::from_slice::<WorkflowRun>(&malformed).is_err());
    raw["states"]["child"]["status"] = serde_json::json!("pending");
    raw["states"]["child"]["attempts"] = serde_json::json!(0);
    raw["states"]["child"]["last_completion"] = serde_json::Value::Null;
    raw["states"]["root"]["status"] = serde_json::json!("running");
    raw["states"]["root"]["attempts"] = serde_json::json!(0);
    assert!(serde_json::from_value::<WorkflowRun>(raw).is_err());
}

#[test]
fn restart_blocks_running_attempt_and_preserves_attempt_identity() {
    let plan = definition(vec![
        step("researcher", &[], "researcher", "remote", 2),
        step("writer", &["researcher"], "writer", "local", 1),
        step("reviewer", &["writer"], "reviewer", "local", 1),
        step("parallel", &[], "coder", "local", 1),
    ]);
    let mut live = WorkflowRun::new(plan).unwrap();
    let issued = live.start("researcher").unwrap();
    assert_eq!(issued.attempt, 1);
    // Live-session idempotency is unchanged; only *restored* attempts block.
    assert_eq!(live.start("researcher").unwrap(), issued);
    let persisted = live.to_json().unwrap();
    let mut recovered = WorkflowRun::from_json_bounded(&persisted).unwrap();
    let progress = recovered.progress("researcher").unwrap();
    assert_eq!(progress.status, Status::Blocked);
    assert_eq!(progress.attempts, issued.attempt);
    assert_eq!(progress.last_completion, None);
    assert_eq!(recovered.ready_steps(), ["parallel"]);
    assert!(!recovered.finished());
    assert!(matches!(
        recovered.start("researcher"),
        Err(WorkflowError::NotReady(_))
    ));
    assert!(matches!(
        recovered.start("writer"),
        Err(WorkflowError::NotReady(_))
    ));
    assert!(matches!(
        recovered.complete("researcher", issued.attempt, Outcome::Succeeded),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert!(matches!(
        recovered.retry("researcher"),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert_eq!(
        recovered.progress("researcher").unwrap().attempts,
        issued.attempt
    );

    // Direct serde deserialization must not bypass restart quarantine.
    let direct: WorkflowRun = serde_json::from_slice(&persisted).unwrap();
    assert_eq!(
        direct.progress("researcher").unwrap().status,
        Status::Blocked
    );
    let once = recovered.to_json().unwrap();
    let mut twice: WorkflowRun = serde_json::from_slice(&once).unwrap();
    assert_eq!(twice, recovered);
    assert!(matches!(
        twice.start("researcher"),
        Err(WorkflowError::NotReady(_))
    ));

    // Only an explicit trusted-host action abandons the uncertain attempt.
    assert!(recovered.skip("researcher").unwrap());
    assert_eq!(
        recovered.progress("researcher").unwrap().status,
        Status::Skipped
    );
    assert_eq!(recovered.progress("researcher").unwrap().attempts, 1);
    assert_eq!(
        recovered.progress("writer").unwrap().status,
        Status::Skipped
    );
    assert_eq!(
        recovered.progress("reviewer").unwrap().status,
        Status::Skipped
    );
    assert_eq!(recovered.ready_steps(), ["parallel"]);
    let after_abandon = WorkflowRun::from_json_bounded(&recovered.to_json().unwrap()).unwrap();
    assert_eq!(after_abandon, recovered);
}

#[test]
fn second_attempt_restart_does_not_reissue_or_reuse_previous_attempt() {
    let mut run = WorkflowRun::new(definition(vec![
        step("root", &[], "researcher", "local", 3),
        step("dependent", &["root"], "writer", "local", 1),
    ]))
    .unwrap();
    let first = run.start("root").unwrap();
    assert_eq!(
        run.complete("root", first.attempt, Outcome::Failed)
            .unwrap(),
        Status::AwaitingRetry
    );
    assert!(run.retry("root").unwrap());
    let second = run.start("root").unwrap();
    assert_eq!(second.attempt, 2);
    let mut recovered = WorkflowRun::from_json_bounded(&run.to_json().unwrap()).unwrap();
    let progress = recovered.progress("root").unwrap();
    assert_eq!(progress.status, Status::Blocked);
    assert_eq!(progress.attempts, 2);
    assert_eq!(
        progress
            .last_completion
            .as_ref()
            .map(|entry| (entry.attempt, entry.outcome)),
        Some((1, Outcome::Failed))
    );
    assert!(matches!(
        recovered.start("root"),
        Err(WorkflowError::NotReady(_))
    ));
    assert!(matches!(
        recovered.complete("root", 1, Outcome::Succeeded),
        Err(WorkflowError::AttemptMismatch { .. })
    ));
    assert!(matches!(
        recovered.complete("root", 2, Outcome::Succeeded),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert!(matches!(
        recovered.retry("root"),
        Err(WorkflowError::InvalidTransition(_))
    ));
    assert_eq!(recovered.ready_steps(), Vec::<String>::new());
    assert!(recovered.cancel_step("root").unwrap());
    assert_eq!(
        recovered.progress("dependent").unwrap().status,
        Status::Skipped
    );
    let restored = WorkflowRun::from_json_bounded(&recovered.to_json().unwrap()).unwrap();
    assert_eq!(restored.progress("root").unwrap().attempts, 2);
    assert_eq!(restored.progress("root").unwrap().status, Status::Cancelled);
    assert!(matches!(
        WorkflowRun::from_json_bounded(&restored.to_json().unwrap())
            .unwrap()
            .start("root"),
        Err(WorkflowError::NotReady(_))
    ));
}

#[test]
fn global_cancellation_resolves_restart_blocked_attempt_without_replay() {
    let mut live = WorkflowRun::new(definition(vec![
        step("a", &[], "agent", "local", 2),
        step("b", &["a"], "agent", "local", 1),
    ]))
    .unwrap();
    live.start("a").unwrap();
    let mut restored = WorkflowRun::from_json_bounded(&live.to_json().unwrap()).unwrap();
    assert_eq!(restored.progress("a").unwrap().status, Status::Blocked);
    assert!(restored.cancel());
    assert!(restored.finished());
    assert_eq!(restored.progress("a").unwrap().status, Status::Cancelled);
    assert_eq!(restored.progress("a").unwrap().attempts, 1);
    let recovered = WorkflowRun::from_json_bounded(&restored.to_json().unwrap()).unwrap();
    assert_eq!(recovered, restored);
}

#[test]
fn restored_run_refuses_impossible_retry_history_and_oversized_json() {
    let original = WorkflowRun::new(definition(vec![step(
        "research",
        &[],
        "researcher",
        "local",
        3,
    )]))
    .unwrap();
    let mut raw: serde_json::Value = serde_json::from_slice(&original.to_json().unwrap()).unwrap();
    raw["states"]["research"]["status"] = serde_json::json!("running");
    raw["states"]["research"]["attempts"] = serde_json::json!(2);
    raw["states"]["research"]["last_completion"] =
        serde_json::json!({"attempt":1,"outcome":"succeeded"});
    assert!(serde_json::from_value::<WorkflowRun>(raw.clone()).is_err());

    // A valid retry must follow exactly the preceding failed attempt.
    raw["states"]["research"]["last_completion"] =
        serde_json::json!({"attempt":1,"outcome":"failed"});
    assert!(serde_json::from_value::<WorkflowRun>(raw.clone()).is_ok());
    raw["states"]["research"]["status"] = serde_json::json!("skipped");
    // An interrupted second attempt can be explicitly abandoned after
    // restart, preserving the previous failure and current attempt number.
    let mut abandoned = serde_json::from_value::<WorkflowRun>(raw).unwrap();
    assert_eq!(
        abandoned.progress("research").unwrap().status,
        Status::Skipped
    );
    assert_eq!(abandoned.progress("research").unwrap().attempts, 2);
    assert!(matches!(
        abandoned.start("research"),
        Err(WorkflowError::NotReady(_))
    ));

    let oversized = vec![b' '; MAX_JSON_BYTES + 1];
    assert!(matches!(
        WorkflowRun::from_json_bounded(&oversized),
        Err(WorkflowError::InputTooLarge)
    ));
}
