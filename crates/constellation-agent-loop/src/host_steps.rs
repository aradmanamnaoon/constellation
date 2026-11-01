//! Host-owned progress snapshots and deterministic model-step input boundary.
//! Checkpoints must be authenticated and monotonically anchored by the host.
//! A checkpoint is deliberately incapable of restoring a pending tool grant.

use constellation_core::{AgentId, RoomId, TaskId};
use constellation_tools::{CancellationToken, LocalExecutor, OperationContext};
use serde::{Deserialize, Serialize};

use crate::{AgentTurn, LoopError, TurnEvent, TurnStatus, MAX_MODEL_ATTEMPTS, MAX_TOOL_STEPS};

const CHECKPOINT_VERSION: u8 = 1;

/// Only the trusted host may persist/load this value. Protect its bytes
/// with an independent authenticated store and a durable revision floor.
/// It intentionally contains no operation ID, prepared action, read result,
/// permission grant, consent evidence, or model-generated executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTurnCheckpoint {
    version: u8,
    agent_id: AgentId,
    room_id: RoomId,
    task_id: TaskId,
    status: TurnStatus,
    tool_steps: u8,
    model_attempts: u8,
    revision: u64,
}

impl HostTurnCheckpoint {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn status(&self) -> TurnStatus {
        self.status
    }
}

/// Metadata made available to a trusted deterministic model adapter. The
/// adapter owns its prompt/observations; no tool output is copied implicitly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelStepView {
    pub revision: u64,
    pub model_attempts: u8,
    pub tool_steps: u8,
}

/// A provider returns *untrusted* JSON text, never an approval or tool handle.
/// Host controls transport, pricing, cancellation, task and spend budgets.
pub trait ModelStepSource {
    fn next_response(&mut self, view: ModelStepView) -> Result<String, LoopError>;
}

impl AgentTurn {
    /// Safe only at a settled model boundary or a terminal state. A pending
    /// proposal is owned by the executor's durable approval/denial journal,
    /// so it must be reconciled by the host before a later checkpoint.
    pub fn checkpoint_for_host(&self) -> Result<HostTurnCheckpoint, LoopError> {
        if self.pending.is_some() || self.status == TurnStatus::AwaitingApproval {
            return Err(LoopError::UnsafeCheckpoint);
        }
        Ok(HostTurnCheckpoint {
            version: CHECKPOINT_VERSION,
            agent_id: self.context.agent_id,
            room_id: self.context.room_id,
            task_id: self.context.task_id,
            status: self.status,
            tool_steps: self.proposals,
            model_attempts: self.attempts,
            revision: self.revision,
        })
    }

    /// The host supplies an authenticated checkpoint and a separately
    /// persisted minimum revision; this function checks exact settled chronology
    /// but cannot verify HMAC, task state, or a missing approval journal.
    pub fn restore_from_host(
        context: OperationContext,
        checkpoint: HostTurnCheckpoint,
        minimum_revision: u64,
    ) -> Result<Self, LoopError> {
        // Every model attempt advances the revision exactly once. Every
        // successfully proposed tool advances it a second time when the
        // host durably denies or dispatches it. A terminal cancellation or
        // exhausted-step boundary may add one more revision. There is no
        // other reachable *settled* history in this coordinator. In
        // particular a fabricated AwaitingModel checkpoint must not omit a
        // pending tool's denial/dispatch or insert a phantom transition.
        let settled_revision =
            u64::from(checkpoint.model_attempts) + u64::from(checkpoint.tool_steps);
        let chronological = match checkpoint.status {
            TurnStatus::AwaitingModel | TurnStatus::Complete => {
                checkpoint.revision == settled_revision
            }
            TurnStatus::Cancelled => {
                checkpoint.revision == settled_revision
                    || checkpoint.revision == settled_revision + 1
            }
            TurnStatus::AwaitingApproval => false,
        };
        if checkpoint.version != CHECKPOINT_VERSION
            || checkpoint.agent_id != context.agent_id
            || checkpoint.room_id != context.room_id
            || checkpoint.task_id != context.task_id
            || !chronological
            || checkpoint.tool_steps > MAX_TOOL_STEPS
            || checkpoint.model_attempts > MAX_MODEL_ATTEMPTS
            || checkpoint.tool_steps > checkpoint.model_attempts
            || checkpoint.revision < minimum_revision
            || (checkpoint.status == TurnStatus::Complete && checkpoint.model_attempts == 0)
        {
            return Err(LoopError::UnsafeCheckpoint);
        }
        Ok(Self {
            context,
            status: checkpoint.status,
            proposals: checkpoint.tool_steps,
            attempts: checkpoint.model_attempts,
            revision: checkpoint.revision,
            pending: None,
        })
    }

    /// Drives exactly one model step. The expected revision fences duplicate
    /// delayed replies, and *failed provider calls count toward the budget*.
    /// The returned tool proposal still requires separate native user consent.
    pub fn advance_from_source(
        &mut self,
        expected_revision: u64,
        provider: &mut impl ModelStepSource,
        executor: &LocalExecutor,
    ) -> Result<TurnEvent, LoopError> {
        self.advance_from_source_with_cancellation(
            expected_revision,
            provider,
            executor,
            &CancellationToken::new(),
        )
    }

    /// A cancellation-aware *synchronous* provider turn. A cancellation
    /// before invocation does not contact the provider. If cancellation
    /// occurs while next_response blocks, the return is charged as an
    /// attempted model turn, but the returned text is never proposed to the
    /// tool executor or exposed as a final answer. Cancellation wins again
    /// after parsing, so even a late flag during proposal preparation denies
    /// the unapproved action before returning. The host must persist the
    /// terminal checkpoint and must NOT retry this cancelled turn.
    ///
    /// The provider callback is not interrupted or force-killed here: the
    /// native transport must still bound its time, costs and side effects.
    pub fn advance_from_source_with_cancellation(
        &mut self,
        expected_revision: u64,
        provider: &mut impl ModelStepSource,
        executor: &LocalExecutor,
        cancellation: &CancellationToken,
    ) -> Result<TurnEvent, LoopError> {
        if expected_revision != self.revision {
            return Err(LoopError::StaleModelStep);
        }
        if self.status != TurnStatus::AwaitingModel {
            return Err(LoopError::WrongState);
        }
        if cancellation.is_cancelled() {
            self.cancel(executor)?;
            return Err(LoopError::Cancelled);
        }
        if self.attempts >= MAX_MODEL_ATTEMPTS {
            // The existing model submission gate terminates the turn without
            // invoking an expensive or untrusted provider once exhausted.
            return self.submit_model_at_revision(expected_revision, "", executor);
        }
        let view = ModelStepView {
            revision: self.revision,
            model_attempts: self.attempts,
            tool_steps: self.proposals,
        };
        let reply = provider.next_response(view);
        if cancellation.is_cancelled() {
            // The request reached a provider; charging an attempt prevents a
            // slow or hostile source from repeatedly receiving free retries.
            self.attempts += 1;
            self.revision += 1;
            self.cancel(executor)?;
            return Err(LoopError::Cancelled);
        }
        let result = match reply {
            Ok(raw) => self.submit_model_at_revision(expected_revision, &raw, executor),
            Err(_) => {
                self.attempts += 1;
                self.revision += 1;
                Err(LoopError::Provider)
            }
        };
        if cancellation.is_cancelled() {
            match self.status {
                TurnStatus::AwaitingModel | TurnStatus::AwaitingApproval => {
                    self.cancel(executor)?;
                }
                TurnStatus::Complete => {
                    // Completion has not been returned to the host yet.
                    self.status = TurnStatus::Cancelled;
                    self.revision += 1;
                }
                TurnStatus::Cancelled => {}
            }
            return Err(LoopError::Cancelled);
        }
        result
    }
}
