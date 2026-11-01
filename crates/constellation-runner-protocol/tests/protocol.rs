use constellation_runner_protocol::{
    BoundPermission, Capability, Clock, Endpoint, JobOutcome, Message, MessagePolicy, Pair,
    PairStatus, PermissionScope, ProtocolError, ReceiveOutcome, Secret, Side, TaskAttribution,
    MAX_INSTRUCTION_BYTES, MAX_TRACKED, MAX_WIRE_BYTES,
};
use sha2::{Digest, Sha256};
use std::cell::Cell;

struct FakeClock(Cell<i64>);
impl FakeClock {
    fn new(value: i64) -> Self {
        Self(Cell::new(value))
    }
    fn set(&self, value: i64) {
        self.0.set(value);
    }
}
impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.get()
    }
}

fn secret(seed: u8) -> Secret {
    Secret::from_bytes([seed; 32]).unwrap()
}

fn pair(seed: u8) -> Pair {
    let mut pairing = Pair::enroll("workstation-1", "runner-1", secret(seed)).unwrap();
    pairing.approve().unwrap();
    pairing
}

fn endpoints() -> (Endpoint, Endpoint) {
    (
        Endpoint::new(Side::Workstation, pair(41), MessagePolicy::default()).unwrap(),
        Endpoint::new(Side::Runner, pair(41), MessagePolicy::default()).unwrap(),
    )
}

fn scope() -> PermissionScope {
    PermissionScope {
        capability: Capability::ReadEvidence,
        resource_id: "source-1".into(),
    }
}

fn issue(ws: &mut Endpoint, clock: &impl Clock) -> Vec<u8> {
    ws.issue_job(
        "job-1",
        "agent-1",
        scope(),
        "Summarize trusted evidence; do not run tools",
        30_000,
        clock,
    )
    .unwrap()
}

fn sha(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn bound_owner_run_task_cannot_be_rewritten_and_survives_receipts_and_checkpoints() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let attribution = TaskAttribution {
        owner_id: "owner-1".into(),
        run_id: "run-1".into(),
        task_id: "task-1".into(),
    };
    let job = ws
        .issue_bound_job(
            "job-1",
            "agent-1",
            BoundPermission {
                scope: scope(),
                task: attribution.clone(),
            },
            "Bound summary only",
            30_000,
            &clock,
        )
        .unwrap();
    let wire = String::from_utf8(job.clone()).unwrap();
    for (from, to) in [
        ("owner-1", "owner-2"),
        ("run-1", "run-2"),
        ("task-1", "task-2"),
        ("source-1", "source-2"),
    ] {
        assert_eq!(
            runner.receive(wire.replacen(from, to, 1).as_bytes(), &clock),
            Err(ProtocolError::Authentication)
        );
        assert_eq!(runner.checkpoint_generation(), 0);
    }
    let accepted = runner.receive(&job, &clock).unwrap();
    assert_eq!(accepted.info().task.as_ref(), Some(&attribution));
    let receipt = runner
        .issue_receipt(accepted.info().sequence, 30_000, &clock)
        .unwrap();
    assert_eq!(
        ws.receive(&receipt, &clock).unwrap().info().task,
        Some(attribution.clone())
    );

    let checkpoint = runner.checkpoint().unwrap();
    let mut restarted = Endpoint::recover(
        Side::Runner,
        pair(41),
        MessagePolicy::default(),
        &checkpoint,
        runner.checkpoint_generation(),
    )
    .unwrap();
    assert_eq!(restarted.receive(&job, &clock), Err(ProtocolError::Replay));
    let cancel = ws.issue_cancel("job-1", 30_000, &clock).unwrap();
    let cancelled = restarted.receive(&cancel, &clock).unwrap();
    assert_eq!(cancelled.info().task, Some(attribution));
}

#[test]
fn lost_result_is_reissued_from_checkpoint_without_reexecuting_any_job() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let job = issue(&mut ws, &clock);
    runner.receive(&job, &clock).unwrap();
    let outcome_sha = sha(b"immutable prior outcome only");
    let first = runner
        .issue_result("job-1", JobOutcome::Succeeded, &outcome_sha, 10_000, &clock)
        .unwrap();
    let watermark = runner.checkpoint_generation();
    let checkpoint = runner.checkpoint().unwrap();
    drop(runner);

    clock.set(1_100_000);
    let mut runner = Endpoint::recover(
        Side::Runner,
        pair(41),
        MessagePolicy::default(),
        &checkpoint,
        watermark,
    )
    .unwrap();
    assert_eq!(ws.receive(&first, &clock), Err(ProtocolError::TimeWindow));
    assert_eq!(
        runner.issue_result(
            "job-1",
            JobOutcome::Failed,
            &sha(b"changed"),
            10_000,
            &clock
        ),
        Err(ProtocolError::Conflict)
    );
    let fresh = runner.reissue_result("job-1", 30_000, &clock).unwrap();
    let accepted = ws.receive(&fresh, &clock).unwrap();
    assert!(accepted.is_new());
    assert_eq!(runner.receive(&job, &clock), Err(ProtocolError::TimeWindow));
    assert_eq!(
        runner.issue_cancel("job-1", 10_000, &clock),
        Err(ProtocolError::Role)
    );
    assert_eq!(
        runner.reissue_result("unknown", 30_000, &clock),
        Err(ProtocolError::Conflict)
    );
    assert_eq!(runner.checkpoint_generation(), watermark + 1);

    let duplicate = runner.reissue_result("job-1", 30_000, &clock).unwrap();
    assert!(matches!(
        ws.receive(&duplicate, &clock),
        Ok(ReceiveOutcome::DuplicateResult(_))
    ));
    assert_eq!(ws.receive(&duplicate, &clock), Err(ProtocolError::Replay));
    let receipt = ws
        .issue_receipt(accepted.info().sequence, 30_000, &clock)
        .unwrap();
    assert!(runner.receive(&receipt, &clock).unwrap().is_new());
}

#[test]
fn signed_receipt_reconciliation_matches_original_digest_and_attribution() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let first = ws
        .issue_bound_job(
            "job-1",
            "agent-1",
            BoundPermission {
                scope: scope(),
                task: TaskAttribution {
                    owner_id: "owner-1".into(),
                    run_id: "run-1".into(),
                    task_id: "task-1".into(),
                },
            },
            "Write a draft",
            5_000,
            &clock,
        )
        .unwrap();
    let original_info = runner.receive(&first, &clock).unwrap();
    let original_ack = runner
        .issue_receipt(original_info.info().sequence, 5_000, &clock)
        .unwrap();
    drop(original_ack);
    let watermark = runner.checkpoint_generation();
    let checkpoint = runner.checkpoint().unwrap();
    clock.set(1_100_000);
    let mut runner = Endpoint::recover(
        Side::Runner,
        pair(41),
        MessagePolicy::default(),
        &checkpoint,
        watermark,
    )
    .unwrap();
    let request = ws.issue_receipt_reconciliation(1, 30_000, &clock).unwrap();
    let received = runner.receive(&request, &clock).unwrap();
    assert_eq!(received.info().task, original_info.info().task);
    assert_eq!(runner.receive(&request, &clock), Err(ProtocolError::Replay));
    let receipt = runner.issue_receipt(1, 30_000, &clock).unwrap();
    assert!(ws.receive(&receipt, &clock).unwrap().is_new());
    let another = ws.issue_receipt_reconciliation(1, 30_000, &clock).unwrap();
    runner.receive(&another, &clock).unwrap();
    let newer = runner.issue_receipt(1, 30_000, &clock).unwrap();
    assert!(matches!(
        ws.receive(&newer, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));
    assert!(runner.issue_receipt(2, 30_000, &clock).is_ok());
}

#[test]
fn stale_reconciliations_and_outcome_conflicts_fail_closed() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let original = issue(&mut ws, &clock);
    runner.receive(&original, &clock).unwrap();
    let receipt = runner.issue_receipt(1, 30_000, &clock).unwrap();
    ws.receive(&receipt, &clock).unwrap();
    let mut query = ws.issue_receipt_reconciliation(1, 10_000, &clock).unwrap();
    // Modifying a correctly signed query must fail authentication.
    let at = query.iter().position(|byte| *byte == b'j').unwrap();
    query[at] ^= 1;
    assert!(runner.receive(&query, &clock).is_err());
    assert_eq!(runner.last_incoming_sequence(), 1);
    // A new, honest query can still succeed after rejecting a forgery.
    let correct = ws.issue_receipt_reconciliation(1, 10_000, &clock).unwrap();
    assert!(runner.receive(&correct, &clock).unwrap().is_new());

    let outcome = sha(b"completed");
    let first = runner
        .issue_result("job-1", JobOutcome::Succeeded, &outcome, 30_000, &clock)
        .unwrap();
    assert!(ws.receive(&first, &clock).unwrap().is_new());
    let mut shadow = Endpoint::new(Side::Runner, pair(41), MessagePolicy::default()).unwrap();
    shadow.receive(&original, &clock).unwrap();
    // Advance shadow's outgoing sequence by sending a legal receipt, then
    // produce another *validly signed* result with a contradictory digest.
    shadow.issue_receipt(1, 30_000, &clock).unwrap();
    shadow.issue_receipt(1, 30_000, &clock).unwrap();
    let conflicting = shadow
        .issue_result(
            "job-1",
            JobOutcome::Failed,
            &sha(b"tampered-result"),
            30_000,
            &clock,
        )
        .unwrap();
    assert_eq!(
        ws.receive(&conflicting, &clock),
        Err(ProtocolError::Conflict)
    );
    assert_eq!(ws.last_incoming_sequence(), 2);
}

#[test]
fn pairing_requires_explicit_approval_and_nonzero_secret() {
    assert_eq!(
        Secret::try_from_slice(&[7; 31]).unwrap_err(),
        ProtocolError::Invalid
    );
    assert_eq!(
        Secret::from_bytes([0; 32]).unwrap_err(),
        ProtocolError::Invalid
    );
    assert!(format!("{:?}", secret(0xab)).contains("REDACTED"));
    let mut pending = Pair::enroll("workstation-1", "runner-1", secret(17)).unwrap();
    assert_eq!(pending.status(), PairStatus::Pending);
    assert!(matches!(
        Endpoint::new(Side::Runner, pending, MessagePolicy::default()),
        Err(ProtocolError::NotPaired)
    ));
    pending = Pair::enroll("workstation-1", "runner-1", secret(17)).unwrap();
    pending.approve().unwrap();
    assert_eq!(pending.approve(), Err(ProtocolError::NotPaired));
    assert_eq!(
        Pair::enroll("same", "same", secret(3)).err(),
        Some(ProtocolError::Invalid)
    );
    assert_eq!(
        Pair::enroll("path/escape", "runner-1", secret(3)).err(),
        Some(ProtocolError::Invalid)
    );
}

#[test]
fn job_cancel_result_and_all_receipt_classes_are_correlated_and_idempotent() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let job = issue(&mut ws, &clock);
    assert_eq!(ws.next_outgoing_sequence(), 2);
    let first = runner.receive(&job, &clock).unwrap();
    assert!(first.is_new());
    assert_eq!(first.info().agent_id, "agent-1");
    assert_eq!(first.info().permission, scope());
    assert_eq!(first.info().sender, Side::Workstation);

    let job_receipt = runner
        .issue_receipt(first.info().sequence, 20_000, &clock)
        .unwrap();
    assert!(ws.receive(&job_receipt, &clock).unwrap().is_new());
    assert!(matches!(
        ws.receive(&job_receipt, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));
    let duplicate_with_new_sequence = runner.issue_receipt(1, 20_000, &clock).unwrap();
    assert!(matches!(
        ws.receive(&duplicate_with_new_sequence, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));

    let cancel = ws.issue_cancel("job-1", 20_000, &clock).unwrap();
    let cancelled = runner.receive(&cancel, &clock).unwrap();
    assert!(cancelled.is_new());
    assert_eq!(runner.receive(&cancel, &clock), Err(ProtocolError::Replay));
    let cancel_receipt = runner
        .issue_receipt(cancelled.info().sequence, 20_000, &clock)
        .unwrap();
    assert!(ws.receive(&cancel_receipt, &clock).unwrap().is_new());
    assert!(matches!(
        ws.receive(&cancel_receipt, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));
    assert_eq!(
        ws.issue_cancel("job-1", 20_000, &clock),
        Err(ProtocolError::Conflict)
    );

    let digest = sha(b"cancelled");
    assert_eq!(
        runner.issue_result("job-1", JobOutcome::Succeeded, &digest, 20_000, &clock),
        Err(ProtocolError::Conflict)
    );
    let result = runner
        .issue_result("job-1", JobOutcome::Cancelled, &digest, 20_000, &clock)
        .unwrap();
    let received_result = ws.receive(&result, &clock).unwrap();
    assert!(received_result.is_new());
    assert_eq!(ws.receive(&result, &clock), Err(ProtocolError::Replay));
    let result_receipt = ws
        .issue_receipt(received_result.info().sequence, 20_000, &clock)
        .unwrap();
    assert!(runner.receive(&result_receipt, &clock).unwrap().is_new());
    assert!(matches!(
        runner.receive(&result_receipt, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));
    assert_eq!(
        runner.issue_result("job-1", JobOutcome::Cancelled, &digest, 20_000, &clock),
        Err(ProtocolError::Conflict)
    );
    assert_eq!(
        runner.issue_receipt(1_000, 20_000, &clock),
        Err(ProtocolError::Conflict)
    );
    assert_eq!(
        ws.issue_receipt(received_result.info().sequence + 100, 20_000, &clock),
        Err(ProtocolError::Conflict)
    );
}

#[test]
fn result_successful_without_execution_is_only_a_signed_claim() {
    let clock = FakeClock::new(500_000);
    let (mut ws, mut runner) = endpoints();
    runner.receive(&issue(&mut ws, &clock), &clock).unwrap();
    let digest = sha(b"the host must validate output bytes");
    let result = runner
        .issue_result("job-1", JobOutcome::Succeeded, &digest, 5_000, &clock)
        .unwrap();
    ws.receive(&result, &clock).unwrap();
    assert_eq!(
        ws.issue_cancel("job-1", 5_000, &clock),
        Err(ProtocolError::Conflict)
    );
    assert_eq!(
        runner.issue_result("job-1", JobOutcome::Failed, &digest, 5_000, &clock),
        Err(ProtocolError::Conflict)
    );
}

#[test]
fn cancellation_request_is_not_proof_of_cancelled_execution_and_can_race_result() {
    let clock = FakeClock::new(510_000);
    let (mut ws, mut runner) = endpoints();
    runner.receive(&issue(&mut ws, &clock), &clock).unwrap();
    // The user requested cancel, but the runner has not observed it yet.
    let cancel_in_transit = ws.issue_cancel("job-1", 20_000, &clock).unwrap();
    // A completed result can arrive first. A protocol must not misrepresent
    // cancellation as an already-acknowledged process shutdown.
    let digest = sha(b"finished before cancel delivery");
    let success = runner
        .issue_result("job-1", JobOutcome::Succeeded, &digest, 20_000, &clock)
        .unwrap();
    assert!(ws.receive(&success, &clock).unwrap().is_new());
    let checkpoint = ws.checkpoint().unwrap();
    assert!(Endpoint::recover(
        Side::Workstation,
        pair(41),
        MessagePolicy::default(),
        &checkpoint,
        ws.checkpoint_generation()
    )
    .is_ok());
    assert_eq!(
        runner.receive(&cancel_in_transit, &clock),
        Err(ProtocolError::Conflict)
    );
}

#[test]
fn forged_tag_and_swapped_ids_or_roles_are_rejected_without_advancing_replay() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let original = issue(&mut ws, &clock);
    let encoded = String::from_utf8(original.clone()).unwrap();

    for tampered in [
        encoded.replace("workstation-1", "workstation-x"),
        encoded.replace("runner-1", "runner-other"),
        encoded.replace("\"workstation\"", "\"runner\""),
        encoded.replace("agent-1", "agent-2"),
        encoded.replace("source-1", "source-2"),
        encoded.replace("Summarize", "Corrupt___"),
    ] {
        assert_eq!(
            runner.receive(tampered.as_bytes(), &clock),
            Err(ProtocolError::Authentication)
        );
        assert_eq!(runner.last_incoming_sequence(), 0);
    }
    assert!(runner.receive(&original, &clock).unwrap().is_new());
    assert_eq!(
        runner.receive(&original, &clock),
        Err(ProtocolError::Replay)
    );
}

#[test]
fn different_pair_secret_even_with_matching_ids_fails_authentication() {
    let clock = FakeClock::new(800_000);
    let (mut ws, _) = endpoints();
    let mut wrong_runner = Endpoint::new(Side::Runner, pair(42), MessagePolicy::default()).unwrap();
    assert_eq!(
        wrong_runner.receive(&issue(&mut ws, &clock), &clock),
        Err(ProtocolError::Authentication)
    );
}

#[test]
fn valid_hmac_with_another_workstation_identity_is_still_denied() {
    let clock = FakeClock::new(800_000);
    let mut foreign_pair = Pair::enroll("workstation-foreign", "runner-1", secret(41)).unwrap();
    foreign_pair.approve().unwrap();
    let mut foreign_ws =
        Endpoint::new(Side::Workstation, foreign_pair, MessagePolicy::default()).unwrap();
    let (_, mut runner) = endpoints();
    let message = foreign_ws
        .issue_job(
            "job-1",
            "agent-1",
            scope(),
            "cross-pair request",
            20_000,
            &clock,
        )
        .unwrap();
    // Same shared bytes; the MAC is valid, but its signed workstation ID
    // is not the locally approved workstation.
    assert_eq!(
        runner.receive(&message, &clock),
        Err(ProtocolError::Authentication)
    );
    assert_eq!(runner.last_incoming_sequence(), 0);
}

#[test]
fn untrusted_input_and_token_strings_never_appear_in_protocol_errors() {
    let clock = FakeClock::new(800_000);
    let (_, mut runner) = endpoints();
    let attacker_input = br#"{"secret":"Bearer-super-secret-token","instruction":"steal files"}"#;
    let error = runner.receive(attacker_input, &clock).unwrap_err();
    assert_eq!(error, ProtocolError::Invalid);
    let display = format!("{error}");
    let debug = format!("{error:?}");
    for forbidden in ["Bearer-super-secret-token", "steal files", "secret"] {
        assert!(!display.contains(forbidden));
        assert!(!debug.contains(forbidden));
    }
}

#[test]
fn wire_is_canonical_and_rejects_unknown_unsigned_payload_fields() {
    let clock = FakeClock::new(800_000);
    let (mut ws, mut runner) = endpoints();
    let original = String::from_utf8(issue(&mut ws, &clock)).unwrap();
    let padded = format!(" {original}");
    assert_eq!(
        runner.receive(padded.as_bytes(), &clock),
        Err(ProtocolError::Invalid)
    );
    let unknown_field = original.replace(
        "\"instruction\":",
        "\"untrusted_extension\":\"anything\",\"instruction\":",
    );
    assert_ne!(unknown_field, original);
    assert_eq!(
        runner.receive(unknown_field.as_bytes(), &clock),
        Err(ProtocolError::Invalid)
    );
    let invalid_tag = original.replace("\"tag\":\"", "\"tag\":\"NOT_HEX_");
    assert_eq!(
        runner.receive(invalid_tag.as_bytes(), &clock),
        Err(ProtocolError::Authentication)
    );
    assert!(runner
        .receive(original.as_bytes(), &clock)
        .unwrap()
        .is_new());
}

#[test]
fn timestamp_skew_future_issued_and_expired_messages_fail_closed() {
    let sender_clock = FakeClock::new(1_000_000);
    let receiver_clock = FakeClock::new(969_999);
    let (mut ws, mut runner) = endpoints();
    let signed = issue(&mut ws, &sender_clock);
    assert_eq!(
        runner.receive(&signed, &receiver_clock),
        Err(ProtocolError::TimeWindow)
    );
    receiver_clock.set(970_000);
    assert!(runner.receive(&signed, &receiver_clock).unwrap().is_new());

    let (mut ws2, mut runner2) = endpoints();
    let signed2 = issue(&mut ws2, &sender_clock);
    receiver_clock.set(1_030_000);
    assert_eq!(
        runner2.receive(&signed2, &receiver_clock),
        Err(ProtocolError::TimeWindow)
    );
    receiver_clock.set(1_029_999);
    assert!(runner2.receive(&signed2, &receiver_clock).unwrap().is_new());
}

#[test]
fn expired_idempotent_receipts_fail_closed_even_when_previously_seen() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    runner.receive(&issue(&mut ws, &clock), &clock).unwrap();
    let receipt = runner.issue_receipt(1, 100, &clock).unwrap();
    assert!(ws.receive(&receipt, &clock).unwrap().is_new());
    clock.set(1_000_099);
    assert!(matches!(
        ws.receive(&receipt, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));
    clock.set(1_000_100);
    assert_eq!(ws.receive(&receipt, &clock), Err(ProtocolError::TimeWindow));
}

#[test]
fn invalid_clock_policy_and_overflowed_expiry_are_rejected() {
    assert!(matches!(
        Endpoint::new(
            Side::Runner,
            pair(41),
            MessagePolicy {
                max_ttl_ms: 0,
                max_future_skew_ms: 0
            }
        ),
        Err(ProtocolError::Invalid)
    ));
    let clock = FakeClock::new(i64::MAX - 1);
    let (mut ws, _) = endpoints();
    assert_eq!(
        ws.issue_job("job-1", "agent-1", scope(), "fixture", 10, &clock),
        Err(ProtocolError::TimeWindow)
    );
    assert_eq!(ws.next_outgoing_sequence(), 1);
}

#[test]
fn rotating_invalidates_in_flight_messages_and_old_key_epochs() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let old = issue(&mut ws, &clock);
    assert_eq!(ws.rotate_key(secret(50)).unwrap(), 2);
    assert_eq!(runner.rotate_key(secret(50)).unwrap(), 2);
    assert_eq!(ws.next_outgoing_sequence(), 2);
    assert!(matches!(
        runner.receive(&old, &clock),
        Err(ProtocolError::Authentication | ProtocolError::Epoch)
    ));
    assert_eq!(
        ws.issue_cancel("job-1", 20_000, &clock),
        Err(ProtocolError::Conflict)
    );
    let new_job = ws
        .issue_job("job-1", "agent-1", scope(), "New epoch", 20_000, &clock)
        .unwrap();
    assert!(runner.receive(&new_job, &clock).unwrap().is_new());
    assert_eq!(runner.key_epoch(), 2);
}

#[test]
fn revocation_disables_issuance_and_verification_and_never_leaks_secret() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let job = issue(&mut ws, &clock);
    runner.revoke();
    assert_eq!(runner.pair_status(), PairStatus::Revoked);
    assert_eq!(runner.receive(&job, &clock), Err(ProtocolError::NotPaired));
    assert_eq!(runner.rotate_key(secret(99)), Err(ProtocolError::NotPaired));
    ws.revoke();
    assert_eq!(
        ws.issue_job("job-2", "agent-1", scope(), "test", 1000, &clock),
        Err(ProtocolError::NotPaired)
    );
    let failure = format!("{}", runner.receive(&job, &clock).unwrap_err());
    assert!(!failure.contains("workstation-1"));
    assert!(!failure.contains("runner-1"));
    assert!(!failure.contains("agent-1"));
    assert!(!failure.contains("41"));
}

#[test]
fn payload_limits_permissions_and_expiry_policy_are_enforced() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    assert_eq!(
        ws.issue_job(
            "job-1",
            "agent-1",
            scope(),
            &"Z".repeat(MAX_INSTRUCTION_BYTES + 1),
            1000,
            &clock
        ),
        Err(ProtocolError::Invalid)
    );
    assert_eq!(
        ws.issue_job("job-1", "agent-1", scope(), "do work", 300_001, &clock),
        Err(ProtocolError::TimeWindow)
    );
    assert_eq!(
        ws.issue_job(
            "job-1",
            "agent-1",
            PermissionScope {
                capability: Capability::DraftText,
                resource_id: "../escaped".into()
            },
            "do work",
            1000,
            &clock
        ),
        Err(ProtocolError::Invalid)
    );
    assert_eq!(
        ws.issue_job("job-1", "agent-1", scope(), "with\u{0000}nul", 1000, &clock),
        Err(ProtocolError::Invalid)
    );
    let oversized_wire = vec![b' '; MAX_WIRE_BYTES + 1];
    assert_eq!(
        runner.receive(&oversized_wire, &clock),
        Err(ProtocolError::Oversized)
    );
    assert_eq!(ws.next_outgoing_sequence(), 1);
    let job = issue(&mut ws, &clock);
    assert!(runner.receive(&job, &clock).unwrap().is_new());
}

#[test]
fn replayed_old_job_after_a_new_job_is_denied() {
    let clock = FakeClock::new(600_000);
    let (mut ws, mut runner) = endpoints();
    let first = issue(&mut ws, &clock);
    let second = ws
        .issue_job("job-2", "agent-1", scope(), "Other work", 20_000, &clock)
        .unwrap();
    assert!(runner.receive(&second, &clock).unwrap().is_new());
    assert_eq!(runner.receive(&first, &clock), Err(ProtocolError::Replay));
    assert_eq!(runner.last_incoming_sequence(), 2);
}

#[test]
fn bounded_tracking_fails_closed_without_evicting_replay_history() {
    let clock = FakeClock::new(600_000);
    let (mut ws, mut runner) = endpoints();
    let mut earliest = Vec::new();
    for index in 0..MAX_TRACKED {
        let wire = ws
            .issue_job(
                &format!("job-{index}"),
                "agent-1",
                scope(),
                "bounded fixture",
                20_000,
                &clock,
            )
            .unwrap();
        if index == 0 {
            earliest = wire.clone();
        }
        assert!(runner.receive(&wire, &clock).unwrap().is_new());
    }
    assert_eq!(
        ws.issue_job("overflow", "agent-1", scope(), "extra", 20_000, &clock),
        Err(ProtocolError::Capacity)
    );
    assert_eq!(ws.next_outgoing_sequence(), 1025);
    assert_eq!(
        runner.receive(&earliest, &clock),
        Err(ProtocolError::Replay)
    );
    let full_snapshot = runner.checkpoint().unwrap();
    assert!(full_snapshot.len() <= constellation_runner_protocol::MAX_CHECKPOINT_BYTES);
    let mut recovered = Endpoint::recover(
        Side::Runner,
        pair(41),
        MessagePolicy::default(),
        &full_snapshot,
        runner.checkpoint_generation(),
    )
    .unwrap();
    assert_eq!(
        recovered.receive(&earliest, &clock),
        Err(ProtocolError::Replay)
    );
}

#[test]
fn trusted_authorization_denies_before_mutation_then_exposes_verified_instruction() {
    let clock = FakeClock::new(1_000_000);
    let (mut ws, mut runner) = endpoints();
    let job = issue(&mut ws, &clock);
    let checks = Cell::new(0);
    assert_eq!(
        runner.receive_authorized(&job, &clock, |info, message| {
            checks.set(checks.get() + 1);
            assert_eq!(info.agent_id, "agent-1");
            assert_eq!(info.permission, scope());
            assert!(matches!(message, Message::Job { .. }));
            false
        }),
        Err(ProtocolError::Unauthorized)
    );
    assert_eq!(checks.get(), 1);
    assert_eq!(runner.last_incoming_sequence(), 0);
    assert_eq!(runner.checkpoint_generation(), 0);
    let (accepted, message) = runner
        .receive_authorized(&job, &clock, |info, _| {
            info.permission.capability == Capability::ReadEvidence
                && info.permission.resource_id == "source-1"
        })
        .unwrap();
    assert!(accepted.is_new());
    assert!(matches!(message, Message::Job { instruction } if instruction.contains("Summarize")));
    assert_eq!(runner.checkpoint_generation(), 1);
    assert_eq!(runner.receive(&job, &clock), Err(ProtocolError::Replay));

    let mut forged = job.clone();
    *forged.last_mut().unwrap() = b'!';
    let (mut _unused, mut runner2) = endpoints();
    let callback_ran = Cell::new(false);
    assert!(runner2
        .receive_authorized(&forged, &clock, |_, _| {
            callback_ran.set(true);
            true
        })
        .is_err());
    assert!(!callback_ran.get());
}

#[test]
fn recovered_checkpoints_preserve_jobs_cancellation_and_receipt_idempotency() {
    let clock = FakeClock::new(2_000_000);
    let (mut ws, mut runner) = endpoints();
    let job = issue(&mut ws, &clock);
    runner.receive(&job, &clock).unwrap();

    let ws_bytes = ws.checkpoint().unwrap();
    let runner_bytes = runner.checkpoint().unwrap();
    assert_eq!(ws.checkpoint_generation(), 1);
    assert_eq!(runner.checkpoint_generation(), 1);
    let mut ws = Endpoint::recover(
        Side::Workstation,
        pair(41),
        MessagePolicy::default(),
        &ws_bytes,
        1,
    )
    .unwrap();
    let mut runner = Endpoint::recover(
        Side::Runner,
        pair(41),
        MessagePolicy::default(),
        &runner_bytes,
        1,
    )
    .unwrap();
    assert_eq!(runner.receive(&job, &clock), Err(ProtocolError::Replay));
    assert_eq!(runner.last_incoming_sequence(), 1);

    let receipt = runner.issue_receipt(1, 10_000, &clock).unwrap();
    ws.receive(&receipt, &clock).unwrap();
    let ws_receipt_bytes = ws.checkpoint().unwrap();
    let mut ws = Endpoint::recover(
        Side::Workstation,
        pair(41),
        MessagePolicy::default(),
        &ws_receipt_bytes,
        ws.checkpoint_generation(),
    )
    .unwrap();
    assert!(matches!(
        ws.receive(&receipt, &clock),
        Ok(ReceiveOutcome::DuplicateReceipt(_))
    ));
    let cancel = ws.issue_cancel("job-1", 10_000, &clock).unwrap();
    runner.receive(&cancel, &clock).unwrap();
    let new_snapshot = runner.checkpoint().unwrap();
    let mut runner = Endpoint::recover(
        Side::Runner,
        pair(41),
        MessagePolicy::default(),
        &new_snapshot,
        runner.checkpoint_generation(),
    )
    .unwrap();
    assert_eq!(runner.receive(&cancel, &clock), Err(ProtocolError::Replay));
    assert_eq!(
        runner.issue_result(
            "job-1",
            JobOutcome::Succeeded,
            &sha(b"illegal"),
            10_000,
            &clock
        ),
        Err(ProtocolError::Conflict)
    );
    let result = runner
        .issue_result(
            "job-1",
            JobOutcome::Cancelled,
            &sha(b"stopped"),
            10_000,
            &clock,
        )
        .unwrap();
    ws.receive(&result, &clock).unwrap();
    assert_eq!(
        ws.issue_cancel("job-1", 10_000, &clock),
        Err(ProtocolError::Conflict)
    );
}

#[test]
fn checkpoint_rejects_tampering_rollback_swapped_identity_and_wrong_key() {
    let clock = FakeClock::new(3_000_000);
    let (mut ws, _) = endpoints();
    let initial = ws.checkpoint().unwrap();
    issue(&mut ws, &clock);
    let latest = ws.checkpoint().unwrap();
    assert_eq!(ws.checkpoint_generation(), 1);
    assert!(matches!(
        Endpoint::recover(
            Side::Workstation,
            pair(41),
            MessagePolicy::default(),
            &initial,
            1
        ),
        Err(ProtocolError::Checkpoint)
    ));
    let mut forged = latest.clone();
    let index = forged.iter().position(|b| *b == b'4').unwrap();
    forged[index] = b'5';
    assert!(matches!(
        Endpoint::recover(
            Side::Workstation,
            pair(41),
            MessagePolicy::default(),
            &forged,
            1
        ),
        Err(ProtocolError::Checkpoint)
    ));
    assert!(matches!(
        Endpoint::recover(Side::Runner, pair(41), MessagePolicy::default(), &latest, 1),
        Err(ProtocolError::Checkpoint)
    ));
    assert!(matches!(
        Endpoint::recover(
            Side::Workstation,
            pair(42),
            MessagePolicy::default(),
            &latest,
            1
        ),
        Err(ProtocolError::Checkpoint)
    ));
    assert!(matches!(
        Endpoint::recover(
            Side::Workstation,
            pair(41),
            MessagePolicy::default(),
            &latest,
            2
        ),
        Err(ProtocolError::Checkpoint)
    ));
    let mut restored = Endpoint::recover(
        Side::Workstation,
        pair(41),
        MessagePolicy::default(),
        &latest,
        1,
    )
    .unwrap();
    assert_eq!(restored.rotate_key(secret(41)), Err(ProtocolError::Invalid));
    assert_eq!(restored.key_epoch(), 1);
    assert_eq!(restored.checkpoint_generation(), 1);
    assert_eq!(restored.rotate_key(secret(43)), Ok(2));
    let rotated = restored.checkpoint().unwrap();
    assert!(matches!(
        Endpoint::recover(
            Side::Workstation,
            pair(41),
            MessagePolicy::default(),
            &rotated,
            2
        ),
        Err(ProtocolError::Checkpoint)
    ));
    let mut approved = Pair::enroll("workstation-1", "runner-1", secret(43)).unwrap();
    approved.approve().unwrap();
    // A pair with the new key but an old epoch must still fail closed.
    assert!(matches!(
        Endpoint::recover(
            Side::Workstation,
            approved,
            MessagePolicy::default(),
            &rotated,
            2
        ),
        Err(ProtocolError::Checkpoint)
    ));
    let trusted_pair = Pair::resume_approved("workstation-1", "runner-1", secret(43), 2).unwrap();
    let recovered = Endpoint::recover(
        Side::Workstation,
        trusted_pair,
        MessagePolicy::default(),
        &rotated,
        2,
    )
    .unwrap();
    assert_eq!(recovered.key_epoch(), 2);
    assert_eq!(recovered.checkpoint_generation(), 2);
}
