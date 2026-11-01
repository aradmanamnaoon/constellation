//! A model proposes one structured action at a time. It never grants a tool
//! scope, constructs a native consent proof, or executes a device action.
//!
//! The trusted host must separately check the authoritative task state, show
//! the exact private `OperationPreview` in native UI, call `LocalExecutor::approve`
//! only after genuine user consent, and supply the resulting opaque approval.
//! This crate is an offline coordinator, not an autonomous desktop integration.

use constellation_core::{OperationId, PermissionGate};
use constellation_tools::{
    CancellationToken, ExecutionOutput, ExecutionReceipt, LocalExecutor, OperationApproval,
    OperationContext, OperationDetails, OperationRequest, PreparedOperation, ToolError, WriteMode,
};
use serde::de::{Error as _, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::fmt;
use thiserror::Error;

pub const MAX_MODEL_BYTES: usize = 16 * 1024;
pub const MAX_FINAL_BYTES: usize = 8 * 1024;
pub const MAX_CREATE_BYTES: usize = 8 * 1024;
pub const MAX_TOOL_STEPS: u8 = 8;
/// Includes invalid JSON, denied paths and failed operation preparation.
/// Persistent host-level token/time/spend budgets are still required.
pub const MAX_MODEL_ATTEMPTS: u8 = 16;

mod host_steps;
pub use host_steps::{HostTurnCheckpoint, ModelStepSource, ModelStepView};
mod multi_step;
pub use multi_step::{
    ModelAttemptTicket, MultiStepBudget, MultiStepCheckpoint, MultiStepPhase, MultiStepRun,
};
mod tool_observation;
pub use tool_observation::{ObservedToolOutcome, TrustedToolObservation};

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LoopError {
    #[error("model response exceeds the bounded protocol")]
    Oversized,
    #[error("model returned an invalid or unsupported structured action")]
    InvalidProposal,
    #[error("agent turn is not in the required state")]
    WrongState,
    #[error("agent tool step budget exhausted")]
    StepLimit,
    #[error("trusted tool operation failed")]
    Executor,
    #[error("a trusted approval already exists; cancel using cancel_approved")]
    ApprovedCancellationRequired,
    #[error("model response belongs to a stale iteration")]
    StaleModelStep,
    #[error("checkpoint is invalid, untrusted, or cannot safely resume")]
    UnsafeCheckpoint,
    #[error("trusted model provider failed to supply a response")]
    Provider,
    #[error("model step was cancelled before its response could be accepted")]
    Cancelled,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    AwaitingModel,
    AwaitingApproval,
    Complete,
    Cancelled,
}

pub enum ModelDecision {
    Final(String),
    Read { path: String },
    Create { path: String, content: String },
}

/// JSON object with exactly one value per key. Most JSON parsers silently
/// retain the last duplicate key, allowing a model/provider to show one
/// request in a transcript and dispatch another after normalization.
struct UniqueObject(Map<String, Value>);

impl<'de> Deserialize<'de> for UniqueObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = UniqueObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a unique-key JSON action object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = input.next_entry::<String, Value>()? {
                    if values.insert(key, value).is_some() {
                        return Err(A::Error::custom("duplicate tool decision field"));
                    }
                }
                Ok(UniqueObject(values))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

/// The wire protocol never includes an approval bool, permission grant,
/// operation ID or principal identity. Unknown fields fail closed.
pub fn parse_model_decision(raw: &str) -> Result<ModelDecision, LoopError> {
    if raw.is_empty() || raw.len() > MAX_MODEL_BYTES {
        return Err(LoopError::Oversized);
    }
    let UniqueObject(object) = serde_json::from_str(raw).map_err(|_| LoopError::InvalidProposal)?;
    let object = &object;
    match member(object, "type")? {
        "final" if exact_fields(object, &["type", "text"]) => {
            let text = member(object, "text")?;
            if text.trim().is_empty() || text.len() > MAX_FINAL_BYTES || forbidden_text(text) {
                return Err(LoopError::InvalidProposal);
            }
            Ok(ModelDecision::Final(text.to_owned()))
        }
        "tool" => {
            let action = member(object, "action")?;
            let path = member(object, "path")?;
            if path.is_empty() || path.len() > 1024 || !display_safe_path(path) {
                return Err(LoopError::InvalidProposal);
            }
            match action {
                "read_file" if exact_fields(object, &["type", "action", "path"]) => {
                    Ok(ModelDecision::Read {
                        path: path.to_owned(),
                    })
                }
                "create_file" if exact_fields(object, &["type", "action", "path", "content"]) => {
                    let content = member(object, "content")?;
                    if content.is_empty()
                        || content.len() > MAX_CREATE_BYTES
                        || forbidden_text(content)
                    {
                        return Err(LoopError::InvalidProposal);
                    }
                    Ok(ModelDecision::Create {
                        path: path.to_owned(),
                        content: content.to_owned(),
                    })
                }
                _ => Err(LoopError::InvalidProposal),
            }
        }
        _ => Err(LoopError::InvalidProposal),
    }
}

fn exact_fields(object: &Map<String, Value>, allowed: &[&str]) -> bool {
    object.len() == allowed.len() && allowed.iter().all(|key| object.contains_key(*key))
}

fn member<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, LoopError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or(LoopError::InvalidProposal)
}

fn forbidden_text(text: &str) -> bool {
    text.chars()
        .any(|ch| ch == '\0' || (ch.is_control() && ch != '\n' && ch != '\t'))
}

/// Model-proposed paths have one portable, relative spelling. Reject lexical
/// escapes, Windows alternate separators, drive/ADS prefixes, and Win32
/// reserved punctuation before a renderer ever sees a consent proposal.
/// Keep a visually unambiguous ASCII alphabet: native file pickers may still
/// handle Unicode filenames independently. Never silently normalize paths.
fn display_safe_path(value: &str) -> bool {
    value.bytes().all(|byte| {
        matches!(byte, b' '..=b'~')
            && !matches!(byte, b'\\' | b':' | b'<' | b'>' | b'|' | b'"' | b'?' | b'*')
    }) && value.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with('.')
            && !part.ends_with(' ')
            && !part.starts_with(' ')
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposedAction {
    ReadFile,
    CreateFile,
}

/// May be surfaced to a renderer for a *pending* proposal. Read contents,
/// size, digest and canonical workspace location are deliberately omitted.
/// The trusted native host must separately inspect the private prepared
/// operation before displaying its exact commitment to the user.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicOperationPreview {
    pub action: ProposedAction,
    pub relative_path: String,
    pub bytes: Option<usize>,
    pub sha256: Option<String>,
}

pub enum TurnEvent {
    Finished(String),
    ApprovalRequired {
        operation_id: OperationId,
        preview: PublicOperationPreview,
    },
}

struct Pending {
    operation_id: OperationId,
    prepared: Option<PreparedOperation>,
    /// Read targets are resolved only by the trusted host after an explicit
    /// user decision to inspect the path. In particular, an untrusted model
    /// cannot learn file existence, length, hash or text validity from the
    /// result of `submit_model`.
    deferred_read_path: Option<String>,
    /// Exact model-proposed UTF-8 bytes used to prepare a CreateNew action.
    /// Trusted host only: must be displayed independently in native consent.
    reviewed_create_content: Option<String>,
}

/// One bounded task-scoped iteration with no direct authorization channel.
pub struct AgentTurn {
    context: OperationContext,
    status: TurnStatus,
    proposals: u8,
    attempts: u8,
    revision: u64,
    pending: Option<Pending>,
}

impl AgentTurn {
    pub fn new(context: OperationContext) -> Self {
        Self {
            context,
            status: TurnStatus::AwaitingModel,
            proposals: 0,
            attempts: 0,
            revision: 0,
            pending: None,
        }
    }

    pub fn status(&self) -> TurnStatus {
        self.status
    }

    pub fn tool_steps(&self) -> u8 {
        self.proposals
    }

    pub fn model_attempts(&self) -> u8 {
        self.attempts
    }

    /// Charge a host-observed failed provider reservation without a fabricated
    /// model response or preparing any tool. Only the multi-step host layer in
    /// this crate calls this method.
    pub(crate) fn charge_failed_provider_at_revision(
        &mut self,
        expected_revision: u64,
    ) -> Result<(), LoopError> {
        if expected_revision != self.revision {
            return Err(LoopError::StaleModelStep);
        }
        if self.status != TurnStatus::AwaitingModel {
            return Err(LoopError::WrongState);
        }
        if self.attempts >= MAX_MODEL_ATTEMPTS {
            self.status = TurnStatus::Cancelled;
            self.revision += 1;
            return Err(LoopError::StepLimit);
        }
        self.attempts += 1;
        self.revision += 1;
        Ok(())
    }

    /// Monotonic iteration fence. Pass this value with a trusted provider
    /// request, then submit its answer only against the same revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The native host may consume the single independently authenticated
    /// model-origin file result to reserve ONE subsequent *text-only* model
    /// request. Charge the attempt and advance the signed checkpoint BEFORE
    /// any billable HTTP request. Reopening this checkpoint cannot authorize
    /// a second request: only the exact first settled tool revision (2) is
    /// admissible. This does not grant, prepare or execute tools.
    pub fn reserve_single_final_followup(
        &mut self,
        expected_revision: u64,
    ) -> Result<u64, LoopError> {
        if expected_revision != self.revision {
            return Err(LoopError::StaleModelStep);
        }
        if expected_revision != 2
            || self.status != TurnStatus::AwaitingModel
            || self.pending.is_some()
            || self.proposals != 1
            || self.attempts != 1
        {
            return Err(LoopError::WrongState);
        }
        if self.attempts >= MAX_MODEL_ATTEMPTS {
            return Err(LoopError::StepLimit);
        }
        self.attempts += 1;
        self.revision += 1;
        Ok(self.revision)
    }

    /// Rejects late/duplicate asynchronous responses before charging a new
    /// attempt or preparing another tool operation.
    pub fn submit_model_at_revision(
        &mut self,
        revision: u64,
        raw: &str,
        executor: &LocalExecutor,
    ) -> Result<TurnEvent, LoopError> {
        if revision != self.revision {
            return Err(LoopError::StaleModelStep);
        }
        self.submit_model(raw, executor)
    }

    pub fn pending_for_trusted_host(&self) -> Option<(OperationId, &PreparedOperation)> {
        self.pending
            .as_ref()
            .and_then(|pending| pending.prepared.as_ref().map(|p| (pending.operation_id, p)))
    }

    /// Called only by the trusted host after it has obtained independent
    /// consent to inspect the read target. The host must then review the
    /// exact returned SHA, length and canonical path before granting scope and
    /// issuing the one-use executor approval. A failed preparation leaves the
    /// proposal pending so the host can reject it without an effect.
    pub fn prepare_pending_read_for_native_review(
        &mut self,
        executor: &LocalExecutor,
    ) -> Result<(OperationId, &PreparedOperation), LoopError> {
        if self.status != TurnStatus::AwaitingApproval {
            return Err(LoopError::WrongState);
        }
        let pending = self.pending.as_mut().ok_or(LoopError::WrongState)?;
        if pending.prepared.is_none() {
            let path = pending
                .deferred_read_path
                .as_ref()
                .ok_or(LoopError::WrongState)?;
            pending.prepared = Some(
                executor
                    .prepare(
                        self.context,
                        OperationRequest::Read {
                            path: path.clone().into(),
                        },
                    )
                    .map_err(|_| LoopError::Executor)?,
            );
        }
        Ok((
            pending.operation_id,
            pending.prepared.as_ref().ok_or(LoopError::WrongState)?,
        ))
    }

    /// Native host may display the original model-proposed create bytes.
    /// Reads deliberately return None; they are approved by a pinned hash.
    /// The returned borrow is stable for this pending operation and must
    /// never be rederived from a later model message or renderer input.
    pub fn pending_create_content_for_native_review(&self) -> Option<&str> {
        self.pending
            .as_ref()
            .and_then(|pending| pending.reviewed_create_content.as_deref())
    }

    pub fn submit_model(
        &mut self,
        raw: &str,
        executor: &LocalExecutor,
    ) -> Result<TurnEvent, LoopError> {
        if self.status != TurnStatus::AwaitingModel {
            return Err(LoopError::WrongState);
        }
        if self.attempts >= MAX_MODEL_ATTEMPTS {
            self.status = TurnStatus::Cancelled;
            self.revision += 1;
            return Err(LoopError::StepLimit);
        }
        // Charge every candidate before parsing or checking paths. This
        // prevents infinitely many malformed/failed provider iterations.
        self.attempts += 1;
        self.revision += 1;
        match parse_model_decision(raw)? {
            ModelDecision::Final(text) => {
                self.status = TurnStatus::Complete;
                Ok(TurnEvent::Finished(text))
            }
            proposal => {
                if self.proposals >= MAX_TOOL_STEPS {
                    return Err(LoopError::StepLimit);
                }
                let (request, reviewed_create_content, public_action, relative_path) =
                    match proposal {
                        ModelDecision::Read { path } => {
                            (None, None, ProposedAction::ReadFile, path)
                        }
                        ModelDecision::Create { path, content } => {
                            let bytes = content.as_bytes().to_vec();
                            (
                                Some(OperationRequest::Write {
                                    path: path.clone().into(),
                                    bytes,
                                    mode: WriteMode::CreateNew,
                                }),
                                Some(content),
                                ProposedAction::CreateFile,
                                path,
                            )
                        }
                        ModelDecision::Final(_) => unreachable!(),
                    };
                let deferred_read_path = if request.is_none() {
                    Some(relative_path.clone())
                } else {
                    None
                };
                let prepared = request
                    .map(|request| {
                        executor
                            .prepare(self.context, request)
                            .map_err(|_| LoopError::Executor)
                    })
                    .transpose()?;
                let (public_bytes, public_sha256) =
                    match prepared.as_ref().map(|p| p.preview().details) {
                        None | Some(OperationDetails::Read { .. }) => (None, None),
                        Some(OperationDetails::Write { bytes, sha256, .. }) => {
                            (Some(bytes), Some(sha256))
                        }
                        Some(OperationDetails::Command { .. }) => return Err(LoopError::Executor),
                    };
                let operation_id = OperationId::new();
                self.proposals += 1;
                self.pending = Some(Pending {
                    operation_id,
                    prepared,
                    deferred_read_path,
                    reviewed_create_content,
                });
                self.status = TurnStatus::AwaitingApproval;
                Ok(TurnEvent::ApprovalRequired {
                    operation_id,
                    preview: PublicOperationPreview {
                        action: public_action,
                        relative_path,
                        bytes: public_bytes,
                        sha256: public_sha256,
                    },
                })
            }
        }
    }

    /// The host must durably record the user's denial before advancing the
    /// agent. A deferred read is denied by its lexical intent without opening
    /// the file; a prepared operation is denied by its immutable digest.
    /// Neither path permits reuse of the rejected operation ID.
    pub fn deny(
        &mut self,
        executor: &LocalExecutor,
        operation_id: OperationId,
    ) -> Result<(), LoopError> {
        let pending = self.pending.as_ref().ok_or(LoopError::WrongState)?;
        if self.status != TurnStatus::AwaitingApproval || pending.operation_id != operation_id {
            return Err(LoopError::WrongState);
        }
        if let Some(prepared) = &pending.prepared {
            executor
                .deny(operation_id, prepared)
                .map_err(|_| LoopError::Executor)?;
        } else if let Some(path) = &pending.deferred_read_path {
            executor
                .deny_unprepared_read(operation_id, self.context, path)
                .map_err(|_| LoopError::Executor)?;
        } else {
            return Err(LoopError::WrongState);
        }
        self.pending = None;
        self.status = TurnStatus::AwaitingModel;
        self.revision += 1;
        Ok(())
    }

    /// A host-owned observation is issued only after the executor has
    /// durably accepted the exact denial. The model cannot manufacture this
    /// value from its JSON proposal or claim that native approval occurred.
    pub fn deny_with_observation(
        &mut self,
        executor: &LocalExecutor,
        operation_id: OperationId,
    ) -> Result<TrustedToolObservation, LoopError> {
        let action = self.pending_observation_action()?;
        self.deny(executor, operation_id)?;
        Ok(TrustedToolObservation::after_durable_denial(
            self.context,
            operation_id,
            action,
            self.revision,
        ))
    }

    fn pending_observation_action(&self) -> Result<ProposedAction, LoopError> {
        if self.status != TurnStatus::AwaitingApproval {
            return Err(LoopError::WrongState);
        }
        let pending = self.pending.as_ref().ok_or(LoopError::WrongState)?;
        match (
            pending.reviewed_create_content.is_some(),
            pending.deferred_read_path.is_some(),
            pending.prepared.as_ref().map(|p| p.preview().details),
        ) {
            (
                true,
                false,
                Some(OperationDetails::Write {
                    mode: WriteMode::CreateNew,
                    ..
                }),
            ) => Ok(ProposedAction::CreateFile),
            (false, true, None | Some(OperationDetails::Read { .. })) => {
                Ok(ProposedAction::ReadFile)
            }
            _ => Err(LoopError::WrongState),
        }
    }

    /// Only a separate trusted UI path may obtain `approval` from the
    /// executor. The model protocol contains no representation of this token.
    pub fn dispatch_approved(
        &mut self,
        executor: &LocalExecutor,
        gate: &PermissionGate,
        approval: &OperationApproval,
        cancellation: &CancellationToken,
    ) -> Result<(ExecutionReceipt, Option<ExecutionOutput>), LoopError> {
        let pending = self.pending.as_ref().ok_or(LoopError::WrongState)?;
        if self.status != TurnStatus::AwaitingApproval
            || approval.operation_id != pending.operation_id
            || pending.prepared.as_ref().map(|p| p.digest()) != Some(approval.digest.as_str())
        {
            return Err(LoopError::WrongState);
        }
        let pending = self.pending.take().ok_or(LoopError::WrongState)?;
        let prepared = pending.prepared.ok_or(LoopError::WrongState)?;
        // Executor itself verifies permission, digest, journal and single use.
        // Denial, cancellation and failures never create an implicit grant.
        let result = executor
            .execute(gate, approval, prepared, cancellation)
            .map_err(|_| LoopError::Executor);
        self.status = match &result {
            Ok((receipt, _))
                if receipt.outcome == constellation_tools::OperationOutcome::Cancelled =>
            {
                TurnStatus::Cancelled
            }
            Ok(_) => TurnStatus::AwaitingModel,
            // A failed authorization/journal boundary ends this turn; never
            // silently generate another proposal against uncertain authority.
            Err(_) => TurnStatus::Cancelled,
        };
        self.revision += 1;
        result
    }

    /// Execute using a *separately* granted one-use native approval and issue
    /// an immutable bounded observation derived only from the actual executor
    /// result. The observation contains no file bytes, raw errors, paths or
    /// credential-bearing output. For cancellation or uncertain failure no
    /// subsequent model step is authorized and no observation is returned.
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
        let action = self.pending_observation_action()?;
        let (receipt, output) = self.dispatch_approved(executor, gate, approval, cancellation)?;
        let observation = (self.status == TurnStatus::AwaitingModel).then(|| {
            TrustedToolObservation::after_execution(self.context, &receipt, action, self.revision)
        });
        Ok((receipt, output, observation))
    }

    pub fn cancel(&mut self, executor: &LocalExecutor) -> Result<(), LoopError> {
        if self.status == TurnStatus::Complete || self.status == TurnStatus::Cancelled {
            return Err(LoopError::WrongState);
        }
        if let Some(pending) = &self.pending {
            let rejection = if let Some(prepared) = &pending.prepared {
                executor.deny(pending.operation_id, prepared)
            } else if let Some(path) = &pending.deferred_read_path {
                executor.deny_unprepared_read(pending.operation_id, self.context, path)
            } else {
                return Err(LoopError::WrongState);
            };
            match rejection {
                Ok(()) => (),
                Err(ToolError::OperationReplay) => {
                    return Err(LoopError::ApprovedCancellationRequired);
                }
                Err(_) => return Err(LoopError::Executor),
            }
        }
        self.pending = None;
        self.status = TurnStatus::Cancelled;
        self.revision += 1;
        Ok(())
    }

    /// If the trusted host has already durably approved the operation, use
    /// the executor's cancelled dispatch path to consume that approval and
    /// persist a cancelled receipt. A pre-approval `deny` cannot overwrite
    /// an existing approval journal entry.
    pub fn cancel_approved(
        &mut self,
        executor: &LocalExecutor,
        gate: &PermissionGate,
        approval: &OperationApproval,
    ) -> Result<ExecutionReceipt, LoopError> {
        let token = CancellationToken::new();
        token.cancel();
        let (receipt, output) = self.dispatch_approved(executor, gate, approval, &token)?;
        self.status = TurnStatus::Cancelled;
        if receipt.outcome != constellation_tools::OperationOutcome::Cancelled || output.is_some() {
            return Err(LoopError::Executor);
        }
        Ok(receipt)
    }
}
