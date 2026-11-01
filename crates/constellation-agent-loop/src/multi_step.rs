//! Host-controlled multi-step admission. This module neither approves nor
//! dispatches any action without a separate native executor approval.
use crate::{
    parse_model_decision, AgentTurn, HostTurnCheckpoint, LoopError, ModelDecision,
    TrustedToolObservation, TurnEvent, TurnStatus, MAX_MODEL_ATTEMPTS, MAX_TOOL_STEPS,
};
use constellation_core::{OperationId, PermissionGate};
use constellation_tools::{
    CancellationToken, ExecutionOutput, ExecutionReceipt, LocalExecutor, OperationApproval,
    OperationContext, PreparedOperation,
};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 1;
const MAX_CHECKPOINT_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultiStepBudget {
    pub model_attempts: u8,
    pub tool_steps: u8,
}
impl MultiStepBudget {
    pub fn new(model_attempts: u8, tool_steps: u8) -> Result<Self, LoopError> {
        if model_attempts == 0
            || tool_steps == 0
            || model_attempts > MAX_MODEL_ATTEMPTS
            || tool_steps > MAX_TOOL_STEPS
        {
            return Err(LoopError::StepLimit);
        }
        Ok(Self {
            model_attempts,
            tool_steps,
        })
    }
}

/// The two unsettled stages are durable quarantine markers, never resumable
/// authorizations, model requests or executor operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiStepPhase {
    Settled,
    ProviderInFlight,
    NativeReviewRequired,
}

/// Native must HMAC/authenticate these JSON bytes and anchor a monotonic
/// revision outside the provider or renderer before trying to restore.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultiStepCheckpoint {
    version: u8,
    settled: HostTurnCheckpoint,
    budget: MultiStepBudget,
    reservations: u8,
    phase: MultiStepPhase,
}
impl MultiStepCheckpoint {
    pub fn phase(&self) -> MultiStepPhase {
        self.phase
    }
    pub fn revision(&self) -> u64 {
        self.settled.revision()
    }
    pub fn provider_reservations(&self) -> u8 {
        self.reservations
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>, LoopError> {
        let bytes = serde_json::to_vec(self).map_err(|_| LoopError::UnsafeCheckpoint)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(LoopError::Oversized);
        }
        Ok(bytes)
    }
}

/// Ephemeral response fence, not a grant or an operation ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelAttemptTicket {
    revision: u64,
    ordinal: u8,
}
impl ModelAttemptTicket {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn ordinal(&self) -> u8 {
        self.ordinal
    }
}

/// Each distinct proposal must pass independently through the old AgentTurn
/// native approval and LocalExecutor gate. A crash while provider/review is
/// pending permanently blocks recovery of this particular session.
pub struct MultiStepRun {
    turn: AgentTurn,
    budget: MultiStepBudget,
    reservations: u8,
    phase: MultiStepPhase,
    last_settled: HostTurnCheckpoint,
    ticket: Option<ModelAttemptTicket>,
}
impl MultiStepRun {
    pub fn new(context: OperationContext, budget: MultiStepBudget) -> Self {
        let turn = AgentTurn::new(context);
        let last_settled = turn.checkpoint_for_host().expect("new turn is settled");
        Self {
            turn,
            budget,
            reservations: 0,
            phase: MultiStepPhase::Settled,
            last_settled,
            ticket: None,
        }
    }
    pub fn status(&self) -> TurnStatus {
        self.turn.status()
    }
    pub fn revision(&self) -> u64 {
        self.turn.revision()
    }
    pub fn model_attempts(&self) -> u8 {
        self.turn.model_attempts()
    }
    pub fn provider_reservations(&self) -> u8 {
        self.reservations
    }
    pub fn tool_steps(&self) -> u8 {
        self.turn.tool_steps()
    }
    pub fn phase(&self) -> MultiStepPhase {
        self.phase
    }

    /// Persist this marker BEFORE polling a billable provider or displaying
    /// native consent. A pending marker cannot recover to an executable state.
    pub fn checkpoint_for_host(&self) -> MultiStepCheckpoint {
        MultiStepCheckpoint {
            version: VERSION,
            settled: self.last_settled.clone(),
            budget: self.budget,
            reservations: self.reservations,
            phase: self.phase,
        }
    }
    pub fn restore_from_host(
        context: OperationContext,
        bytes: &[u8],
        minimum_revision: u64,
    ) -> Result<Self, LoopError> {
        if bytes.is_empty() || bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(LoopError::UnsafeCheckpoint);
        }
        let cp: MultiStepCheckpoint =
            serde_json::from_slice(bytes).map_err(|_| LoopError::UnsafeCheckpoint)?;
        if cp.version != VERSION
            || cp.phase != MultiStepPhase::Settled
            || MultiStepBudget::new(cp.budget.model_attempts, cp.budget.tool_steps).is_err()
        {
            return Err(LoopError::UnsafeCheckpoint);
        }
        let turn = AgentTurn::restore_from_host(context, cp.settled.clone(), minimum_revision)?;
        let attempts = turn.model_attempts();
        if !(cp.reservations == attempts
            || (turn.status() == TurnStatus::Cancelled
                && cp.reservations == attempts.saturating_add(1)))
            || cp.reservations > cp.budget.model_attempts
            || attempts > cp.budget.model_attempts
            || turn.tool_steps() > cp.budget.tool_steps
        {
            return Err(LoopError::UnsafeCheckpoint);
        }
        Ok(Self {
            turn,
            budget: cp.budget,
            reservations: cp.reservations,
            phase: MultiStepPhase::Settled,
            last_settled: cp.settled,
            ticket: None,
        })
    }

    /// Reserve billing BEFORE the model request. Native must durably commit
    /// the returned provider-in-flight checkpoint before calling HTTP.
    pub fn reserve_model(
        &mut self,
        expected_revision: u64,
    ) -> Result<(ModelAttemptTicket, MultiStepCheckpoint), LoopError> {
        if expected_revision != self.revision() {
            return Err(LoopError::StaleModelStep);
        }
        if self.phase != MultiStepPhase::Settled || self.turn.status() != TurnStatus::AwaitingModel
        {
            return Err(LoopError::WrongState);
        }
        if self.reservations >= self.budget.model_attempts {
            return Err(LoopError::StepLimit);
        }
        self.reservations += 1;
        let ticket = ModelAttemptTicket {
            revision: expected_revision,
            ordinal: self.reservations,
        };
        self.ticket = Some(ticket);
        self.phase = MultiStepPhase::ProviderInFlight;
        Ok((ticket, self.checkpoint_for_host()))
    }
    fn verify_ticket(&self, ticket: ModelAttemptTicket) -> Result<(), LoopError> {
        if self.phase != MultiStepPhase::ProviderInFlight
            || self.ticket != Some(ticket)
            || self.revision() != ticket.revision
        {
            return Err(LoopError::StaleModelStep);
        }
        Ok(())
    }
    fn settle(&mut self) -> Result<(), LoopError> {
        self.ticket = None;
        self.phase = MultiStepPhase::Settled;
        self.last_settled = self.turn.checkpoint_for_host()?;
        Ok(())
    }
    /// A proposal still requires new native consent. Invalid or over-budget
    /// responses consume their charged attempt; no implicit OS effect follows.
    pub fn receive_model(
        &mut self,
        ticket: ModelAttemptTicket,
        raw: &str,
        executor: &LocalExecutor,
    ) -> Result<TurnEvent, LoopError> {
        self.verify_ticket(ticket)?;
        if self.turn.tool_steps() >= self.budget.tool_steps
            && matches!(
                parse_model_decision(raw),
                Ok(ModelDecision::Read { .. } | ModelDecision::Create { .. })
            )
        {
            let _ = self
                .turn
                .submit_model_at_revision(ticket.revision, "{}", executor);
            self.settle()?;
            return Err(LoopError::StepLimit);
        }
        let result = self
            .turn
            .submit_model_at_revision(ticket.revision, raw, executor);
        if self.turn.status() == TurnStatus::AwaitingApproval {
            self.phase = MultiStepPhase::NativeReviewRequired;
            self.ticket = None;
        } else {
            self.settle()?;
        }
        result
    }

    /// Charge a genuine native-observed failed provider request directly.
    /// No fake provider callback or model response is generated.
    pub fn provider_failed(&mut self, ticket: ModelAttemptTicket) -> Result<(), LoopError> {
        self.verify_ticket(ticket)?;
        let result = self
            .turn
            .charge_failed_provider_at_revision(ticket.revision);
        self.settle()?;
        result
    }

    pub fn pending_for_trusted_host(&self) -> Option<(OperationId, &PreparedOperation)> {
        if self.phase == MultiStepPhase::NativeReviewRequired {
            self.turn.pending_for_trusted_host()
        } else {
            None
        }
    }
    pub fn pending_create_content_for_native_review(&self) -> Option<&str> {
        if self.phase == MultiStepPhase::NativeReviewRequired {
            self.turn.pending_create_content_for_native_review()
        } else {
            None
        }
    }
    pub fn prepare_pending_read_for_native_review(
        &mut self,
        executor: &LocalExecutor,
    ) -> Result<(OperationId, &PreparedOperation), LoopError> {
        if self.phase != MultiStepPhase::NativeReviewRequired {
            return Err(LoopError::WrongState);
        }
        self.turn.prepare_pending_read_for_native_review(executor)
    }
    pub fn deny_with_observation(
        &mut self,
        executor: &LocalExecutor,
        operation_id: OperationId,
    ) -> Result<TrustedToolObservation, LoopError> {
        if self.phase != MultiStepPhase::NativeReviewRequired {
            return Err(LoopError::WrongState);
        }
        let observed = self.turn.deny_with_observation(executor, operation_id)?;
        self.settle()?;
        Ok(observed)
    }
    /// Native separately verifies task status, displays a real consent prompt,
    /// grants exact scope and supplies one-use executor approval per step.
    pub fn dispatch_approved_with_observation(
        &mut self,
        executor: &LocalExecutor,
        gate: &PermissionGate,
        approval: &OperationApproval,
        cancellation: &CancellationToken,
    ) -> Result<
        (
            ExecutionReceipt,
            Option<ExecutionOutput>,
            Option<TrustedToolObservation>,
        ),
        LoopError,
    > {
        if self.phase != MultiStepPhase::NativeReviewRequired {
            return Err(LoopError::WrongState);
        }
        let result =
            self.turn
                .dispatch_approved_with_observation(executor, gate, approval, cancellation);
        if self.turn.status() != TurnStatus::AwaitingApproval {
            self.settle()?;
        }
        result
    }
    pub fn cancel(&mut self, executor: &LocalExecutor) -> Result<(), LoopError> {
        self.turn.cancel(executor)?;
        self.settle()
    }
    /// Approved cancellation must consume that exact one-use executor token;
    /// untrusted provider text cannot invoke this operation.
    pub fn cancel_approved(
        &mut self,
        executor: &LocalExecutor,
        gate: &PermissionGate,
        approval: &OperationApproval,
    ) -> Result<ExecutionReceipt, LoopError> {
        if self.phase != MultiStepPhase::NativeReviewRequired {
            return Err(LoopError::WrongState);
        }
        let receipt = self.turn.cancel_approved(executor, gate, approval)?;
        self.settle()?;
        Ok(receipt)
    }
}
