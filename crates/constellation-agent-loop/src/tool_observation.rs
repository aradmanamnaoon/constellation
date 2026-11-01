//! A deliberately tiny native-host-to-model feedback boundary. Instances can
//! only be constructed after an actual executor denial or execution returned
//! to `AgentTurn`. This is not an approval, capability or persisted receipt.

use constellation_core::OperationId;
use constellation_tools::{ExecutionReceipt, OperationContext, OperationOutcome};

use crate::{LoopError, ProposedAction};

/// The model sees only fixed vocabulary; never a path, command, raw error,
/// user document, stdout, stderr, credential or arbitrary provider text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservedToolOutcome {
    Denied,
    Succeeded,
    Failed,
    TimedOut,
    OutputLimit,
}

impl ObservedToolOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::OutputLimit => "output_limit",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedToolObservation {
    context: OperationContext,
    operation_id: OperationId,
    revision: u64,
    action: ProposedAction,
    outcome: ObservedToolOutcome,
}

impl TrustedToolObservation {
    pub(crate) fn after_durable_denial(
        context: OperationContext,
        operation_id: OperationId,
        action: ProposedAction,
        revision: u64,
    ) -> Self {
        Self {
            context,
            operation_id,
            revision,
            action,
            outcome: ObservedToolOutcome::Denied,
        }
    }

    pub(crate) fn after_execution(
        context: OperationContext,
        receipt: &ExecutionReceipt,
        action: ProposedAction,
        revision: u64,
    ) -> Self {
        let outcome = match receipt.outcome {
            OperationOutcome::Succeeded => ObservedToolOutcome::Succeeded,
            OperationOutcome::Failed => ObservedToolOutcome::Failed,
            OperationOutcome::TimedOut => ObservedToolOutcome::TimedOut,
            OperationOutcome::OutputLimit => ObservedToolOutcome::OutputLimit,
            // Cancelled operations never yield a continuation observation;
            // the caller filters them before constructing this value.
            OperationOutcome::Cancelled => unreachable!("cancelled turn must not resume model"),
        };
        Self {
            context,
            operation_id: receipt.operation_id,
            revision,
            action,
            outcome,
        }
    }

    pub fn outcome(&self) -> ObservedToolOutcome {
        self.outcome
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Only the exact native principal may construct the next provider input.
    /// The resulting JSON is fixed vocabulary plus an opaque UUID and a
    /// monotonic revision, bounded independently of any file/output size.
    pub fn model_json_for(&self, context: OperationContext) -> Result<String, LoopError> {
        if context != self.context {
            return Err(LoopError::UnsafeCheckpoint);
        }
        let action = match self.action {
            ProposedAction::ReadFile => "read_file",
            ProposedAction::CreateFile => "create_file",
        };
        let json = serde_json::json!({
            "type": "trusted_tool_observation",
            "operation_id": self.operation_id.to_string(),
            "action": action,
            "outcome": self.outcome.label(),
            "revision": self.revision,
        })
        .to_string();
        if json.len() > 256 {
            return Err(LoopError::Oversized);
        }
        Ok(json)
    }
}
