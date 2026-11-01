//! Offline, effect-free authentication and lifecycle primitives for a
//! future user-owned runner. This module never performs I/O or tool execution.
#![forbid(unsafe_code)]

mod framing;
pub use framing::{read_frame, write_frame};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

type HmacSha256 = Hmac<Sha256>;

pub const VERSION: u16 = 1;
pub const MAX_WIRE_BYTES: usize = 16 * 1024;
pub const MAX_INSTRUCTION_BYTES: usize = 4096;
pub const MAX_TRACKED: usize = 1024;
pub const MAX_CHECKPOINT_BYTES: usize = 2 * 1024 * 1024;
const CHECKPOINT_DOMAIN: &[u8] = b"constellation-runner-checkpoint-v1\0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid runner protocol field or message")]
    Invalid,
    #[error("message exceeds a protocol size limit")]
    Oversized,
    #[error("authentication or identity verification failed")]
    Authentication,
    #[error("pair is not approved or has been revoked")]
    NotPaired,
    #[error("key epoch does not match the approved pair")]
    Epoch,
    #[error("message is outside its allowed time window")]
    TimeWindow,
    #[error("message sequence has already been used")]
    Replay,
    #[error("message violates protocol ordering or correlation")]
    Conflict,
    #[error("protocol tracking capacity reached; re-pair with a new key")]
    Capacity,
    #[error("invalid role for protocol operation")]
    Role,
    #[error("trusted local authorization denied this job")]
    Unauthorized,
    #[error("message sequence counter exhausted")]
    SequenceExhausted,
    #[error("checkpoint is invalid, untrusted, or rolled back")]
    Checkpoint,
}

pub type Result<T> = std::result::Result<T, ProtocolError>;

/// Supplied by the host, never read implicitly. A trusted monotonic/wall-clock
/// source is necessary for real expiry policy.
pub trait Clock {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagePolicy {
    pub max_ttl_ms: i64,
    pub max_future_skew_ms: i64,
}

impl Default for MessagePolicy {
    fn default() -> Self {
        Self {
            max_ttl_ms: 300_000,
            max_future_skew_ms: 30_000,
        }
    }
}

impl MessagePolicy {
    fn validate(self) -> Result<()> {
        if !(1..=3_600_000).contains(&self.max_ttl_ms)
            || !(0..=300_000).contains(&self.max_future_skew_ms)
        {
            return Err(ProtocolError::Invalid);
        }
        Ok(())
    }
}

/// A 32-byte high-entropy shared key. It is deliberately neither Clone nor
/// Serialize; Debug always redacts it. Provision using a CSPRNG outside crate.
pub struct Secret(Zeroizing<[u8; 32]>);

impl Secret {
    pub fn from_bytes(bytes: [u8; 32]) -> Result<Self> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Err(ProtocolError::Invalid);
        }
        Ok(Self(Zeroizing::new(bytes)))
    }

    pub fn try_from_slice(bytes: &[u8]) -> Result<Self> {
        let data: [u8; 32] = bytes.try_into().map_err(|_| ProtocolError::Invalid)?;
        Self::from_bytes(data)
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Workstation,
    Runner,
}

impl Side {
    fn peer(self) -> Self {
        match self {
            Self::Workstation => Self::Runner,
            Self::Runner => Self::Workstation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStatus {
    Pending,
    Approved,
    Revoked,
}

pub struct Pair {
    workstation_id: String,
    runner_id: String,
    secret: Secret,
    epoch: u64,
    status: PairStatus,
}

impl Pair {
    /// Local enrollment only: does not authenticate or contact another device.
    pub fn enroll(workstation_id: &str, runner_id: &str, secret: Secret) -> Result<Self> {
        if !valid_id(workstation_id) || !valid_id(runner_id) || workstation_id == runner_id {
            return Err(ProtocolError::Invalid);
        }
        Ok(Self {
            workstation_id: workstation_id.into(),
            runner_id: runner_id.into(),
            secret,
            epoch: 1,
            status: PairStatus::Pending,
        })
    }

    /// Trusted local user approval is required before any message is issued.
    pub fn approve(&mut self) -> Result<()> {
        if self.status != PairStatus::Pending {
            return Err(ProtocolError::NotPaired);
        }
        self.status = PairStatus::Approved;
        Ok(())
    }

    /// Reconstruct an approved pair ONLY from a trusted, rollback-protected
    /// local key store that already records user approval, key and epoch.
    /// Never use data supplied by a peer as proof of approval or key epoch.
    pub fn resume_approved(
        workstation_id: &str,
        runner_id: &str,
        secret: Secret,
        epoch: u64,
    ) -> Result<Self> {
        if epoch == 0 {
            return Err(ProtocolError::Epoch);
        }
        let mut pair = Self::enroll(workstation_id, runner_id, secret)?;
        pair.epoch = epoch;
        pair.status = PairStatus::Approved;
        Ok(pair)
    }

    pub fn status(&self) -> PairStatus {
        self.status
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ReadEvidence,
    DraftText,
    AnalyzeData,
}

/// An attribution and request for permission, NOT a grant. No paths, shell,
/// unrestricted OS capabilities or wildcard resources are encoded here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionScope {
    pub capability: Capability,
    pub resource_id: String,
}

/// Explicit owner and run/task attribution inside the signed envelope.
/// This is an immutable assertion by the paired workstation, never an OS grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAttribution {
    pub owner_id: String,
    pub run_id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundPermission {
    pub scope: PermissionScope,
    pub task: TaskAttribution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum Message {
    Job {
        instruction: String,
    },
    Cancel {
        for_job_sequence: u64,
    },
    Result {
        for_job_sequence: u64,
        outcome: JobOutcome,
        output_sha256: String,
    },
    /// SHA-256 of the *entire* canonical signed message being acknowledged.
    Receipt {
        for_sequence: u64,
        message_sha256: String,
    },
    /// A fresh signed request for a new receipt of an already accepted peer
    /// message. The original can have expired; the request itself cannot.
    /// The peer verifies the original digest and all attribution in its
    /// durable replay ledger without re-applying the original message.
    ReconcileReceipt {
        for_sequence: u64,
        message_sha256: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u16,
    workstation_id: String,
    runner_id: String,
    sender: Side,
    key_epoch: u64,
    sequence: u64,
    issued_at_ms: i64,
    expires_at_ms: i64,
    job_id: String,
    agent_id: String,
    permission: PermissionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<TaskAttribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthFields {
    header: Header,
    message: Message,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedWire {
    header: Header,
    message: Message,
    tag: String,
}

/// Public receipt metadata; it intentionally excludes instructions and keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageInfo {
    pub job_id: String,
    pub agent_id: String,
    pub permission: PermissionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskAttribution>,
    pub sender: Side,
    pub sequence: u64,
    pub digest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveOutcome {
    Applied(MessageInfo),
    DuplicateReceipt(MessageInfo),
    /// Another authenticated commitment to a previously received outcome.
    /// The job state is unchanged; only replay bookkeeping advances.
    DuplicateResult(MessageInfo),
}

impl ReceiveOutcome {
    pub fn is_new(&self) -> bool {
        matches!(self, Self::Applied(_))
    }

    pub fn info(&self) -> &MessageInfo {
        match self {
            Self::Applied(info) | Self::DuplicateReceipt(info) | Self::DuplicateResult(info) => {
                info
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRecord {
    sequence: u64,
    agent_id: String,
    permission: PermissionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<TaskAttribution>,
    cancelled: bool,
    result: Option<JobOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_sha256: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexedMessage {
    info: MessageInfo,
    is_receipt: bool,
}

/// Secret-free endpoint state; the host's rollback watermark is stored separately.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointFields {
    version: u16,
    side: Side,
    workstation_id: String,
    runner_id: String,
    key_epoch: u64,
    policy: MessagePolicy,
    generation: u64,
    next_outgoing: u64,
    last_incoming: u64,
    issued: BTreeMap<u64, MessageInfo>,
    received: BTreeMap<u64, IndexedMessage>,
    acknowledged: BTreeSet<u64>,
    jobs: BTreeMap<String, JobRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedCheckpoint {
    fields: CheckpointFields,
    tag: String,
}

/// Owns strictly bounded in-memory anti-replay and job/receipt ledgers.
/// A future host MUST persist state with trusted rollback protection between
/// restarts; reusing the same key after constructing a fresh Endpoint is unsafe.
pub struct Endpoint {
    side: Side,
    pair: Pair,
    policy: MessagePolicy,
    generation: u64,
    next_outgoing: u64,
    last_incoming: u64,
    issued: BTreeMap<u64, MessageInfo>,
    received: BTreeMap<u64, IndexedMessage>,
    acknowledged: BTreeSet<u64>,
    jobs: BTreeMap<String, JobRecord>,
}

impl Endpoint {
    pub fn new(side: Side, pair: Pair, policy: MessagePolicy) -> Result<Self> {
        policy.validate()?;
        if pair.status != PairStatus::Approved {
            return Err(ProtocolError::NotPaired);
        }
        Ok(Self {
            side,
            pair,
            policy,
            generation: 0,
            next_outgoing: 1,
            last_incoming: 0,
            issued: BTreeMap::new(),
            received: BTreeMap::new(),
            acknowledged: BTreeSet::new(),
            jobs: BTreeMap::new(),
        })
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn pair_status(&self) -> PairStatus {
        self.pair.status
    }

    pub fn key_epoch(&self) -> u64 {
        self.pair.epoch
    }

    pub fn next_outgoing_sequence(&self) -> u64 {
        self.next_outgoing
    }

    pub fn last_incoming_sequence(&self) -> u64 {
        self.last_incoming
    }

    /// Increments on each accepted new message, issued message or key rotation.
    /// A host must pin this high-water mark outside the checkpoint's storage.
    pub fn checkpoint_generation(&self) -> u64 {
        self.generation
    }

    /// Export authenticated replay, job and receipt state. This checkpoint is
    /// integrity protected but not encrypted; store it privately. It excludes
    /// instructions and secrets, but includes job IDs and permission scopes.
    pub fn checkpoint(&self) -> Result<Vec<u8>> {
        self.ensure_approved()?;
        let fields = CheckpointFields {
            version: VERSION,
            side: self.side,
            workstation_id: self.pair.workstation_id.clone(),
            runner_id: self.pair.runner_id.clone(),
            key_epoch: self.pair.epoch,
            policy: self.policy,
            generation: self.generation,
            next_outgoing: self.next_outgoing,
            last_incoming: self.last_incoming,
            issued: self.issued.clone(),
            received: self.received.clone(),
            acknowledged: self.acknowledged.clone(),
            jobs: self.jobs.clone(),
        };
        let canonical = serde_json::to_vec(&fields).map_err(|_| ProtocolError::Checkpoint)?;
        let tag = checkpoint_tag(&self.pair.secret, &canonical)?;
        let signed = SignedCheckpoint {
            fields,
            tag: encode_hex(&tag),
        };
        let wire = serde_json::to_vec(&signed).map_err(|_| ProtocolError::Checkpoint)?;
        if wire.len() > MAX_CHECKPOINT_BYTES {
            return Err(ProtocolError::Oversized);
        }
        Ok(wire)
    }

    /// Recover a pinned, authenticated checkpoint with the original approved
    /// key. `minimum_generation` MUST come from independent rollback-resistant
    /// host state (not the supplied checkpoint). The host also pins key epoch,
    /// pair identity and approval/revocation in its trusted key store.
    pub fn recover(
        side: Side,
        pair: Pair,
        policy: MessagePolicy,
        wire: &[u8],
        minimum_generation: u64,
    ) -> Result<Self> {
        policy.validate()?;
        if pair.status != PairStatus::Approved {
            return Err(ProtocolError::NotPaired);
        }
        if wire.len() > MAX_CHECKPOINT_BYTES {
            return Err(ProtocolError::Oversized);
        }
        let signed: SignedCheckpoint =
            serde_json::from_slice(wire).map_err(|_| ProtocolError::Checkpoint)?;
        if serde_json::to_vec(&signed).map_err(|_| ProtocolError::Checkpoint)? != wire {
            return Err(ProtocolError::Checkpoint);
        }
        let fields = signed.fields;
        let canonical = serde_json::to_vec(&fields).map_err(|_| ProtocolError::Checkpoint)?;
        let tag = decode_hex(&signed.tag).ok_or(ProtocolError::Checkpoint)?;
        let mut mac = HmacSha256::new_from_slice(&pair.secret.0[..])
            .map_err(|_| ProtocolError::Checkpoint)?;
        mac.update(CHECKPOINT_DOMAIN);
        mac.update(&canonical);
        mac.verify_slice(&tag)
            .map_err(|_| ProtocolError::Checkpoint)?;
        if fields.version != VERSION
            || fields.side != side
            || fields.workstation_id != pair.workstation_id
            || fields.runner_id != pair.runner_id
            || fields.key_epoch != pair.epoch
            || fields.policy != policy
            || fields.generation < minimum_generation
            || !valid_checkpoint_fields(&fields)
        {
            return Err(ProtocolError::Checkpoint);
        }
        Ok(Self {
            side,
            pair,
            policy,
            generation: fields.generation,
            next_outgoing: fields.next_outgoing,
            last_incoming: fields.last_incoming,
            issued: fields.issued,
            received: fields.received,
            acknowledged: fields.acknowledged,
            jobs: fields.jobs,
        })
    }

    /// Trusted local/out-of-band rotation. Old epoch messages/unfinished jobs
    /// are invalidated. Counters continue increasing.
    pub fn rotate_key(&mut self, new_secret: Secret) -> Result<u64> {
        self.ensure_approved()?;
        let epoch = self.pair.epoch.checked_add(1).ok_or(ProtocolError::Epoch)?;
        let generation = self.next_generation()?;
        // Reject a no-op rotation without revealing either key.
        let expected = checkpoint_tag(&self.pair.secret, b"key-equality")?;
        let mut new_mac = HmacSha256::new_from_slice(&new_secret.0[..])
            .map_err(|_| ProtocolError::Authentication)?;
        new_mac.update(CHECKPOINT_DOMAIN);
        new_mac.update(b"key-equality");
        if new_mac.verify_slice(&expected).is_ok() {
            return Err(ProtocolError::Invalid);
        }
        self.pair.secret = new_secret;
        self.pair.epoch = epoch;
        self.generation = generation;
        self.issued.clear();
        self.received.clear();
        self.acknowledged.clear();
        self.jobs.clear();
        Ok(epoch)
    }

    pub fn revoke(&mut self) {
        self.pair.status = PairStatus::Revoked;
        self.pair.secret.0.zeroize();
        self.issued.clear();
        self.received.clear();
        self.acknowledged.clear();
        self.jobs.clear();
    }

    pub fn issue_job(
        &mut self,
        job_id: &str,
        agent_id: &str,
        permission: PermissionScope,
        instruction: &str,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.issue_job_with_task(
            job_id,
            agent_id,
            (permission, None),
            instruction,
            ttl_ms,
            clock,
        )
    }

    /// Opt-in signed binding of the owner, run, task, job, agent and exact
    /// permission scope. Every later Cancel/Result/Receipt carries the same
    /// attribution, and recovery checks it against the original job.
    pub fn issue_bound_job(
        &mut self,
        job_id: &str,
        agent_id: &str,
        bound: BoundPermission,
        instruction: &str,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.issue_job_with_task(
            job_id,
            agent_id,
            (bound.scope, Some(bound.task)),
            instruction,
            ttl_ms,
            clock,
        )
    }

    fn issue_job_with_task(
        &mut self,
        job_id: &str,
        agent_id: &str,
        requested: (PermissionScope, Option<TaskAttribution>),
        instruction: &str,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        let (permission, task) = requested;
        self.require_role(Side::Workstation)?;
        if self.jobs.contains_key(job_id) {
            return Err(ProtocolError::Conflict);
        }
        self.ensure_job_capacity()?;
        let message = Message::Job {
            instruction: instruction.into(),
        };
        let wire = self.issue(
            job_id,
            agent_id,
            &permission,
            (task.clone(), message),
            ttl_ms,
            clock,
        )?;
        self.jobs.insert(
            job_id.into(),
            JobRecord {
                sequence: self.next_outgoing - 1,
                agent_id: agent_id.into(),
                permission,
                task,
                cancelled: false,
                result: None,
                result_sha256: None,
            },
        );
        Ok(wire)
    }

    pub fn issue_cancel(
        &mut self,
        job_id: &str,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.require_role(Side::Workstation)?;
        let job = self.jobs.get(job_id).ok_or(ProtocolError::Conflict)?;
        if job.cancelled || job.result.is_some() {
            return Err(ProtocolError::Conflict);
        }
        let message = Message::Cancel {
            for_job_sequence: job.sequence,
        };
        let (agent, scope, task) = (
            job.agent_id.clone(),
            job.permission.clone(),
            job.task.clone(),
        );
        let wire = self.issue(job_id, &agent, &scope, (task, message), ttl_ms, clock)?;
        self.jobs
            .get_mut(job_id)
            .ok_or(ProtocolError::Conflict)?
            .cancelled = true;
        Ok(wire)
    }

    /// Authenticates an outcome digest only; the library never runs the job
    /// or reads its output. Callers must hash actual result bytes themselves.
    pub fn issue_result(
        &mut self,
        job_id: &str,
        outcome: JobOutcome,
        output_sha256: &str,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.require_role(Side::Runner)?;
        let job = self.jobs.get(job_id).ok_or(ProtocolError::Conflict)?;
        if job.result.is_some() || (job.cancelled && outcome != JobOutcome::Cancelled) {
            return Err(ProtocolError::Conflict);
        }
        if !valid_digest(output_sha256) {
            return Err(ProtocolError::Invalid);
        }
        let message = Message::Result {
            for_job_sequence: job.sequence,
            outcome,
            output_sha256: output_sha256.into(),
        };
        let (agent, scope, task) = (
            job.agent_id.clone(),
            job.permission.clone(),
            job.task.clone(),
        );
        let wire = self.issue(job_id, &agent, &scope, (task, message), ttl_ms, clock)?;
        self.jobs
            .get_mut(job_id)
            .ok_or(ProtocolError::Conflict)?
            .result = Some(outcome);
        self.jobs
            .get_mut(job_id)
            .ok_or(ProtocolError::Conflict)?
            .result_sha256 = Some(output_sha256.into());
        Ok(wire)
    }

    /// Reissue a fresh signed Result from the same checkpointed outcome and
    /// output digest after uncertain delivery. Does not run/retry any effect.
    /// Before calling, persist the initial `issue_result` checkpoint and the
    /// authenticated original result/output commitment together.
    pub fn reissue_result(
        &mut self,
        job_id: &str,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.require_role(Side::Runner)?;
        let job = self.jobs.get(job_id).ok_or(ProtocolError::Conflict)?;
        let outcome = job.result.ok_or(ProtocolError::Conflict)?;
        let digest = job.result_sha256.clone().ok_or(ProtocolError::Conflict)?;
        let message = Message::Result {
            for_job_sequence: job.sequence,
            outcome,
            output_sha256: digest,
        };
        let (agent, scope, task) = (
            job.agent_id.clone(),
            job.permission.clone(),
            job.task.clone(),
        );
        self.issue(job_id, &agent, &scope, (task, message), ttl_ms, clock)
    }

    /// Issue a fresh authenticated recovery request for an earlier message
    /// whose signed receipt might have been lost. No original instruction or
    /// tool side effect is replayed. The original sequence and digest are
    /// pinned in this endpoint's issued ledger.
    pub fn issue_receipt_reconciliation(
        &mut self,
        issued_sequence: u64,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.ensure_approved()?;
        let info = self
            .issued
            .get(&issued_sequence)
            .ok_or(ProtocolError::Conflict)?
            .clone();
        // A receipt itself is never a valid reconciliation target.
        // The peer will independently require the target to be non-receipt.
        self.issue(
            &info.job_id,
            &info.agent_id,
            &info.permission,
            (
                info.task,
                Message::ReconcileReceipt {
                    for_sequence: issued_sequence,
                    message_sha256: info.digest_sha256,
                },
            ),
            ttl_ms,
            clock,
        )
    }

    /// Only a message already authenticated through receive() can be
    /// acknowledged. Can acknowledge Job, Cancel or Result, without effects.
    pub fn issue_receipt(
        &mut self,
        received_sequence: u64,
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        self.ensure_approved()?;
        let received = self
            .received
            .get(&received_sequence)
            .ok_or(ProtocolError::Conflict)?;
        if received.is_receipt {
            return Err(ProtocolError::Conflict);
        }
        let info = received.info.clone();
        self.issue(
            &info.job_id,
            &info.agent_id,
            &info.permission,
            (
                info.task,
                Message::Receipt {
                    for_sequence: received_sequence,
                    message_sha256: info.digest_sha256,
                },
            ),
            ttl_ms,
            clock,
        )
    }

    fn issue(
        &mut self,
        job_id: &str,
        agent_id: &str,
        permission: &PermissionScope,
        attributed_message: (Option<TaskAttribution>, Message),
        ttl_ms: i64,
        clock: &impl Clock,
    ) -> Result<Vec<u8>> {
        let (task, message) = attributed_message;
        self.ensure_approved()?;
        if self.issued.len() >= MAX_TRACKED {
            return Err(ProtocolError::Capacity);
        }
        if self.next_outgoing == u64::MAX {
            return Err(ProtocolError::SequenceExhausted);
        }
        if !valid_id(job_id)
            || !valid_id(agent_id)
            || !valid_scope(permission)
            || task.as_ref().is_some_and(|task| !valid_task(task))
        {
            return Err(ProtocolError::Invalid);
        }
        validate_message(&message)?;
        if !(1..=self.policy.max_ttl_ms).contains(&ttl_ms) {
            return Err(ProtocolError::TimeWindow);
        }
        let now = clock.now_ms();
        let expiry = now.checked_add(ttl_ms).ok_or(ProtocolError::TimeWindow)?;
        if now < 0 {
            return Err(ProtocolError::TimeWindow);
        }
        let fields = AuthFields {
            header: Header {
                version: VERSION,
                workstation_id: self.pair.workstation_id.clone(),
                runner_id: self.pair.runner_id.clone(),
                sender: self.side,
                key_epoch: self.pair.epoch,
                sequence: self.next_outgoing,
                issued_at_ms: now,
                expires_at_ms: expiry,
                job_id: job_id.into(),
                agent_id: agent_id.into(),
                permission: permission.clone(),
                task,
            },
            message,
        };
        let signature = auth_tag(&self.pair.secret, &fields)?;
        let signed = SignedWire {
            header: fields.header,
            message: fields.message,
            tag: encode_hex(&signature),
        };
        let wire = serde_json::to_vec(&signed).map_err(|_| ProtocolError::Invalid)?;
        if wire.len() > MAX_WIRE_BYTES {
            return Err(ProtocolError::Oversized);
        }
        let info = info_for(&signed);
        let generation = self.next_generation()?;
        self.issued.insert(info.sequence, info);
        self.next_outgoing += 1;
        self.generation = generation;
        Ok(wire)
    }

    /// Authenticates and records a message. This low-level API cannot grant
    /// permission to execute its contents. Execution hosts should use
    /// `receive_authorized` to apply an independent local policy first.
    pub fn receive(&mut self, bytes: &[u8], clock: &impl Clock) -> Result<ReceiveOutcome> {
        self.receive_authorized(bytes, clock, |_, _| true)
            .map(|(outcome, _)| outcome)
    }

    /// Verify HMAC, then invoke a pure trusted-local authorization predicate
    /// for a NEW job before recording it. The callback receives authenticated
    /// agent/scope attribution and the signed job instruction. A rejection
    /// leaves counters/ledgers unchanged. Do not execute effects in the hook;
    /// persist a checkpoint before executing the returned authenticated job.
    pub fn receive_authorized(
        &mut self,
        bytes: &[u8],
        clock: &impl Clock,
        authorize_job: impl FnOnce(&MessageInfo, &Message) -> bool,
    ) -> Result<(ReceiveOutcome, Message)> {
        self.ensure_approved()?;
        if bytes.len() > MAX_WIRE_BYTES {
            return Err(ProtocolError::Oversized);
        }
        let signed: SignedWire =
            serde_json::from_slice(bytes).map_err(|_| ProtocolError::Invalid)?;
        // Only canonical, generated wire bytes are valid. This also rejects
        // otherwise ignored/unsigned unknown nested JSON fields and prevents
        // attackers from adding unauthenticated structural extensions.
        if serde_json::to_vec(&signed).map_err(|_| ProtocolError::Invalid)? != bytes {
            return Err(ProtocolError::Invalid);
        }
        let fields = AuthFields {
            header: signed.header.clone(),
            message: signed.message.clone(),
        };
        let tag = decode_hex(&signed.tag).ok_or(ProtocolError::Authentication)?;
        let signed_bytes = serde_json::to_vec(&fields).map_err(|_| ProtocolError::Invalid)?;
        let mut hmac = HmacSha256::new_from_slice(&self.pair.secret.0[..])
            .map_err(|_| ProtocolError::Authentication)?;
        hmac.update(&signed_bytes);
        hmac.verify_slice(&tag)
            .map_err(|_| ProtocolError::Authentication)?;

        let h = &signed.header;
        if h.workstation_id != self.pair.workstation_id
            || h.runner_id != self.pair.runner_id
            || h.sender != self.side.peer()
        {
            return Err(ProtocolError::Authentication);
        }
        if h.key_epoch != self.pair.epoch {
            return Err(ProtocolError::Epoch);
        }
        if h.version != VERSION
            || h.sequence == 0
            || !valid_id(&h.job_id)
            || !valid_id(&h.agent_id)
            || !valid_scope(&h.permission)
            || h.task.as_ref().is_some_and(|task| !valid_task(task))
        {
            return Err(ProtocolError::Invalid);
        }
        validate_message(&signed.message)?;
        let ttl = h
            .expires_at_ms
            .checked_sub(h.issued_at_ms)
            .ok_or(ProtocolError::TimeWindow)?;
        let now = clock.now_ms();
        if h.issued_at_ms < 0
            || now < 0
            || ttl <= 0
            || ttl > self.policy.max_ttl_ms
            || h.issued_at_ms > now.saturating_add(self.policy.max_future_skew_ms)
            || now >= h.expires_at_ms
        {
            return Err(ProtocolError::TimeWindow);
        }
        let info = info_for(&signed);
        if h.sequence <= self.last_incoming {
            if matches!(signed.message, Message::Receipt { .. })
                && self.received.get(&h.sequence).is_some_and(|prior| {
                    prior.is_receipt && prior.info.digest_sha256 == info.digest_sha256
                })
            {
                return Ok((ReceiveOutcome::DuplicateReceipt(info), signed.message));
            }
            return Err(ProtocolError::Replay);
        }
        if self.received.len() >= MAX_TRACKED {
            return Err(ProtocolError::Capacity);
        }
        let mut duplicate_receipt = false;
        let mut duplicate_result = false;
        match &signed.message {
            Message::Job { .. } => {
                if self.side != Side::Runner || self.jobs.contains_key(&h.job_id) {
                    return Err(ProtocolError::Conflict);
                }
                self.ensure_job_capacity()?;
                if !authorize_job(&info, &signed.message) {
                    return Err(ProtocolError::Unauthorized);
                }
            }
            Message::Cancel { for_job_sequence } => {
                if self.side != Side::Runner {
                    return Err(ProtocolError::Role);
                }
                let job = self.match_job(h, *for_job_sequence)?;
                if job.cancelled || job.result.is_some() {
                    return Err(ProtocolError::Conflict);
                }
            }
            Message::Result {
                for_job_sequence,
                outcome,
                output_sha256,
            } => {
                if self.side != Side::Workstation {
                    return Err(ProtocolError::Role);
                }
                let job = self.match_job(h, *for_job_sequence)?;
                // A workstation cancellation is a REQUEST, not proof the
                // runner has stopped. A successful result may legitimately
                // race ahead of delivery of that request. Only the runner
                // (which observes Cancel) prohibits success after receipt.
                if job.result.is_some() {
                    if job.result != Some(*outcome)
                        || job.result_sha256.as_deref() != Some(output_sha256.as_str())
                    {
                        return Err(ProtocolError::Conflict);
                    }
                    duplicate_result = true;
                }
            }
            Message::Receipt {
                for_sequence,
                message_sha256,
            } => {
                let sent = self
                    .issued
                    .get(for_sequence)
                    .ok_or(ProtocolError::Conflict)?;
                if sent.digest_sha256 != *message_sha256
                    || sent.job_id != h.job_id
                    || sent.agent_id != h.agent_id
                    || sent.permission != h.permission
                    || sent.task != h.task
                {
                    return Err(ProtocolError::Conflict);
                }
                duplicate_receipt = self.acknowledged.contains(for_sequence);
            }
            Message::ReconcileReceipt {
                for_sequence,
                message_sha256,
            } => {
                let recorded = self
                    .received
                    .get(for_sequence)
                    .ok_or(ProtocolError::Conflict)?;
                if recorded.is_receipt
                    || recorded.info.digest_sha256 != *message_sha256
                    || recorded.info.job_id != h.job_id
                    || recorded.info.agent_id != h.agent_id
                    || recorded.info.permission != h.permission
                    || recorded.info.task != h.task
                {
                    return Err(ProtocolError::Conflict);
                }
            }
        }
        // All rejecting checks are complete; subsequent changes are infallible.
        let generation = self.next_generation()?;
        match &signed.message {
            Message::Job { .. } => {
                self.jobs.insert(
                    h.job_id.clone(),
                    JobRecord {
                        sequence: h.sequence,
                        agent_id: h.agent_id.clone(),
                        permission: h.permission.clone(),
                        task: h.task.clone(),
                        cancelled: false,
                        result: None,
                        result_sha256: None,
                    },
                );
            }
            Message::Cancel { .. } => {
                if let Some(job) = self.jobs.get_mut(&h.job_id) {
                    job.cancelled = true;
                }
            }
            Message::Result {
                outcome,
                output_sha256,
                ..
            } => {
                if let Some(job) = self.jobs.get_mut(&h.job_id) {
                    job.result = Some(*outcome);
                    job.result_sha256 = Some(output_sha256.clone());
                }
            }
            Message::Receipt { for_sequence, .. } => {
                self.acknowledged.insert(*for_sequence);
            }
            Message::ReconcileReceipt { .. } => {}
        }
        self.received.insert(
            h.sequence,
            IndexedMessage {
                info: info.clone(),
                is_receipt: matches!(signed.message, Message::Receipt { .. }),
            },
        );
        self.last_incoming = h.sequence;
        self.generation = generation;
        if duplicate_receipt {
            Ok((ReceiveOutcome::DuplicateReceipt(info), signed.message))
        } else if duplicate_result {
            Ok((ReceiveOutcome::DuplicateResult(info), signed.message))
        } else {
            Ok((ReceiveOutcome::Applied(info), signed.message))
        }
    }

    fn match_job(&self, header: &Header, for_job_sequence: u64) -> Result<&JobRecord> {
        let job = self
            .jobs
            .get(&header.job_id)
            .ok_or(ProtocolError::Conflict)?;
        if job.sequence != for_job_sequence
            || job.agent_id != header.agent_id
            || job.permission != header.permission
            || job.task != header.task
        {
            return Err(ProtocolError::Conflict);
        }
        Ok(job)
    }

    fn ensure_approved(&self) -> Result<()> {
        if self.pair.status != PairStatus::Approved {
            return Err(ProtocolError::NotPaired);
        }
        Ok(())
    }

    fn require_role(&self, role: Side) -> Result<()> {
        self.ensure_approved()?;
        if self.side != role {
            return Err(ProtocolError::Role);
        }
        Ok(())
    }

    fn ensure_job_capacity(&self) -> Result<()> {
        if self.jobs.len() >= MAX_TRACKED {
            return Err(ProtocolError::Capacity);
        }
        Ok(())
    }

    fn next_generation(&self) -> Result<u64> {
        self.generation
            .checked_add(1)
            .ok_or(ProtocolError::SequenceExhausted)
    }
}

fn checkpoint_tag(secret: &Secret, payload: &[u8]) -> Result<[u8; 32]> {
    let mut mac =
        HmacSha256::new_from_slice(&secret.0[..]).map_err(|_| ProtocolError::Checkpoint)?;
    mac.update(CHECKPOINT_DOMAIN);
    mac.update(payload);
    Ok(mac.finalize().into_bytes().into())
}

fn valid_checkpoint_fields(fields: &CheckpointFields) -> bool {
    if fields.next_outgoing == 0
        || fields.issued.len() > MAX_TRACKED
        || fields.received.len() > MAX_TRACKED
        || fields.acknowledged.len() > MAX_TRACKED
        || fields.jobs.len() > MAX_TRACKED
        || fields
            .acknowledged
            .iter()
            .any(|seq| !fields.issued.contains_key(seq))
    {
        return false;
    }
    let valid_info = |sequence: &u64, info: &MessageInfo, sender: Side, upper: u64| {
        *sequence > 0
            && *sequence <= upper
            && info.sequence == *sequence
            && info.sender == sender
            && valid_id(&info.job_id)
            && valid_id(&info.agent_id)
            && valid_scope(&info.permission)
            && info.task.as_ref().is_none_or(valid_task)
            && valid_digest(&info.digest_sha256)
    };
    if fields
        .issued
        .iter()
        .any(|(seq, info)| !valid_info(seq, info, fields.side, fields.next_outgoing - 1))
        || fields.received.iter().any(|(seq, item)| {
            !valid_info(seq, &item.info, fields.side.peer(), fields.last_incoming)
        })
    {
        return false;
    }
    fields.jobs.iter().all(|(job_id, job)| {
        if !valid_id(job_id)
            || !valid_id(&job.agent_id)
            || !valid_scope(&job.permission)
            || job.task.as_ref().is_some_and(|task| !valid_task(task))
            || job
                .result_sha256
                .as_ref()
                .is_some_and(|digest| !valid_digest(digest))
            || (job.result.is_none() && job.result_sha256.is_some())
            || job.sequence == 0
            || (fields.side == Side::Runner
                && job.cancelled
                && matches!(job.result, Some(JobOutcome::Succeeded | JobOutcome::Failed)))
        {
            return false;
        }
        let related = if fields.side == Side::Workstation {
            fields.issued.get(&job.sequence)
        } else {
            fields.received.get(&job.sequence).map(|entry| &entry.info)
        };
        related.is_some_and(|info| {
            info.job_id == *job_id
                && info.agent_id == job.agent_id
                && info.permission == job.permission
                && info.task == job.task
                && info.sender == Side::Workstation
        })
    })
}

fn info_for(signed: &SignedWire) -> MessageInfo {
    let canonical = serde_json::to_vec(signed).expect("serializing a typed signed envelope");
    MessageInfo {
        job_id: signed.header.job_id.clone(),
        agent_id: signed.header.agent_id.clone(),
        permission: signed.header.permission.clone(),
        task: signed.header.task.clone(),
        sender: signed.header.sender,
        sequence: signed.header.sequence,
        digest_sha256: encode_hex(&Sha256::digest(canonical)),
    }
}

fn auth_tag(key: &Secret, fields: &AuthFields) -> Result<[u8; 32]> {
    let canonical = serde_json::to_vec(fields).map_err(|_| ProtocolError::Invalid)?;
    let mut mac =
        HmacSha256::new_from_slice(&key.0[..]).map_err(|_| ProtocolError::Authentication)?;
    mac.update(&canonical);
    Ok(mac.finalize().into_bytes().into())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

fn decode_hex(value: &str) -> Option<[u8; 32]> {
    if !valid_digest(value) {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let nibble = |character: u8| match character {
            b'0'..=b'9' => Some(character - b'0'),
            b'a'..=b'f' => Some(character - b'a' + 10),
            _ => None,
        };
        out[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, ch)| {
            if index == 0 {
                ch.is_ascii_alphanumeric()
            } else {
                ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.' | b':')
            }
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|ch| ch.is_ascii_digit() || (b'a'..=b'f').contains(&ch))
}

fn valid_scope(scope: &PermissionScope) -> bool {
    valid_id(&scope.resource_id)
}

fn valid_task(task: &TaskAttribution) -> bool {
    valid_id(&task.owner_id) && valid_id(&task.run_id) && valid_id(&task.task_id)
}

fn validate_message(message: &Message) -> Result<()> {
    match message {
        Message::Job { instruction } => {
            if instruction.trim().is_empty()
                || instruction.len() > MAX_INSTRUCTION_BYTES
                || instruction
                    .chars()
                    .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
            {
                return Err(ProtocolError::Invalid);
            }
        }
        Message::Cancel { for_job_sequence } => {
            if *for_job_sequence == 0 {
                return Err(ProtocolError::Invalid);
            }
        }
        Message::Result {
            for_job_sequence,
            output_sha256,
            ..
        } => {
            if *for_job_sequence == 0 || !valid_digest(output_sha256) {
                return Err(ProtocolError::Invalid);
            }
        }
        Message::Receipt {
            for_sequence,
            message_sha256,
        }
        | Message::ReconcileReceipt {
            for_sequence,
            message_sha256,
        } => {
            if *for_sequence == 0 || !valid_digest(message_sha256) {
                return Err(ProtocolError::Invalid);
            }
        }
    }
    Ok(())
}
