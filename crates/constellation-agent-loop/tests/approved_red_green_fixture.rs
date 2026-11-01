//! Offline P08 integration evidence: real approved process -> mock model
//! proposal -> trusted CreateNew -> the *same* approved process -> PASS.
//!
//! The proposal is a fixed, explicitly simulated JSON model response. No AI
//! provider, shell, network, desktop consent UI, or billing is involved. This
//! test acts as a trusted host: its explicit grants/approvals simulate the
//! separate human decision, never a capability granted by `AgentTurn`.
//! Unix compiles and executes a generated Rust unit test; Windows compiles its
//! test metadata without linking because the executor deliberately strips the
//! Visual Studio linker environment from child processes.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use constellation_agent_loop::{
    parse_model_decision, AgentTurn, LoopError, ProposedAction, TurnEvent, TurnStatus,
};
use constellation_core::{AgentId, OperationId, PermissionGate, RoomId, TaskId, ToolAction};
use constellation_tools::{
    CancellationToken, ExecutionOutput, ExecutionReceipt, LocalExecutor, OperationContext,
    OperationDetails, OperationOutcome, OperationRequest, ToolError, WriteMode,
};
use sha2::{Digest, Sha256};

const GENERATED_SOURCE: &str = "pub fn double(value: u32) -> u32 { value * 2 }\n\n#[test]\nfn generated_code_contract() {\n    assert_eq!(double(0), 0);\n    assert_eq!(double(21), 42);\n    assert_eq!(double(500), 1_000);\n}\n";
const JOURNAL_KEY: [u8; 32] = [0x5e; 32]; // Fixed test key only; never use in production.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);

fn principal() -> OperationContext {
    OperationContext {
        agent_id: AgentId::new(),
        room_id: RoomId::new(),
        task_id: TaskId::new(),
    }
}

fn process_request() -> OperationRequest {
    OperationRequest::Process {
        executable: std::env::current_exe().expect("test harness absolute executable"),
        working_dir: PathBuf::new(),
        args: vec![
            "--exact".into(),
            "compiled_generated_source_child".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ],
        timeout: PROCESS_TIMEOUT,
    }
}

fn expected_process_hash(output: &constellation_tools::ProcessOutput) -> String {
    let mut digest = Sha256::new();
    digest.update(&output.stdout);
    digest.update(&output.stderr);
    format!("{:x}", digest.finalize())
}

fn process_result(
    receipt: &ExecutionReceipt,
    output: Option<ExecutionOutput>,
) -> constellation_tools::ProcessOutput {
    let Some(ExecutionOutput::Process(output)) = output else {
        panic!("a real bounded child process must produce a process output");
    };
    assert_eq!(
        receipt.output_sha256.as_deref(),
        Some(expected_process_hash(&output).as_str())
    );
    assert_eq!(receipt.exit_code, output.exit_code);
    output
}

#[test]
fn mock_model_trusted_create_new_repairs_real_failing_process_test() {
    let workspace = tempfile::tempdir().unwrap();
    let audit = tempfile::tempdir().unwrap();
    fs::create_dir(workspace.path().join("src")).unwrap();
    let source = workspace.path().join("src/answer.rs");
    let denied_source = workspace.path().join("src/denied.rs");
    let journal = audit.path().join("process-create-process.jsonl");
    let executor = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        32_768,
        32_768,
        &journal,
        JOURNAL_KEY,
    )
    .unwrap();
    let context = principal();
    let mut gate = PermissionGate::new();
    assert!(matches!(
        parse_model_decision(
            r#"{"type":"tool","action":"execute_command","path":"src/answer.rs"}"#
        ),
        Err(LoopError::InvalidProposal)
    )); // The mock model cannot select the real test-process capability.

    // RED: the first *real* LocalExecutor::Process dispatch runs this test
    // binary's child fixture. That fixture actually invokes rustc --test on
    // src/answer.rs. The file is missing, so compilation and the child test
    // fail with an observed nonzero process exit, not a manufactured result.
    let baseline = executor.prepare(context, process_request()).unwrap();
    let process_scope = baseline.scope().clone();
    let baseline_digest = baseline.digest().to_owned();
    assert_eq!(process_scope.action, ToolAction::ExecuteCommand);
    let baseline_id = OperationId::new();
    assert!(matches!(
        executor.approve(&gate, baseline_id, &baseline),
        Err(ToolError::PermissionDenied)
    ));
    gate.grant(process_scope.clone()).unwrap(); // Explicit trusted test-host permission.
    let baseline_approval = executor.approve(&gate, baseline_id, &baseline).unwrap();
    let (red_receipt, red_output) = executor
        .execute(
            &gate,
            &baseline_approval,
            baseline,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(red_receipt.outcome, OperationOutcome::Failed);
    assert_eq!(red_receipt.exit_code, Some(101));
    let red_output = process_result(&red_receipt, red_output);
    // Rust's test harness puts panic details on stderr on some platforms.
    let red_log = format!(
        "{}{}",
        String::from_utf8_lossy(&red_output.stdout),
        String::from_utf8_lossy(&red_output.stderr)
    );
    assert!(
        red_log.contains("RUSTC_FAILED"),
        "child did not invoke compiler: {red_log}"
    );
    assert!(
        red_log.contains("FAILED"),
        "expected actual failing test harness"
    );
    assert!(!source.exists());
    println!(
        "P08_MOCK_REAL_RED: process_exit={:?} outcome={:?} compiler_error=observed",
        red_receipt.exit_code, red_receipt.outcome
    );
    assert!(gate.revoke(&process_scope));

    // Denied MOCK proposal. The model has no ability to grant, approve, or
    // even choose an operation ID. The host rejects it and journals a Deny.
    let mut turn = AgentTurn::new(context);
    let denied_mock_model_proposal = serde_json::json!({
        "type": "tool", "action": "create_file",
        "path": "src/denied.rs", "content": "untrusted mock content"
    });
    let denied_id = match turn
        .submit_model(&denied_mock_model_proposal.to_string(), &executor)
        .unwrap()
    {
        TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
        TurnEvent::Finished(_) => panic!("mock proposal must require approval"),
    };
    assert!(gate
        .authorize(turn.pending_for_trusted_host().unwrap().1.scope())
        .is_err());
    turn.deny(&executor, denied_id).unwrap();
    assert!(!denied_source.exists());
    assert!(executor.receipt(denied_id).is_none()); // Deny record, no effect receipt.
    assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    // A host-persisted revision survives a simulated orchestrator restart.
    // The executor stays separate and its denied operation ID stays consumed.
    let settled = turn.checkpoint_for_host().unwrap();
    let saved = serde_json::to_vec(&settled).unwrap();
    turn = AgentTurn::restore_from_host(
        context,
        serde_json::from_slice(&saved).unwrap(),
        settled.revision(),
    )
    .unwrap();
    assert_eq!(turn.tool_steps(), 1);
    assert_eq!(turn.model_attempts(), 1);

    // The fixed MOCK structured model response proposes only a CreateNew
    // source file, with no shell/process action or permission fields.
    let mock_model_create_proposal = serde_json::json!({
        "type": "tool", "action": "create_file",
        "path": "src/answer.rs", "content": GENERATED_SOURCE
    });
    let source_sha256 = format!("{:x}", Sha256::digest(GENERATED_SOURCE.as_bytes()));
    let model_revision = turn.revision();
    let create_id = match turn
        .submit_model_at_revision(
            model_revision,
            &mock_model_create_proposal.to_string(),
            &executor,
        )
        .unwrap()
    {
        TurnEvent::ApprovalRequired {
            operation_id,
            preview,
        } => {
            assert_eq!(preview.action, ProposedAction::CreateFile);
            assert_eq!(preview.relative_path, "src/answer.rs");
            assert_eq!(preview.bytes, Some(GENERATED_SOURCE.len()));
            assert_eq!(preview.sha256.as_deref(), Some(source_sha256.as_str()));
            operation_id
        }
        TurnEvent::Finished(_) => panic!("untrusted model may not finish with a tool effect"),
    };
    assert_eq!(
        turn.pending_create_content_for_native_review(),
        Some(GENERATED_SOURCE)
    );
    let (pending_id, prepared) = turn.pending_for_trusted_host().unwrap();
    assert_eq!(pending_id, create_id);
    let create_digest = prepared.digest().to_owned();
    let create_scope = prepared.scope().clone();
    assert_eq!(create_scope.action, ToolAction::WriteFile);
    assert_ne!(create_scope, process_scope);
    let OperationDetails::Write {
        bytes,
        sha256,
        mode,
        ..
    } = prepared.preview().details
    else {
        panic!("host requires the immutable prepared CreateNew preview");
    };
    assert_eq!(mode, WriteMode::CreateNew);
    assert_eq!(bytes, GENERATED_SOURCE.len());
    assert_eq!(sha256, source_sha256);
    assert!(matches!(
        executor.approve(&gate, create_id, prepared),
        Err(ToolError::PermissionDenied)
    ));
    gate.grant(create_scope.clone()).unwrap(); // Explicit *independent* host approval.
    let create_approval = executor.approve(&gate, create_id, prepared).unwrap();
    assert!(
        !source.exists(),
        "model proposal and host approval alone cannot write"
    );
    let (create_receipt, create_output) = turn
        .dispatch_approved(
            &executor,
            &gate,
            &create_approval,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(create_receipt.outcome, OperationOutcome::Succeeded);
    assert_eq!(create_receipt.operation_id, create_id);
    assert_eq!(create_receipt.digest, create_digest);
    assert_eq!(
        create_receipt.output_sha256.as_deref(),
        Some(source_sha256.as_str())
    );
    let Some(ExecutionOutput::Write(write)) = create_output else {
        panic!("expected a real CreateNew filesystem effect");
    };
    assert_eq!(write.canonical_file, fs::canonicalize(&source).unwrap());
    assert_eq!(write.bytes_written, GENERATED_SOURCE.len());
    assert_eq!(write.sha256, source_sha256);
    assert_eq!(fs::read(&source).unwrap(), GENERATED_SOURCE.as_bytes());
    assert!(!denied_source.exists());
    assert_eq!(turn.status(), TurnStatus::AwaitingModel);
    assert!(gate.revoke(&create_scope));
    let settled = turn.checkpoint_for_host().unwrap();
    turn = AgentTurn::restore_from_host(
        context,
        serde_json::from_slice(&serde_json::to_vec(&settled).unwrap()).unwrap(),
        settled.revision(),
    )
    .unwrap();
    assert_eq!(turn.tool_steps(), 2);
    assert_eq!(turn.model_attempts(), 2);

    // GREEN: independently regrant the *process* scope and approve a NEW
    // operation ID. The executable, arguments, working directory and timeout
    // are identical to RED. This is a real second Process dispatch: the child
    // compiles src/answer.rs, and on Unix runs its Rust test harness.
    let green = executor.prepare(context, process_request()).unwrap();
    assert_eq!(green.digest(), baseline_digest);
    assert_eq!(green.scope(), &process_scope);
    assert!(gate.authorize(&process_scope).is_err());
    let green_id = OperationId::new();
    assert_ne!(baseline_id, green_id);
    assert!(matches!(
        executor.approve(&gate, green_id, &green),
        Err(ToolError::PermissionDenied)
    ));
    gate.grant(process_scope.clone()).unwrap();
    let green_approval = executor.approve(&gate, green_id, &green).unwrap();
    let (green_receipt, green_output) = executor
        .execute(&gate, &green_approval, green, &CancellationToken::new())
        .unwrap();
    let green_output = process_result(&green_receipt, green_output);
    assert_eq!(
        green_receipt.outcome,
        OperationOutcome::Succeeded,
        "green run: stdout={} stderr={}",
        String::from_utf8_lossy(&green_output.stdout),
        String::from_utf8_lossy(&green_output.stderr)
    );
    assert_eq!(green_receipt.exit_code, Some(0));
    let green_log = String::from_utf8_lossy(&green_output.stdout);
    #[cfg(unix)]
    {
        assert!(
            green_log.contains("GENERATED_SOURCE_TEST_PASSED"),
            "{green_log}"
        );
        assert!(green_log.contains("test result: ok"), "{green_log}");
    }
    #[cfg(windows)]
    assert!(
        green_log.contains("GENERATED_SOURCE_COMPILE_PASSED"),
        "{green_log}"
    );
    assert_eq!(fs::read(&source).unwrap(), GENERATED_SOURCE.as_bytes());
    println!(
        "P08_MOCK_REAL_GREEN: process_exit={:?} outcome={:?} source_sha256={} generated_runtime_executed={}",
        green_receipt.exit_code,
        green_receipt.outcome,
        source_sha256,
        cfg!(unix)
    );

    // Old tokens cannot be replayed, even with the original exact grant.
    let stale_process = executor.prepare(context, process_request()).unwrap();
    assert!(matches!(
        executor.execute(
            &gate,
            &baseline_approval,
            stale_process,
            &CancellationToken::new()
        ),
        Err(ToolError::OperationReplay)
    ));
    let stale_write = executor
        .prepare(
            context,
            OperationRequest::Write {
                path: "src/answer.rs".into(),
                bytes: GENERATED_SOURCE.as_bytes().to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    gate.grant(create_scope).unwrap();
    assert!(matches!(
        executor.execute(
            &gate,
            &create_approval,
            stale_write,
            &CancellationToken::new()
        ),
        Err(ToolError::OperationReplay)
    ));
    let denied_prepared = executor
        .prepare(
            context,
            OperationRequest::Write {
                path: "src/denied.rs".into(),
                bytes: b"untrusted mock content".to_vec(),
                mode: WriteMode::CreateNew,
            },
        )
        .unwrap();
    gate.grant(denied_prepared.scope().clone()).unwrap();
    assert!(matches!(
        executor.approve(&gate, denied_id, &denied_prepared),
        Err(ToolError::OperationReplay)
    ));
    assert!(!denied_source.exists());

    assert!(matches!(
        turn.submit_model(
            r#"{"type":"final","text":"Mock fixture completed"}"#,
            &executor
        ),
        Ok(TurnEvent::Finished(_))
    ));
    assert_eq!(turn.status(), TurnStatus::Complete);
    assert_eq!(turn.tool_steps(), 2); // Denied and accepted source proposals.
    assert_eq!(turn.model_attempts(), 3); // Plus the bounded final response.

    // Exact authenticated, principal-bound journal event sequence: two
    // independently approved real process runs, one denied proposal and one
    // approved CreateNew. The modern scoped events commit room/task/agent,
    // unlike the legacy unscoped names; never weaken the writer to fit the
    // old fixture vocabulary.
    let lines = fs::read_to_string(&journal).unwrap();
    let events: Vec<String> = lines
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["record"]["event"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        events,
        [
            "approve_scoped",
            "start",
            "finish",
            "deny_scoped",
            "approve_scoped",
            "start",
            "finish",
            "approve_scoped",
            "start",
            "finish"
        ]
    );
    println!("P08_MOCK_JOURNAL: authenticated_events={}", events.len());
    assert_eq!(executor.receipt(baseline_id), Some(red_receipt.clone()));
    assert_eq!(executor.receipt(create_id), Some(create_receipt.clone()));
    assert_eq!(executor.receipt(green_id), Some(green_receipt.clone()));
    drop(executor);

    // Journal re-open authenticates all receipts and preserves denied IDs.
    let reopened = LocalExecutor::with_authenticated_journal(
        workspace.path(),
        32_768,
        32_768,
        &journal,
        JOURNAL_KEY,
    )
    .unwrap();
    assert_eq!(reopened.receipt(baseline_id), Some(red_receipt));
    assert_eq!(reopened.receipt(create_id), Some(create_receipt));
    assert_eq!(reopened.receipt(green_id), Some(green_receipt));
    assert!(matches!(
        reopened.approve(&gate, denied_id, &denied_prepared),
        Err(ToolError::OperationReplay)
    ));
    assert_eq!(fs::read(&source).unwrap(), GENERATED_SOURCE.as_bytes());
    assert!(!denied_source.exists());
}

/// Invoked twice by LocalExecutor::Process, never by the default test runner.
/// The nested commands are fixed trusted test-fixture binaries, not model
/// commands. This executes the actual Rust compiler and, on Unix, the
/// resulting Rust test. Windows checks test metadata without linking.
#[test]
#[ignore = "only launched via separately approved bounded LocalExecutor Process"]
fn compiled_generated_source_child() {
    let source = Path::new("src/answer.rs");
    let compiled = std::env::current_dir().unwrap().join(if cfg!(windows) {
        "compiled_generated_contract.rmeta"
    } else {
        "compiled_generated_contract"
    });
    let mut compiler = Command::new("rustc");
    compiler.args(["--edition=2021", "--test", "src/answer.rs"]);
    #[cfg(windows)]
    compiler.arg("--emit=metadata");
    let compile = compiler
        .arg("-o")
        .arg(&compiled)
        .output()
        .expect("the offline Rust toolchain must be installed for this fixture");
    assert!(
        compile.status.success(),
        "RUSTC_FAILED for {}: {}",
        source.display(),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(
        compiled.is_file(),
        "compiler must emit the requested artifact"
    );
    #[cfg(unix)]
    {
        let tests = Command::new(&compiled)
            .args(["--exact", "generated_code_contract", "--nocapture"])
            .output()
            .expect("compiled Rust test executable must launch");
        assert!(
            tests.status.success(),
            "GENERATED_SOURCE_TEST_FAILED: stdout={} stderr={}",
            String::from_utf8_lossy(&tests.stdout),
            String::from_utf8_lossy(&tests.stderr)
        );
        assert!(String::from_utf8_lossy(&tests.stdout).contains("1 passed"));
        println!("GENERATED_SOURCE_TEST_PASSED");
    }
    #[cfg(windows)]
    println!("GENERATED_SOURCE_COMPILE_PASSED");
}
