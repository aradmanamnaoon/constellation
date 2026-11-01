//! Durable SQLite snapshots. Loading a persisted Running attempt produces a
//! Blocked attempt, requiring explicit reconciliation by the trusted host.

use std::{
    fs,
    path::{Component, Path},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    CompletionOutcome, StepDispatch, StepProgress, StepStatus, WorkflowError, WorkflowRun,
    MAX_JSON_BYTES, SCHEMA_VERSION,
};

const DB_VERSION: u32 = 1;
const MAX_PAGE: usize = 100;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("invalid durable SQLite path")]
    InvalidPath,
    #[error("unsupported repository schema version {0}")]
    UnsupportedSchema(u32),
    #[error("invalid pagination")]
    InvalidPage,
    #[error("workflow JSON exceeds limit")]
    InputTooLarge,
    #[error("stored workflow is malformed or corrupted")]
    CorruptRecord,
    #[error("workflow already exists")]
    AlreadyExists,
    #[error("workflow not found")]
    NotFound,
    #[error("workflow revision conflict")]
    RevisionConflict,
    #[error("invalid persisted workflow state transition")]
    InvalidTransition,
    #[error("revision exhausted")]
    RevisionOverflow,
    #[error("SQLite mutex poisoned")]
    LockPoisoned,
    #[error(transparent)]
    Workflow(#[from] WorkflowError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type RepositoryResult<T> = std::result::Result<T, RepositoryError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredWorkflow {
    pub run: WorkflowRun,
    pub revision: u64,
}

/// An approved scheduling *proposal* and its durable running snapshot.
/// Only dispatch after this value has been returned by a committed transaction.
/// Provider credentials and tool approvals remain the trusted host's concern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedStep {
    pub stored: StoredWorkflow,
    pub dispatch: StepDispatch,
}

/// Owns one SQLite connection guarded for cross-thread use. Independent
/// instances coordinate through database transactions and revisions.
pub struct Repository {
    connection: Mutex<Connection>,
}

impl Repository {
    /// Opens a file-backed database in WAL/FULL mode. There is no in-memory
    /// fallback and unknown schema versions fail closed.
    pub fn open(path: impl AsRef<Path>) -> RepositoryResult<Self> {
        let path = path.as_ref();
        if !path.is_absolute()
            || path.file_name().is_none()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(RepositoryError::InvalidPath);
        }
        let parent = path.parent().ok_or(RepositoryError::InvalidPath)?;
        fs::create_dir_all(parent)?;
        let path =
            fs::canonicalize(parent)?.join(path.file_name().ok_or(RepositoryError::InvalidPath)?);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(RepositoryError::InvalidPath);
            }
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "trusted_schema", "OFF")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        let sync: i64 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") || sync != 2 {
            return Err(RepositoryError::CorruptRecord);
        }
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != 0 && version != DB_VERSION {
            return Err(RepositoryError::UnsupportedSchema(version));
        }
        if version == 0 {
            // A foreign SQLite database with no version must not be silently
            // adopted as a workflow repository.
            let existing_tables: i64 = tx.query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )?;
            if existing_tables != 0 {
                return Err(RepositoryError::CorruptRecord);
            }
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS workflow_runs (
                id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
                revision INTEGER NOT NULL CHECK(revision > 0),
                schema_version INTEGER NOT NULL CHECK(schema_version = 1),
                payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 524288),
                sha256 TEXT NOT NULL CHECK(length(sha256) = 64)
            ) WITHOUT ROWID;",
        )?;
        if version == 0 {
            tx.pragma_update(None, "user_version", DB_VERSION)?;
        }
        tx.commit()?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn new(path: impl AsRef<Path>) -> RepositoryResult<Self> {
        Self::open(path)
    }

    pub fn create(&self, run: &WorkflowRun) -> RepositoryResult<StoredWorkflow> {
        let payload = encode(run)?;
        let hash = digest(&payload);
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "INSERT INTO workflow_runs (id, revision, schema_version, payload, sha256)
             VALUES (?1, 1, ?2, ?3, ?4)
             ON CONFLICT(id) DO NOTHING",
            params![run.definition().workflow_id, SCHEMA_VERSION, payload, hash],
        )?;
        if changed != 1 {
            return Err(RepositoryError::AlreadyExists);
        }
        tx.commit()?;
        Ok(StoredWorkflow {
            run: run.clone(),
            revision: 1,
        })
    }

    pub fn get(&self, workflow_id: &str) -> RepositoryResult<Option<StoredWorkflow>> {
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let result = load(&tx, workflow_id)?;
        tx.commit()?;
        Ok(result)
    }

    /// Stable workflow-ID order; pagination bounds limit memory and scan cost.
    /// Any corrupt row fails the page instead of being silently dropped.
    pub fn list(&self, limit: usize, offset: usize) -> RepositoryResult<Vec<StoredWorkflow>> {
        if limit == 0 || limit > MAX_PAGE {
            return Err(RepositoryError::InvalidPage);
        }
        let limit = i64::try_from(limit).map_err(|_| RepositoryError::InvalidPage)?;
        let offset = i64::try_from(offset).map_err(|_| RepositoryError::InvalidPage)?;
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let ids = {
            let mut statement =
                tx.prepare("SELECT id FROM workflow_runs ORDER BY id LIMIT ?1 OFFSET ?2")?;
            let rows =
                statement.query_map(params![limit, offset], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut results = Vec::with_capacity(ids.len());
        for id in ids {
            results.push(load(&tx, &id)?.ok_or(RepositoryError::CorruptRecord)?);
        }
        tx.commit()?;
        Ok(results)
    }

    /// Compare-and-swap the entire validated run in one IMMEDIATE transaction.
    /// The stored definition is immutable across revisions.
    pub fn update(
        &self,
        expected_revision: u64,
        run: &WorkflowRun,
    ) -> RepositoryResult<StoredWorkflow> {
        revision_for_update(expected_revision)?;
        encode(run)?;
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior = load(&tx, &run.definition().workflow_id)?.ok_or(RepositoryError::NotFound)?;
        if prior.revision != expected_revision {
            return Err(RepositoryError::RevisionConflict);
        }
        let updated = save_successor(&tx, &prior, run)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Atomically select the smallest ready step ID, increment its attempt and
    /// persist Running before returning any dispatch proposal. Competing
    /// connections see the new revision; a stale expected revision fails closed.
    /// A completed/blocked workflow returns None without creating a revision.
    pub fn claim_next_ready(
        &self,
        workflow_id: &str,
        expected_revision: u64,
    ) -> RepositoryResult<Option<ClaimedStep>> {
        self.claim(workflow_id, None, expected_revision)
    }

    /// Same transactional guarantee, with explicit ready step selection.
    pub fn claim_ready(
        &self,
        workflow_id: &str,
        step_id: &str,
        expected_revision: u64,
    ) -> RepositoryResult<ClaimedStep> {
        self.claim(workflow_id, Some(step_id), expected_revision)?
            .ok_or_else(|| RepositoryError::Workflow(WorkflowError::NotReady(step_id.into())))
    }

    fn claim(
        &self,
        workflow_id: &str,
        requested_step: Option<&str>,
        expected_revision: u64,
    ) -> RepositoryResult<Option<ClaimedStep>> {
        revision_for_update(expected_revision)?;
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior = load(&tx, workflow_id)?.ok_or(RepositoryError::NotFound)?;
        if prior.revision != expected_revision {
            return Err(RepositoryError::RevisionConflict);
        }
        let next = match requested_step {
            Some(step_id) => step_id.to_owned(),
            None => match prior.run.ready_steps().into_iter().next() {
                Some(step_id) => step_id,
                None => return Ok(None),
            },
        };
        let mut run = prior.run.clone();
        let dispatch = run.start(&next)?;
        let stored = save_successor(&tx, &prior, &run)?;
        tx.commit()?;
        Ok(Some(ClaimedStep { stored, dispatch }))
    }

    /// The host must retain the live StoredWorkflow from the start/update
    /// operation. Loading after restart blocks unfinished attempts.
    /// Exact duplicate callbacks return committed state without a new write.
    pub fn complete(
        &self,
        stored: &StoredWorkflow,
        step_id: &str,
        attempt: u8,
        outcome: CompletionOutcome,
    ) -> RepositoryResult<StoredWorkflow> {
        self.complete_recorded(stored, step_id, attempt, outcome, None, false)
    }

    /// Store a bounded successful result and its exact attempt outcome in the
    /// same durable transaction, including when unrelated branches have moved
    /// on. On duplicate callback, a different result is a revision conflict.
    pub fn complete_with_result(
        &self,
        stored: &StoredWorkflow,
        step_id: &str,
        attempt: u8,
        outcome: CompletionOutcome,
        result_text: Option<String>,
    ) -> RepositoryResult<StoredWorkflow> {
        self.complete_recorded(stored, step_id, attempt, outcome, result_text, true)
    }

    fn complete_recorded(
        &self,
        stored: &StoredWorkflow,
        step_id: &str,
        attempt: u8,
        outcome: CompletionOutcome,
        result_text: Option<String>,
        check_result: bool,
    ) -> RepositoryResult<StoredWorkflow> {
        // Validate the callback against its retained live snapshot. Loading a
        // snapshot after restart turns Running into Blocked and is ineligible.
        let mut proposed = stored.run.clone();
        proposed.complete_with_result(step_id, attempt, outcome, result_text.clone())?;
        let mut connection = self.lock()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            load(&tx, &stored.run.definition().workflow_id)?.ok_or(RepositoryError::NotFound)?;
        if stored.revision > current.revision || current.run.definition() != stored.run.definition()
        {
            return Err(RepositoryError::RevisionConflict);
        }
        // Identical callback reports must not create another revision, even
        // if an independent step has advanced in the meantime.
        if current.run.progress(step_id).is_some_and(|progress| {
            progress.attempts == attempt
                && progress.last_completion.as_ref().is_some_and(|completion| {
                    completion.attempt == attempt && completion.outcome == outcome
                })
                && (!check_result || progress.result_text == result_text)
        }) {
            tx.commit()?;
            return Ok(current);
        }

        // Rebase a valid live completion onto any *unrelated* progress that
        // committed since the dispatch. The targeted attempt must still be
        // in flight, with identical attempt counter and completion history.
        // A concurrent cancellation, retry or different completion wins.
        let original = stored
            .run
            .progress(step_id)
            .ok_or_else(|| WorkflowError::UnknownStep(step_id.into()))?;
        let latest = current
            .run
            .progress(step_id)
            .ok_or(RepositoryError::CorruptRecord)?;
        if original.status != StepStatus::Running
            || latest.status != StepStatus::Blocked
            || original.attempts != latest.attempts
            || original.last_completion != latest.last_completion
            || original.result_text != latest.result_text
            || current.run.cancelled()
        {
            return Err(RepositoryError::RevisionConflict);
        }
        let mut merged = current.run.clone();
        merged
            .states
            .get_mut(step_id)
            .ok_or(RepositoryError::CorruptRecord)?
            .status = StepStatus::Running;
        merged.complete_with_result(step_id, attempt, outcome, result_text)?;
        let committed = save_successor(&tx, &current, &merged)?;
        tx.commit()?;
        Ok(committed)
    }

    fn lock(&self) -> RepositoryResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)
    }
}

fn revision_for_update(revision: u64) -> RepositoryResult<i64> {
    i64::try_from(revision)
        .ok()
        .filter(|value| *value > 0 && *value < i64::MAX)
        .ok_or(RepositoryError::RevisionOverflow)
}

/// Commit a single legal successor under an existing IMMEDIATE transaction.
/// This is shared by user-issued updates, durable dispatch and rebased results.
fn save_successor(
    tx: &Transaction<'_>,
    before: &StoredWorkflow,
    after: &WorkflowRun,
) -> RepositoryResult<StoredWorkflow> {
    let revision = revision_for_update(before.revision)?;
    if before.run.definition() != after.definition() {
        return Err(RepositoryError::CorruptRecord);
    }
    if !valid_transition(&before.run, after) {
        return Err(RepositoryError::InvalidTransition);
    }
    let payload = encode(after)?;
    let affected = tx.execute(
        "UPDATE workflow_runs SET revision = revision + 1, payload = ?1, sha256 = ?2
         WHERE id = ?3 AND revision = ?4",
        params![
            payload,
            digest(&payload),
            after.definition().workflow_id,
            revision
        ],
    )?;
    if affected != 1 {
        return Err(RepositoryError::RevisionConflict);
    }
    Ok(StoredWorkflow {
        run: after.clone(),
        revision: before.revision + 1,
    })
}

fn encode(run: &WorkflowRun) -> RepositoryResult<Vec<u8>> {
    let payload = run.to_json()?;
    if payload.is_empty() || payload.len() > MAX_JSON_BYTES {
        return Err(RepositoryError::InputTooLarge);
    }
    Ok(payload)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Compare persisted state against its proposed successor. The persisted
/// Running state is read as Blocked; only a *previously persisted* in-flight
/// attempt can be completed. Pending -> Succeeded without a durable start and
/// terminal -> Pending resets are both rejected.
fn valid_transition(before: &WorkflowRun, after: &WorkflowRun) -> bool {
    if before.cancelled() && !after.cancelled() {
        return false;
    }
    before.states().iter().all(|(id, previous)| {
        let Some(next) = after.progress(id) else {
            return false;
        };
        valid_step_transition(previous, next)
    })
}

fn valid_step_transition(before: &StepProgress, after: &StepProgress) -> bool {
    let same_count = before.attempts == after.attempts;
    let same_history = before.last_completion == after.last_completion;
    let same_result = before.result_text == after.result_text;
    let completed_this_attempt = same_count
        && after.last_completion.as_ref().is_some_and(|last| {
            last.attempt == after.attempts && before.last_completion.as_ref() != Some(last)
        });
    match before.status {
        StepStatus::Pending => {
            (same_count
                && same_history
                && same_result
                && matches!(
                    after.status,
                    StepStatus::Pending | StepStatus::Skipped | StepStatus::Cancelled
                ))
                || (after.attempts == before.attempts.saturating_add(1)
                    && after.attempts > before.attempts
                    && same_history
                    && same_result
                    && after.status == StepStatus::Running)
        }
        StepStatus::Blocked => {
            same_count
                && ((same_history
                    && same_result
                    && matches!(
                        after.status,
                        StepStatus::Blocked | StepStatus::Skipped | StepStatus::Cancelled
                    ))
                    || (completed_this_attempt
                        && (after.status == StepStatus::Succeeded || after.result_text.is_none())
                        && matches!(
                            after.status,
                            StepStatus::Succeeded | StepStatus::Failed | StepStatus::AwaitingRetry
                        )))
        }
        StepStatus::AwaitingRetry => {
            same_count
                && same_history
                && same_result
                && matches!(
                    after.status,
                    StepStatus::AwaitingRetry
                        | StepStatus::Pending
                        | StepStatus::Skipped
                        | StepStatus::Cancelled
                )
        }
        StepStatus::Succeeded
        | StepStatus::Failed
        | StepStatus::Skipped
        | StepStatus::Cancelled => {
            same_count && same_history && same_result && before.status == after.status
        }
        // Loading the prior record always turns Running into Blocked.
        StepStatus::Running => false,
    }
}

fn load(tx: &Transaction<'_>, id: &str) -> RepositoryResult<Option<StoredWorkflow>> {
    // Check database length before allocating potentially hostile BLOB bytes.
    let metadata = tx
        .query_row(
            "SELECT revision, schema_version, length(payload), sha256
             FROM workflow_runs WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((revision, schema, length, expected_hash)) = metadata else {
        return Ok(None);
    };
    if revision < 1
        || schema != i64::from(SCHEMA_VERSION)
        || length < 1
        || length as u64 > MAX_JSON_BYTES as u64
        || expected_hash.len() != 64
        || !expected_hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(RepositoryError::CorruptRecord);
    }
    let payload: Vec<u8> = tx.query_row(
        "SELECT payload FROM workflow_runs WHERE id = ?1",
        [id],
        |row| row.get(0),
    )?;
    if payload.len() as i64 != length || digest(&payload) != expected_hash {
        return Err(RepositoryError::CorruptRecord);
    }
    let run =
        WorkflowRun::from_json_bounded(&payload).map_err(|_| RepositoryError::CorruptRecord)?;
    if run.definition().workflow_id != id {
        return Err(RepositoryError::CorruptRecord);
    }
    Ok(Some(StoredWorkflow {
        run,
        revision: revision as u64,
    }))
}
