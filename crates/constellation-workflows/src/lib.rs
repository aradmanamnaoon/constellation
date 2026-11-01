//! Deterministic, effect-free workflows for a trusted host to orchestrate.
//! No model selection, execution, filesystem access, network calls, or grants.

use std::collections::{BTreeMap, BTreeSet};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use thiserror::Error;

mod repository;
pub use repository::{ClaimedStep, Repository, RepositoryError, RepositoryResult, StoredWorkflow};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_STEPS: usize = 64;
pub const MAX_JSON_BYTES: usize = 512 * 1024;
pub const MAX_INSTRUCTION_BYTES: usize = 4096;
/// A successful step may retain a bounded text artifact in the same durable
/// SQLite revision as its terminal outcome.
pub const MAX_RESULT_BYTES: usize = 16 * 1024;
pub const MAX_ATTEMPTS: u8 = 10;

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("unsupported workflow schema version {0}")]
    UnsupportedVersion(u32),
    #[error("workflow must contain between 1 and 64 steps")]
    StepCount,
    #[error("workflow JSON exceeds the byte limit")]
    InputTooLarge,
    #[error("invalid {0}")]
    InvalidField(&'static str),
    #[error("duplicate step id: {0}")]
    DuplicateId(String),
    #[error("step {step} has duplicate dependency {dependency}")]
    DuplicateDependency { step: String, dependency: String },
    #[error("step {step} references missing dependency {dependency}")]
    MissingDependency { step: String, dependency: String },
    #[error("workflow dependencies contain a cycle")]
    Cycle,
    #[error("unknown step: {0}")]
    UnknownStep(String),
    #[error("step is not ready: {0}")]
    NotReady(String),
    #[error("invalid state transition for step {0}")]
    InvalidTransition(String),
    #[error("step {step} expected attempt {expected}, received {actual}")]
    AttemptMismatch {
        step: String,
        expected: u8,
        actual: u8,
    },
    #[error("conflicting completion for step {0}")]
    ConflictingCompletion(String),
    #[error("invalid restored workflow state")]
    InvalidState,
    #[error("workflow result must be nonempty and at most 16 KiB")]
    InvalidResult,
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, WorkflowError>;

/// One task. Every task explicitly names its role, assigned agent and model
/// provider. The host must resolve the provider independently and obtain any
/// tool permissions separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDefinition {
    pub id: String,
    pub role: String,
    pub agent_id: String,
    pub provider_id: String,
    pub instruction: String,
    pub depends_on: Vec<String>,
    pub max_attempts: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkflowDefinition {
    pub schema_version: u32,
    pub workflow_id: String,
    pub steps: Vec<StepDefinition>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionWire {
    schema_version: u32,
    workflow_id: String,
    steps: Vec<StepDefinition>,
}

impl<'de> Deserialize<'de> for WorkflowDefinition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let wire = DefinitionWire::deserialize(deserializer)?;
        let definition = Self {
            schema_version: wire.schema_version,
            workflow_id: wire.workflow_id,
            steps: wire.steps,
        };
        definition.validate().map_err(D::Error::custom)?;
        Ok(definition)
    }
}

impl WorkflowDefinition {
    pub fn from_json_bounded(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_JSON_BYTES {
            return Err(WorkflowError::InputTooLarge);
        }
        Ok(serde_json::from_slice(input)?)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(WorkflowError::UnsupportedVersion(self.schema_version));
        }
        if !valid_identifier(&self.workflow_id, 128) {
            return Err(WorkflowError::InvalidField("workflow_id"));
        }
        if self.steps.is_empty() || self.steps.len() > MAX_STEPS {
            return Err(WorkflowError::StepCount);
        }
        let mut ids = BTreeSet::new();
        for step in &self.steps {
            if !valid_identifier(&step.id, 64) {
                return Err(WorkflowError::InvalidField("step.id"));
            }
            if !valid_label(&step.role, 64) {
                return Err(WorkflowError::InvalidField("step.role"));
            }
            if !valid_identifier(&step.agent_id, 128) {
                return Err(WorkflowError::InvalidField("step.agent_id"));
            }
            if !valid_identifier(&step.provider_id, 128) {
                return Err(WorkflowError::InvalidField("step.provider_id"));
            }
            if step.instruction.trim().is_empty()
                || step.instruction.len() > MAX_INSTRUCTION_BYTES
                || step
                    .instruction
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return Err(WorkflowError::InvalidField("step.instruction"));
            }
            if !(1..=MAX_ATTEMPTS).contains(&step.max_attempts) {
                return Err(WorkflowError::InvalidField("step.max_attempts"));
            }
            if step.depends_on.len() > MAX_STEPS {
                return Err(WorkflowError::InvalidField("step.depends_on"));
            }
            if !ids.insert(step.id.as_str()) {
                return Err(WorkflowError::DuplicateId(step.id.clone()));
            }
        }
        for step in &self.steps {
            let mut deps = BTreeSet::new();
            for dep in &step.depends_on {
                if !deps.insert(dep) {
                    return Err(WorkflowError::DuplicateDependency {
                        step: step.id.clone(),
                        dependency: dep.clone(),
                    });
                }
                if !ids.contains(dep.as_str()) {
                    return Err(WorkflowError::MissingDependency {
                        step: step.id.clone(),
                        dependency: dep.clone(),
                    });
                }
            }
        }
        self.topological_order_unchecked()?;
        Ok(())
    }

    /// Stable lexicographically tie-broken topological ordering, independent
    /// of input step order and dependency-list order.
    pub fn topological_order(&self) -> Result<Vec<String>> {
        self.validate()?;
        self.topological_order_unchecked()
    }

    fn topological_order_unchecked(&self) -> Result<Vec<String>> {
        let mut indegree = BTreeMap::<&str, usize>::new();
        let mut outgoing = BTreeMap::<&str, Vec<&str>>::new();
        for step in &self.steps {
            indegree.insert(&step.id, step.depends_on.len());
            for dep in &step.depends_on {
                outgoing.entry(dep).or_default().push(&step.id);
            }
        }
        let mut ready: BTreeSet<&str> = indegree
            .iter()
            .filter_map(|(&id, &count)| (count == 0).then_some(id))
            .collect();
        let mut order = Vec::with_capacity(self.steps.len());
        while let Some(id) = ready.pop_first() {
            order.push(id.to_owned());
            if let Some(children) = outgoing.get(id) {
                for child in children {
                    let remaining = indegree.get_mut(child).ok_or(WorkflowError::Cycle)?;
                    *remaining -= 1;
                    if *remaining == 0 {
                        ready.insert(child);
                    }
                }
            }
        }
        if order.len() != self.steps.len() {
            return Err(WorkflowError::Cycle);
        }
        Ok(order)
    }
}

fn valid_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.bytes().enumerate().all(|(index, ch)| {
            if index == 0 {
                ch.is_ascii_alphanumeric()
            } else {
                ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-' | b'.' | b':')
            }
        })
}

fn valid_label(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    /// A running attempt was restored after a restart. Its side effects are
    /// unknown and it must not be redispatched or completed automatically.
    Blocked,
    AwaitingRetry,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionOutcome {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptCompletion {
    pub attempt: u8,
    pub outcome: CompletionOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepProgress {
    pub status: StepStatus,
    pub attempts: u8,
    pub last_completion: Option<AttemptCompletion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_text: Option<String>,
}

impl Default for StepProgress {
    fn default() -> Self {
        Self {
            status: StepStatus::Pending,
            attempts: 0,
            last_completion: None,
            result_text: None,
        }
    }
}

/// The specific approved dispatch *proposal*. A caller must separately
/// resolve the named provider, verify it is configured, and authorize tools.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDispatch {
    pub workflow_id: String,
    pub step_id: String,
    pub attempt: u8,
    pub role: String,
    pub agent_id: String,
    pub provider_id: String,
    pub instruction: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkflowRun {
    pub schema_version: u32,
    definition: WorkflowDefinition,
    states: BTreeMap<String, StepProgress>,
    cancelled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunWire {
    schema_version: u32,
    definition: WorkflowDefinition,
    states: BTreeMap<String, StepProgress>,
    cancelled: bool,
}

impl<'de> Deserialize<'de> for WorkflowRun {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let wire = RunWire::deserialize(deserializer)?;
        let mut run = Self {
            schema_version: wire.schema_version,
            definition: wire.definition,
            states: wire.states,
            cancelled: wire.cancelled,
        };
        // Validate the unmodified persisted state first, then consume every
        // in-flight attempt without changing its ID or completion history.
        // This is the only JSON restoration path, including direct serde use.
        run.validate_state().map_err(D::Error::custom)?;
        for progress in run.states.values_mut() {
            if progress.status == StepStatus::Running {
                progress.status = StepStatus::Blocked;
            }
        }
        Ok(run)
    }
}

impl WorkflowRun {
    pub fn new(definition: WorkflowDefinition) -> Result<Self> {
        definition.validate()?;
        let states = definition
            .steps
            .iter()
            .map(|step| (step.id.clone(), StepProgress::default()))
            .collect();
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            definition,
            states,
            cancelled: false,
        })
    }

    pub fn from_json_bounded(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_JSON_BYTES {
            return Err(WorkflowError::InputTooLarge);
        }
        Ok(serde_json::from_slice(input)?)
    }

    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate_state()?;
        Ok(serde_json::to_vec(self)?)
    }

    pub fn definition(&self) -> &WorkflowDefinition {
        &self.definition
    }

    pub fn progress(&self, step_id: &str) -> Option<&StepProgress> {
        self.states.get(step_id)
    }

    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn states(&self) -> &BTreeMap<String, StepProgress> {
        &self.states
    }

    /// All runnable pending steps in stable ID order. No work starts here.
    pub fn ready_steps(&self) -> Vec<String> {
        if self.cancelled {
            return Vec::new();
        }
        let mut ready = BTreeSet::new();
        for step in &self.definition.steps {
            if self.states[&step.id].status == StepStatus::Pending
                && step
                    .depends_on
                    .iter()
                    .all(|dependency| self.states[dependency].status == StepStatus::Succeeded)
            {
                ready.insert(step.id.clone());
            }
        }
        ready.into_iter().collect()
    }

    /// Idempotent for an already-running step in this live in-memory instance.
    /// Deserialization converts persisted Running steps to Blocked: restarting
    /// the host can never retrieve an old dispatch via this method.
    pub fn start(&mut self, step_id: &str) -> Result<StepDispatch> {
        let step = self.step(step_id)?.clone();
        if self.cancelled {
            return Err(WorkflowError::InvalidTransition(step_id.into()));
        }
        let current = self.states[step_id].status;
        if current != StepStatus::Running {
            if current != StepStatus::Pending || !self.ready_steps().contains(&step.id) {
                return Err(WorkflowError::NotReady(step_id.into()));
            }
            let state = self
                .states
                .get_mut(step_id)
                .ok_or(WorkflowError::InvalidState)?;
            state.attempts += 1;
            state.status = StepStatus::Running;
        }
        Ok(StepDispatch {
            workflow_id: self.definition.workflow_id.clone(),
            step_id: step.id,
            attempt: self.states[step_id].attempts,
            role: step.role,
            agent_id: step.agent_id,
            provider_id: step.provider_id,
            instruction: step.instruction,
        })
    }

    /// Completion must provide the exact attempt returned from start.
    /// Duplicate reports for the latest attempt and outcome are no-ops;
    /// conflicting reports and stale older attempts are rejected.
    pub fn complete(
        &mut self,
        step_id: &str,
        attempt: u8,
        outcome: CompletionOutcome,
    ) -> Result<StepStatus> {
        self.complete_with_result(step_id, attempt, outcome, None)
    }

    /// Persist a bounded model artifact and the succeeded state together. A
    /// failed attempt has no artifact. Older plans with no result remain valid.
    pub fn complete_with_result(
        &mut self,
        step_id: &str,
        attempt: u8,
        outcome: CompletionOutcome,
        result_text: Option<String>,
    ) -> Result<StepStatus> {
        if result_text.as_ref().is_some_and(|value| {
            outcome != CompletionOutcome::Succeeded
                || value.trim().is_empty()
                || value.len() > MAX_RESULT_BYTES
                || value
                    .chars()
                    .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        }) {
            return Err(WorkflowError::InvalidResult);
        }
        let step = self.step(step_id)?;
        let max_attempts = step.max_attempts;
        let state = self
            .states
            .get_mut(step_id)
            .ok_or(WorkflowError::InvalidState)?;
        if attempt != state.attempts || attempt == 0 {
            return Err(WorkflowError::AttemptMismatch {
                step: step_id.into(),
                expected: state.attempts,
                actual: attempt,
            });
        }
        if let Some(last) = &state.last_completion {
            if last.attempt == attempt {
                if last.outcome == outcome && state.result_text == result_text {
                    return Ok(state.status);
                }
                return Err(WorkflowError::ConflictingCompletion(step_id.into()));
            }
        }
        if self.cancelled {
            return Err(WorkflowError::InvalidTransition(step_id.into()));
        }
        if state.status != StepStatus::Running {
            return Err(WorkflowError::InvalidTransition(step_id.into()));
        }
        state.last_completion = Some(AttemptCompletion { attempt, outcome });
        state.result_text = result_text;
        state.status = match outcome {
            CompletionOutcome::Succeeded => StepStatus::Succeeded,
            CompletionOutcome::Failed if attempt < max_attempts => StepStatus::AwaitingRetry,
            CompletionOutcome::Failed => StepStatus::Failed,
        };
        let status = state.status;
        if status == StepStatus::Failed {
            self.propagate_skips();
        }
        Ok(status)
    }

    /// Retry is an explicit scheduling decision. A retryable failure does not
    /// poison dependents until attempts are exhausted or the step is skipped.
    /// Calling retry again while already pending is an idempotent no-op.
    pub fn retry(&mut self, step_id: &str) -> Result<bool> {
        let max_attempts = self.step(step_id)?.max_attempts;
        if self.cancelled {
            return Err(WorkflowError::InvalidTransition(step_id.into()));
        }
        let state = self
            .states
            .get_mut(step_id)
            .ok_or(WorkflowError::InvalidState)?;
        if state.status == StepStatus::AwaitingRetry && state.attempts < max_attempts {
            state.status = StepStatus::Pending;
            return Ok(true);
        }
        if state.status == StepStatus::Pending
            && state.attempts > 0
            && state.attempts < max_attempts
            && state.last_completion.as_ref().is_some_and(|c| {
                c.attempt == state.attempts && c.outcome == CompletionOutcome::Failed
            })
        {
            return Ok(false);
        }
        Err(WorkflowError::InvalidTransition(step_id.into()))
    }

    /// Explicitly abandon a pending, retryable or restart-blocked step.
    /// Transitively skip all successors. Live Running steps cannot be skipped.
    /// A Blocked step requires the trusted host to obtain the user's decision.
    pub fn skip(&mut self, step_id: &str) -> Result<bool> {
        self.step(step_id)?;
        let state = self
            .states
            .get_mut(step_id)
            .ok_or(WorkflowError::InvalidState)?;
        if state.status == StepStatus::Skipped {
            return Ok(false);
        }
        if self.cancelled
            || !matches!(
                state.status,
                StepStatus::Pending | StepStatus::AwaitingRetry | StepStatus::Blocked
            )
        {
            return Err(WorkflowError::InvalidTransition(step_id.into()));
        }
        state.status = StepStatus::Skipped;
        self.propagate_skips();
        Ok(true)
    }

    /// Explicitly cancel an uncompleted or restart-blocked step. Its successors
    /// become skipped while independent ready steps can continue. The host
    /// must separately obtain user consent for resolving a blocked attempt.
    pub fn cancel_step(&mut self, step_id: &str) -> Result<bool> {
        self.step(step_id)?;
        let state = self
            .states
            .get_mut(step_id)
            .ok_or(WorkflowError::InvalidState)?;
        if state.status == StepStatus::Cancelled {
            return Ok(false);
        }
        if self.cancelled
            || !matches!(
                state.status,
                StepStatus::Pending
                    | StepStatus::Running
                    | StepStatus::AwaitingRetry
                    | StepStatus::Blocked
            )
        {
            return Err(WorkflowError::InvalidTransition(step_id.into()));
        }
        state.status = StepStatus::Cancelled;
        self.propagate_skips();
        Ok(true)
    }

    /// Idempotently stop scheduling this run. Completed states are preserved.
    /// The caller must separately cancel any actual live provider/tool work.
    pub fn cancel(&mut self) -> bool {
        if self.cancelled {
            return false;
        }
        self.cancelled = true;
        for state in self.states.values_mut() {
            if matches!(
                state.status,
                StepStatus::Pending
                    | StepStatus::Running
                    | StepStatus::AwaitingRetry
                    | StepStatus::Blocked
            ) {
                state.status = StepStatus::Cancelled;
            }
        }
        true
    }

    pub fn finished(&self) -> bool {
        self.states.values().all(|state| {
            matches!(
                state.status,
                StepStatus::Succeeded
                    | StepStatus::Failed
                    | StepStatus::Skipped
                    | StepStatus::Cancelled
            )
        })
    }

    fn step(&self, step_id: &str) -> Result<&StepDefinition> {
        self.definition
            .steps
            .iter()
            .find(|step| step.id == step_id)
            .ok_or_else(|| WorkflowError::UnknownStep(step_id.into()))
    }

    fn propagate_skips(&mut self) {
        // Acyclic by construction: propagation is transitive and independent
        // of iteration order; 64 steps makes a bounded fixed point cheap.
        for _ in 0..self.definition.steps.len() {
            let mut changed = false;
            for step in &self.definition.steps {
                if self.states[&step.id].status == StepStatus::Pending
                    && step.depends_on.iter().any(|dependency| {
                        matches!(
                            self.states[dependency].status,
                            StepStatus::Failed | StepStatus::Skipped | StepStatus::Cancelled
                        )
                    })
                {
                    self.states.get_mut(&step.id).expect("validated id").status =
                        StepStatus::Skipped;
                    changed = true;
                }
            }
            if !changed {
                return;
            }
        }
    }

    fn validate_state(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(WorkflowError::UnsupportedVersion(self.schema_version));
        }
        self.definition.validate()?;
        if self.states.len() != self.definition.steps.len() {
            return Err(WorkflowError::InvalidState);
        }
        for step in &self.definition.steps {
            let state = self
                .states
                .get(&step.id)
                .ok_or(WorkflowError::InvalidState)?;
            if state.result_text.as_ref().is_some_and(|value| {
                state.status != StepStatus::Succeeded
                    || value.trim().is_empty()
                    || value.len() > MAX_RESULT_BYTES
                    || value
                        .chars()
                        .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
            }) {
                return Err(WorkflowError::InvalidState);
            }
            if state.attempts > step.max_attempts {
                return Err(WorkflowError::InvalidState);
            }
            let completion = state.last_completion.as_ref();
            if let Some(completion) = completion {
                if completion.attempt == 0 || completion.attempt > state.attempts {
                    return Err(WorkflowError::InvalidState);
                }
            }
            let latest = completion.is_some_and(|last| last.attempt == state.attempts);
            let failed_latest =
                latest && completion.is_some_and(|last| last.outcome == CompletionOutcome::Failed);
            let succeeded_latest = latest
                && completion.is_some_and(|last| last.outcome == CompletionOutcome::Succeeded);
            let clean = state.attempts == 0 && completion.is_none();
            let failed_previous = state.attempts > 1
                && completion.is_some_and(|last| {
                    last.attempt == state.attempts - 1 && last.outcome == CompletionOutcome::Failed
                });
            let running_history = (state.attempts == 1 && completion.is_none()) || failed_previous;
            match state.status {
                StepStatus::Pending if clean => {}
                StepStatus::Pending if failed_latest && state.attempts < step.max_attempts => {}
                StepStatus::Running | StepStatus::Blocked if running_history => {}
                StepStatus::AwaitingRetry
                    if failed_latest && state.attempts < step.max_attempts => {}
                StepStatus::Succeeded if succeeded_latest => {}
                StepStatus::Failed if failed_latest && state.attempts == step.max_attempts => {}
                StepStatus::Skipped
                    if clean
                        || running_history
                        || (failed_latest && state.attempts < step.max_attempts) => {}
                StepStatus::Cancelled
                    if clean
                        || running_history
                        || (failed_latest && state.attempts < step.max_attempts) => {}
                _ => return Err(WorkflowError::InvalidState),
            }
            if self.cancelled
                && matches!(
                    state.status,
                    StepStatus::Pending
                        | StepStatus::Running
                        | StepStatus::AwaitingRetry
                        | StepStatus::Blocked
                )
            {
                return Err(WorkflowError::InvalidState);
            }
            if matches!(
                state.status,
                StepStatus::Running
                    | StepStatus::Blocked
                    | StepStatus::AwaitingRetry
                    | StepStatus::Succeeded
                    | StepStatus::Failed
            ) && !step.depends_on.iter().all(|dependency| {
                self.states
                    .get(dependency)
                    .is_some_and(|dep| dep.status == StepStatus::Succeeded)
            }) {
                return Err(WorkflowError::InvalidState);
            }
            if state.status == StepStatus::Pending
                && step.depends_on.iter().any(|dependency| {
                    self.states.get(dependency).is_some_and(|dep| {
                        matches!(
                            dep.status,
                            StepStatus::Failed | StepStatus::Skipped | StepStatus::Cancelled
                        )
                    })
                })
            {
                return Err(WorkflowError::InvalidState);
            }
        }
        Ok(())
    }
}
