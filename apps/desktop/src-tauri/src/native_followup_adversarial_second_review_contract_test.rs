//! Standalone structural regression checks for the prime-owned final follow-up.
//!
//! These tests intentionally do NOT alter lib.rs or register a new Cargo module.
//! Compile and run using:
//!   rustc --edition=2021 --test src/native_followup_adversarial_second_review_contract_test.rs
//! They verify current native source call sequencing, not live OS-vault,
//! concurrency, network, or runtime side effects. Existing native Cargo tests
//! separately cover actual approved execution, source MAC, journal rollback,
//! one-use revisions and Core persistence.

const NATIVE: &str = include_str!("lib.rs");
const MODEL_BINDING: &str = include_str!("native_model_binding.rs");
const NATIVE_TURN: &str = include_str!("native_agent_turn.rs");
const AGENT_LOOP: &str = include_str!("../../../../crates/constellation-agent-loop/src/lib.rs");
const CORE: &str = include_str!("../../../../crates/constellation-core/src/store.rs");
const HOST: &str = include_str!("tool_host.rs");
const ADAPTER: &str = include_str!("../../../../crates/constellation-provider-adapters/src/lib.rs");

/// Extract a complete, zero-indented top-level Rust function. No regex
/// matching across unrelated commands or unit-test fixtures.
fn function<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source.find(signature).unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let end = tail.find("\n}\n").unwrap_or_else(|| panic!("missing end for {signature}"));
    &tail[..end + 2]
}

fn earlier(section: &str, first: &str, next: &str) {
    let a = section.find(first).unwrap_or_else(|| panic!("missing first boundary {first}"));
    let b = section.find(next).unwrap_or_else(|| panic!("missing next boundary {next}"));
    assert!(a < b, "unsafe order: {first} occurs after {next}");
}

#[test]
fn provider_swap_guard_precedes_any_signed_spend_or_network_request() {
    let proposal = function(NATIVE, "async fn studio_propose_local_file_operation(");
    let final_followup = function(NATIVE, "async fn studio_continue_after_native_tool(");
    assert!(proposal.contains(".record_with_profile("));
    assert!(proposal.contains("&provider,"));
    earlier(proposal, ".record_with_profile(", "register_native_review(");
    assert!(final_followup.contains("sources.verify_provider_configuration("));
    earlier(
        final_followup,
        "sources.verify_provider_configuration(",
        "reserve_single_final_followup(2)",
    );
    earlier(
        final_followup,
        "sources.verify_provider_configuration(",
        "model_proposal_budget::reserve_request(",
    );
    earlier(
        final_followup,
        "sources.verify_provider_configuration(",
        "model_text_reply(",
    );
    assert!(MODEL_BINDING.contains("provider_profile_sha256 TEXT NOT NULL"));
    assert!(MODEL_BINDING.contains("provider_profile_sha256.to_owned()"));
    let check = function(MODEL_BINDING, "pub(crate) fn verify_provider_configuration(");
    assert!(check.contains("self.verify("), "source route also requires authenticated row MAC");
    assert!(check.contains("pinned != fingerprint"));
}

#[test]
fn native_approval_and_observation_do_not_restore_stale_consent() {
    let approve = function(NATIVE, "fn studio_approve_local_file_operation(");
    assert!(approve.contains("verify_native_model_review(&state, operation_id)?"));
    assert!(approve.contains("native_consent::confirm_native_consent"));
    earlier(
        approve,
        "native_consent::confirm_native_consent",
        "execute_prepared_file_if_task_pending(",
    );
    assert!(approve.contains("verify_completed_native_model_observation("));
    assert!(approve.contains("value[\"automatic_resume_allowed\"] = json!(false)"));
    assert!(!approve.contains("studio_continue_after_native_tool("));

    let verified = function(NATIVE, "fn verify_native_model_review(");
    assert!(verified.contains("verified_provider("));
    assert!(verified.contains("verify_provider_configuration("));
    assert!(verified.contains("native_agent_resume::verify_review("));
}

#[test]
fn one_use_signed_revision_precedes_budget_then_task_transition_then_poll() {
    let final_followup = function(NATIVE, "async fn studio_continue_after_native_tool(");
    earlier(final_followup, "reserve_single_final_followup(2)", "persist_pending_task(");
    earlier(final_followup, "persist_pending_task(", "model_proposal_budget::reserve_request(");
    earlier(final_followup, "model_proposal_budget::reserve_request(", "TaskState::Queued,");
    earlier(final_followup, "TaskState::Queued,", "TaskState::Running,");
    earlier(final_followup, "TaskState::Running,", "model_text_reply(");
    assert!(final_followup.contains("register_task_request("));
    assert!(final_followup.contains("task_tool_boundary"));

    let loop_reserve = function(AGENT_LOOP, "pub fn reserve_single_final_followup(");
    assert!(loop_reserve.contains("expected_revision != 2"));
    assert!(loop_reserve.contains("self.status != TurnStatus::AwaitingModel"));
    assert!(loop_reserve.contains("self.pending.is_some()"));
    assert!(loop_reserve.contains("self.proposals != 1"));
    assert!(loop_reserve.contains("self.attempts != 1"));
    assert!(loop_reserve.contains("self.attempts += 1"));
    assert!(loop_reserve.contains("self.revision += 1"));
}

#[test]
fn generic_runner_cannot_spend_again_on_an_existing_model_proposal() {
    let text_runner = function(NATIVE, "async fn studio_run_task(");
    assert!(text_runner.contains("model_proposal_budget::request_usage("));
    assert!(text_runner.contains(".task_used"));
    earlier(text_runner, "model_proposal_budget::request_usage(", "register_task_request(");
    earlier(text_runner, "model_proposal_budget::request_usage(", "model_text_reply(");
    assert!(text_runner.contains("must use its authenticated one-use continuation"));
}

#[test]
fn cancelling_during_provider_io_cannot_commit_late_success_or_duplicate_message() {
    let final_followup = function(NATIVE, "async fn studio_continue_after_native_tool(");
    assert!(final_followup.contains("await_cancellable_task_request("));
    assert!(final_followup.contains("complete_task_with_message("));
    assert!(final_followup.contains("TaskState::Failed"));
    let cancel = function(NATIVE, "fn studio_cancel_task(");
    assert!(cancel.contains("task_tool_boundary"));
    assert!(cancel.contains("TaskState::Cancelled"));
    assert!(cancel.contains("sender.send(true)"));

    let complete = function(CORE, "pub fn complete_task_with_message(");
    earlier(complete, "transaction_with_behavior(TransactionBehavior::Immediate)", "if task.state != TaskState::Running");
    assert!(complete.contains("tx.execute("));
    assert!(complete.contains("tx.commit()"));
    assert!(complete.contains("task.state = TaskState::Succeeded"));
    assert!(complete.contains("MessageSender::Agent(task.agent_id)"));
    assert!(complete.contains("TaskState::Running"));
}

#[test]
fn attacker_provider_output_is_never_parsed_as_a_second_tool_command() {
    let final_followup = function(NATIVE, "async fn studio_continue_after_native_tool(");
    assert!(final_followup.contains("model_text_reply("));
    assert!(final_followup.contains("complete_task_with_message("));
    assert!(!final_followup.contains("parse_model_decision("));
    assert!(!final_followup.contains("prepare_trusted_model_file_action("));
    assert!(!final_followup.contains("approve_and_execute("));
    assert!(!final_followup.contains("NativeConsentProof"));
    assert!(final_followup.contains("automatic_resume_allowed"));
    let parse = function(ADAPTER, "pub fn parse_text(");
    assert!(parse.contains("tool_calls"));
    assert!(parse.contains("finish_reason"));
    assert!(parse.contains("AdapterError::UnsupportedContent"));

    let bounded = function(NATIVE_TURN, "pub(crate) fn promote_native_tool_observation<");
    assert!(bounded.contains("JournalOperationState::Completed"));
    assert!(bounded.contains("OperationOutcome::Succeeded"));
    assert!(bounded.contains("cancellation.is_cancelled()"));
}

#[test]
fn authenticated_tool_journal_head_fails_closed_on_rollback() {
    let open = function(HOST, "pub(crate) fn open_anchored(");
    assert!(open.contains("OsJournalHeadAnchor::for_path("));
    assert!(open.contains("with_anchored_authenticated_journal("));
    assert!(HOST.contains("independently_anchored_host_reopens_and_detects_valid_suffix_rollback"));
    assert!(HOST.contains("model_denial_journal_has_distinct_identity_and_rejects_signed_tail_rollback"));
}

#[test]
fn task_instructions_and_agent_role_have_no_public_in_place_edit_api() {
    // Scope is *native* exposed mutation methods, not arbitrary compromise
    // of the user's SQLite file. A future editor needs a signed task/prompt
    // fingerprint before new follow-up admission.
    assert!(!CORE.contains("pub fn update_task("));
    assert!(!CORE.contains("pub fn edit_task("));
    assert!(!CORE.contains("pub fn update_agent("));
    assert!(!NATIVE.contains("fn studio_edit_task("));
    assert!(!NATIVE.contains("fn studio_update_agent_role("));
    let runner = function(NATIVE, "async fn studio_continue_after_native_tool(");
    assert!(runner.contains("task_history(task, agent)"));
}
