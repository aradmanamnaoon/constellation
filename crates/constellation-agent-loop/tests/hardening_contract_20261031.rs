//! Offline security regression tests for the model-to-trusted-host boundary.
//! The coding proposal is an explicit MOCK; no model/provider is called.
//! Every actual process and write is separately scoped and approved by this
//! test's trusted host. The compiler invocation below is fixed fixture code.

use std::{fs, path::PathBuf, process::Command, time::Duration};

use constellation_agent_loop::{
    parse_model_decision, AgentTurn, LoopError, ProposedAction, TurnEvent, TurnStatus,
    MAX_MODEL_ATTEMPTS,
};
use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId};
use constellation_tools::{
    CancellationToken, ExecutionOutput, LocalExecutor, OperationContext, OperationOutcome,
    OperationRequest, ToolError,
};

const SOURCE: &str = "pub fn double(value: u32) -> u32 { value * 2 }\n";
const SECRET: &str = "private contents that must not influence public pre-consent output";
const JOURNAL_KEY: [u8; 32] = [0x7b; 32]; // Test-only fixture key.

fn context() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn child_test_request() -> OperationRequest {
    OperationRequest::Process {
        executable: std::env::current_exe().unwrap(),
        working_dir: PathBuf::new(),
        args: vec![
            "--exact".into(),
            "approved_source_contract_child".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ],
        timeout: Duration::from_secs(30),
    }
}

#[test]
fn no_pre_consent_file_metadata_or_existence_oracle() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(workspace.path().join("private.txt"), SECRET).unwrap();
    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();

    // Both existing and missing targets produce the same public shape.
    // The executor has not prepared either read, so contents, existence,
    // length, SHA and canonical path have not been revealed to the model.
    for name in ["private.txt", "missing.txt"] {
        let mut turn = AgentTurn::new(context());
        let mock = serde_json::json!({"type":"tool","action":"read_file","path":name});
        let event = turn.submit_model(&mock.to_string(), &executor).unwrap();
        let id = match event {
            TurnEvent::ApprovalRequired {
                operation_id,
                preview,
            } => {
                assert_eq!(preview.action, ProposedAction::ReadFile);
                assert_eq!(preview.relative_path, name);
                assert_eq!(preview.bytes, None);
                assert_eq!(preview.sha256, None);
                operation_id
            }
            TurnEvent::Finished(_) => panic!("read proposal bypassed consent"),
        };
        assert!(turn.pending_for_trusted_host().is_none());
        assert!(turn.pending_create_content_for_native_review().is_none());
        assert_eq!(turn.model_attempts(), 1);
        turn.deny(&executor, id).unwrap();
        assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    }

    // Only an independent trusted-host action may resolve the content hash.
    let mut turn = AgentTurn::new(context());
    turn.submit_model(
        r#"{"type":"tool","action":"read_file","path":"private.txt"}"#,
        &executor,
    )
    .unwrap();
    assert!(turn.pending_for_trusted_host().is_none());
    let (_, prepared) = turn
        .prepare_pending_read_for_native_review(&executor)
        .unwrap();
    let constellation_tools::OperationDetails::Read { bytes, .. } = prepared.preview().details
    else {
        panic!("trusted review did not receive read metadata");
    };
    assert_eq!(bytes, SECRET.len());
}

#[test]
fn all_attempts_are_charged_and_unicode_paths_are_rejected() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = LocalExecutor::new(workspace.path(), 16_384, 16_384).unwrap();
    let mut turn = AgentTurn::new(context());
    let attacks = [
        "src/abc\u{202e}txt.rs",  // bidi override
        "src/abc\u{2066}txt.rs",  // isolate
        "src/abc\u{200b}.rs",     // zero-width space
        "src/abc\u{034f}.rs",     // combining grapheme joiner
        "src/abc\u{00ad}.rs",     // soft hyphen
        "src/abc\u{feff}.rs",     // zero-width no-break space
        "src/abc\u{2215}file.rs", // division slash lookalike
        "src/abc\u{ff0f}file.rs", // full-width slash
        "src/abc\u{1d173}.rs",    // invisible musical symbol
        "src/abc\u{13430}.rs",    // hieroglyph format control
        "src/аbc.rs",             // mixed-script Cyrillic a
    ];
    for (index, path) in attacks.iter().enumerate() {
        let raw = serde_json::json!({
            "type":"tool", "action":"create_file", "path":path, "content":"content"
        });
        assert_eq!(
            parse_model_decision(&raw.to_string()).err(),
            Some(LoopError::InvalidProposal)
        );
        assert_eq!(
            turn.submit_model(&raw.to_string(), &executor).err(),
            Some(LoopError::InvalidProposal)
        );
        assert_eq!(turn.model_attempts(), (index + 1) as u8);
        assert_eq!(turn.tool_steps(), 0);
        assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    }
    // A bad path rejected by the executor, malformed JSON, and oversized input
    // still consume attempts, even though no operation is prepared.
    for malformed in [
        r#"{"type":"tool","action":"create_file","path":"../escape.rs","content":"x"}"#.to_owned(),
        "not json".to_owned(),
        " ".repeat(16_385),
    ] {
        assert!(turn.submit_model(&malformed, &executor).is_err());
    }
    assert_eq!(turn.model_attempts(), 14);
    for attempt in 15..=MAX_MODEL_ATTEMPTS {
        assert_eq!(
            turn.submit_model("{}", &executor).err(),
            Some(LoopError::InvalidProposal)
        );
        assert_eq!(turn.model_attempts(), attempt);
    }
    assert_eq!(
        turn.submit_model("{}", &executor).err(),
        Some(LoopError::StepLimit)
    );
    assert_eq!(turn.model_attempts(), MAX_MODEL_ATTEMPTS);
    assert_eq!(turn.status(), TurnStatus::Cancelled);
    assert!(!workspace.path().join("escape.rs").exists());
}

#[test]
fn path_parser_rejects_traversal_drive_unc_and_ambiguous_windows_names() {
    for path in [
        "../secret.txt",
        "docs/../../secret.txt",
        r"..\secret.txt",
        r"docs\..\secret.txt",
        r"C:\secret.txt",
        "C:/secret.txt",
        r"\\server\share\secret.txt",
        "/absolute/path.txt",
        "//server/share.txt",
        "docs//file.txt",
        "docs/./file.txt",
        "docs/",
        "docs/secret.txt:ads",
        "docs/filename.",
        "docs/filename ",
        "docs/ file.txt",
        "docs/file?.txt",
    ] {
        for action in ["read_file", "create_file"] {
            let proposal = if action == "read_file" {
                serde_json::json!({"type":"tool", "action":action, "path":path})
            } else {
                serde_json::json!({
                    "type":"tool", "action":action, "path":path, "content":"test"
                })
            };
            assert_eq!(
                parse_model_decision(&proposal.to_string()).err(),
                Some(LoopError::InvalidProposal),
                "should fail before preparing {action} {path:?}"
            );
        }
    }
    for acceptable in ["src/answer.rs", "docs/My notes-1.md", "readme.txt"] {
        let proposal = serde_json::json!({
            "type":"tool", "action":"read_file", "path":acceptable
        });
        assert!(
            parse_model_decision(&proposal.to_string()).is_ok(),
            "rejected normalized portable path: {acceptable}"
        );
    }
}

#[test]
fn unapproved_denial_and_approved_cancellation_have_distinct_durable_events() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    let journal = audit.path().join("secure.jsonl");
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        &journal,
        JOURNAL_KEY,
    )
    .unwrap();
    let principal = context();
    let mut turn = AgentTurn::new(principal);
    let preview =
        r#"{"type":"tool","action":"create_file","path":"cancelled.rs","content":"never written"}"#;

    let pending_id = match turn.submit_model(preview, &executor).unwrap() {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => panic!("expected unapproved operation"),
    };
    turn.cancel(&executor).unwrap();
    assert_eq!(turn.status(), TurnStatus::Cancelled);
    assert_eq!(executor.receipt(pending_id), None); // denial event, no execution attempt

    let mut approved_turn = AgentTurn::new(principal);
    let approved_id = match approved_turn.submit_model(preview, &executor).unwrap() {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        _ => unreachable!(),
    };
    let mut gate = PermissionGate::new();
    let (_, prepared) = approved_turn.pending_for_trusted_host().unwrap();
    gate.grant(prepared.scope().clone()).unwrap();
    let approval = executor.approve(&gate, approved_id, prepared).unwrap();
    assert_eq!(
        approved_turn.cancel(&executor).err(),
        Some(LoopError::ApprovedCancellationRequired)
    );
    assert_eq!(approved_turn.status(), TurnStatus::AwaitingApproval);
    let mut forged = approval.clone();
    forged.operation_id = OperationId::new();
    assert_eq!(
        approved_turn
            .cancel_approved(&executor, &gate, &forged)
            .err(),
        Some(LoopError::WrongState)
    );
    assert_eq!(approved_turn.status(), TurnStatus::AwaitingApproval);
    let cancelled = approved_turn
        .cancel_approved(&executor, &gate, &approval)
        .unwrap();
    assert_eq!(cancelled.outcome, OperationOutcome::Cancelled);
    assert_eq!(approved_turn.status(), TurnStatus::Cancelled);
    assert_eq!(executor.receipt(approved_id), Some(cancelled.clone()));
    assert!(!workspace.path().join("cancelled.rs").exists());

    let events: Vec<String> = fs::read_to_string(&journal)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["record"]["event"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(events, ["deny_scoped", "approve_scoped", "start", "finish"]);
    drop(executor);
    let reopened = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        16_384,
        &journal,
        JOURNAL_KEY,
    )
    .unwrap();
    assert_eq!(reopened.receipt(approved_id), Some(cancelled));
    let mut replay = AgentTurn::new(principal);
    replay.submit_model(preview, &reopened).unwrap();
    let (_, prepared) = replay.pending_for_trusted_host().unwrap();
    assert!(matches!(
        reopened.approve(&gate, pending_id, prepared),
        Err(ToolError::OperationReplay)
    ));
    assert!(matches!(
        reopened.approve(&gate, approved_id, prepared),
        Err(ToolError::OperationReplay)
    ));
}

#[test]
fn mock_create_fixes_the_same_real_process_test_with_independent_approvals() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    fs::create_dir(workspace.path().join("src")).unwrap();
    let source = workspace.path().join("src/repaired.rs");
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        16_384,
        32_768,
        audit.path().join("journal.jsonl"),
        JOURNAL_KEY,
    )
    .unwrap();
    let principal = context();
    let mut gate = PermissionGate::new();

    let red = executor.prepare(principal, child_test_request()).unwrap();
    let process_scope = red.scope().clone();
    let process_digest = red.digest().to_owned();
    let red_id = OperationId::new();
    assert!(matches!(
        executor.approve(&gate, red_id, &red),
        Err(ToolError::PermissionDenied)
    ));
    gate.grant(process_scope.clone()).unwrap();
    let red_approval = executor.approve(&gate, red_id, &red).unwrap();
    let (red_receipt, red_output) = executor
        .execute(&gate, &red_approval, red, &CancellationToken::new())
        .unwrap();
    assert_eq!(red_receipt.outcome, OperationOutcome::Failed);
    assert_eq!(red_receipt.exit_code, Some(101)); // Actual child Rust test assertion failed.
    assert!(matches!(red_output, Some(ExecutionOutput::Process(_))));
    assert!(!source.exists());
    assert!(gate.revoke(&process_scope));

    let mut turn = AgentTurn::new(principal);
    let mock_proposal = serde_json::json!({
        "type":"tool", "action":"create_file", "path":"src/repaired.rs", "content":SOURCE
    });
    let create_id = match turn
        .submit_model(&mock_proposal.to_string(), &executor)
        .unwrap()
    {
        TurnEvent::ApprovalRequired {
            operation_id,
            preview,
        } => {
            assert_eq!(preview.action, ProposedAction::CreateFile);
            assert_eq!(preview.relative_path, "src/repaired.rs");
            operation_id
        }
        _ => panic!("mock model must stop at explicit approval"),
    };
    let (_, prepared) = turn.pending_for_trusted_host().unwrap();
    let create_scope = prepared.scope().clone();
    assert!(matches!(
        executor.approve(&gate, create_id, prepared),
        Err(ToolError::PermissionDenied)
    ));
    gate.grant(create_scope.clone()).unwrap();
    let create_approval = executor.approve(&gate, create_id, prepared).unwrap();
    assert!(!source.exists());
    let (written, output) = turn
        .dispatch_approved(
            &executor,
            &gate,
            &create_approval,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(written.outcome, OperationOutcome::Succeeded);
    assert!(matches!(output, Some(ExecutionOutput::Write(_))));
    assert_eq!(fs::read_to_string(&source).unwrap(), SOURCE);
    assert!(gate.revoke(&create_scope));

    let green = executor.prepare(principal, child_test_request()).unwrap();
    assert_eq!(green.digest(), process_digest);
    assert_eq!(green.scope(), &process_scope);
    let green_id = OperationId::new();
    assert_ne!(green_id, red_id);
    assert!(matches!(
        executor.approve(&gate, green_id, &green),
        Err(ToolError::PermissionDenied)
    ));
    gate.grant(process_scope).unwrap();
    let green_approval = executor.approve(&gate, green_id, &green).unwrap();
    let (green_receipt, green_output) = executor
        .execute(&gate, &green_approval, green, &CancellationToken::new())
        .unwrap();
    assert_eq!(green_receipt.outcome, OperationOutcome::Succeeded);
    assert_eq!(green_receipt.exit_code, Some(0));
    let Some(ExecutionOutput::Process(log)) = green_output else {
        panic!("same real Process test must produce an execution output");
    };
    assert!(String::from_utf8_lossy(&log.stdout).contains("SOURCE_CONTRACT_VERIFIED"));
    assert!(matches!(
        executor.execute(
            &gate,
            &red_approval,
            executor.prepare(principal, child_test_request()).unwrap(),
            &CancellationToken::new(),
        ),
        Err(ToolError::OperationReplay)
    ));
    assert_eq!(executor.receipt(red_id), Some(red_receipt));
    assert_eq!(executor.receipt(green_id), Some(green_receipt));
}

/// Launched solely as the explicit, independently approved test executable.
#[test]
#[ignore = "launched by trusted LocalExecutor::Process in integration test"]
fn approved_source_contract_child() {
    let source = std::fs::read_to_string("src/repaired.rs")
        .expect("missing source makes the real test process fail");
    assert_eq!(source, SOURCE);
    let status = Command::new("rustc")
        .args([
            "--crate-name",
            "approved_source",
            "--edition=2021",
            "--crate-type=lib",
            "--emit=metadata",
            "src/repaired.rs",
            "-o",
            "compiled_source.rmeta",
        ])
        .status()
        .expect("rustc available in offline test environment");
    assert!(
        status.success(),
        "compiler must typecheck the repaired source"
    );
    println!("SOURCE_CONTRACT_VERIFIED");
}
