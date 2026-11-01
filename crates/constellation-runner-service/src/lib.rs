//! Local-only, Linux-capable authenticated runner inbox. Opt-in loopback TCP
//! is available, but NO remote/public listener, tool executor, shell, provider
//! or model integration exists.
#![forbid(unsafe_code)]

mod loopback;
pub use loopback::{
    authenticate_loopback_client, authenticate_loopback_client_bound, serve_loopback, PairIdentity,
};

use std::{
    fs::OpenOptions,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use constellation_runner_protocol::{
    Clock, Endpoint, Message, MessagePolicy, Pair, PermissionScope, ProtocolError, Secret, Side,
    TaskAttribution, MAX_TRACKED, MAX_WIRE_BYTES,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

const APPLICATION_ID: i64 = 0x4352_5336; // CRS6, owned runner database only.
const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("runner database or transport failure")]
    Io(#[from] std::io::Error),
    #[error("runner database transaction failed")]
    Sql(#[from] rusqlite::Error),
    #[error("authenticated runner protocol refused the request")]
    Protocol(#[from] ProtocolError),
    #[error("pair is pending, revoked, mismatched or not operator-approved")]
    Denied,
    #[error("unknown or incompatible runner database")]
    Schema,
    #[error("runner queue capacity or invalid job state")]
    Queue,
}

pub type Result<T> = std::result::Result<T, ServiceError>;

pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|time| i64::try_from(time.as_millis()).ok())
            .unwrap_or(-1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedJob {
    pub job_id: String,
    pub agent_id: String,
    pub scope: PermissionScope,
    pub task: Option<TaskAttribution>,
    pub state: JobState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobState {
    Pending,
    Granted,
    Cancelled,
}
impl JobState {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "pending" => Ok(Self::Pending),
            "granted" => Ok(Self::Granted),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ServiceError::Queue),
        }
    }
}

struct State {
    workstation: String,
    runner: String,
    epoch: u64,
    approval: i64,
    checkpoint: Option<Vec<u8>>,
    generation: u64,
}

pub struct RunnerStore {
    connection: Connection,
    key: Zeroizing<[u8; 32]>,
}

impl RunnerStore {
    /// Only an operator on the runner machine may create this fresh state.
    /// The key must be exchanged using an independently secure channel. This
    /// function does NOT prove that the remote device possesses the key.
    pub fn initialize(path: &Path, key: [u8; 32], workstation: &str, runner: &str) -> Result<Self> {
        let _pending_pair = Pair::enroll(workstation, runner, Secret::from_bytes(key)?)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        drop(file);
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_millis(500))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             BEGIN IMMEDIATE;
             CREATE TABLE runner_state (
               id INTEGER PRIMARY KEY CHECK(id=1),
               workstation TEXT NOT NULL, runner TEXT NOT NULL,
               key_hash BLOB NOT NULL, epoch INTEGER NOT NULL,
               approval INTEGER NOT NULL CHECK(approval IN (0,1,2)),
               generation INTEGER NOT NULL, checkpoint BLOB
             );
             CREATE TABLE inbox (
               job_id TEXT PRIMARY KEY, message_seq INTEGER NOT NULL UNIQUE,
               agent_id TEXT NOT NULL, capability TEXT NOT NULL,
               resource_id TEXT NOT NULL, instruction TEXT NOT NULL,
               owner_id TEXT, run_id TEXT, task_id TEXT,
               state TEXT NOT NULL CHECK(state IN ('pending','granted','cancelled'))
             );",
        )?;
        connection.execute(
            "INSERT INTO runner_state VALUES(1,?1,?2,?3,1,0,0,NULL)",
            params![workstation, runner, Sha256::digest(key).as_slice()],
        )?;
        connection.pragma_update(None, "application_id", APPLICATION_ID)?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        connection.execute_batch("COMMIT")?;
        Self::open(path, key)
    }

    pub fn open(path: &Path, key: [u8; 32]) -> Result<Self> {
        if key.iter().all(|b| *b == 0) {
            return Err(ServiceError::Denied);
        }
        // An attacker able to read/replace DB and key defeats this boundary.
        // On Unix, reject group/other-readable DBs before opening.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::symlink_metadata(path)?.file_type().is_symlink()
                || std::fs::metadata(path)?.permissions().mode() & 0o077 != 0
            {
                return Err(ServiceError::Denied);
            }
        }
        let mut connection = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_millis(500))?;
        let app: i64 = connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if app != APPLICATION_ID || !(1..=SCHEMA_VERSION).contains(&version) {
            return Err(ServiceError::Schema);
        }
        let db_hash: Vec<u8> =
            connection.query_row("SELECT key_hash FROM runner_state WHERE id=1", [], |row| {
                row.get(0)
            })?;
        if db_hash != Sha256::digest(key).as_slice() {
            return Err(ServiceError::Denied);
        }
        if version == 1 {
            // Existing approved databases keep their replay checkpoint, pair
            // epoch and grants. A second process rechecks under the write lock.
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let actual: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if actual == 1 {
                tx.execute_batch(
                    "ALTER TABLE inbox ADD COLUMN owner_id TEXT;
                     ALTER TABLE inbox ADD COLUMN run_id TEXT;
                     ALTER TABLE inbox ADD COLUMN task_id TEXT;",
                )?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            } else if actual != SCHEMA_VERSION {
                return Err(ServiceError::Schema);
            }
            tx.commit()?;
        }
        Ok(Self {
            connection,
            key: Zeroizing::new(key),
        })
    }

    fn state(conn: &Connection) -> Result<State> {
        let raw: (String, String, i64, i64, i64, Option<Vec<u8>>) = conn.query_row(
            "SELECT workstation, runner, epoch, approval, generation, checkpoint
             FROM runner_state WHERE id=1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        Ok(State {
            workstation: raw.0,
            runner: raw.1,
            epoch: u64::try_from(raw.2).map_err(|_| ServiceError::Schema)?,
            approval: raw.3,
            generation: u64::try_from(raw.4).map_err(|_| ServiceError::Schema)?,
            checkpoint: raw.5,
        })
    }

    pub fn approval_status(&self) -> Result<&'static str> {
        Ok(match Self::state(&self.connection)?.approval {
            0 => "pending",
            1 => "approved",
            2 => "revoked",
            _ => return Err(ServiceError::Schema),
        })
    }

    /// Return the identity pinned to this operator-approved key and epoch.
    /// This is called immediately before each mutual TCP challenge.
    pub fn approved_pair_identity(&self) -> Result<PairIdentity> {
        let state = Self::state(&self.connection)?;
        if state.approval != 1 {
            return Err(ServiceError::Denied);
        }
        Pair::resume_approved(
            &state.workstation,
            &state.runner,
            Secret::from_bytes(*self.key)?,
            state.epoch,
        )?;
        Ok(PairIdentity {
            workstation_id: state.workstation,
            runner_id: state.runner,
            key_epoch: state.epoch,
        })
    }

    /// Separate, explicit operator action. No remotely received Job can
    /// authorize this transition or grant its own scope.
    pub fn approve_pair(&mut self) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = Self::state(&tx)?;
        if state.approval != 0 || state.checkpoint.is_some() || state.generation != 0 {
            return Err(ServiceError::Denied);
        }
        let mut pair = Pair::enroll(
            &state.workstation,
            &state.runner,
            Secret::from_bytes(*self.key)?,
        )?;
        pair.approve()?;
        let endpoint = Endpoint::new(Side::Runner, pair, MessagePolicy::default())?;
        tx.execute(
            "UPDATE runner_state SET approval=1, checkpoint=?1 WHERE id=1 AND approval=0",
            params![endpoint.checkpoint()?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Irreversible for this database/key pair. All queued grants disappear.
    /// A later pairing must use a *fresh* DB and newly generated random key.
    pub fn revoke_pair(&mut self) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if Self::state(&tx)?.approval != 1 {
            return Err(ServiceError::Denied);
        }
        tx.execute(
            "UPDATE runner_state SET approval=2, checkpoint=NULL WHERE id=1",
            [],
        )?;
        tx.execute(
            "UPDATE inbox SET state='cancelled' WHERE state != 'cancelled'",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Apply exactly one authenticated message and durable inbox update.
    /// It returns a signed receipt *only after* committing inbox + replay
    /// counters + outgoing receipt generation in the same SQLite transaction.
    /// This method never calls or authorizes an OS tool.
    pub fn accept(&mut self, frame: &[u8], clock: &impl Clock) -> Result<Vec<u8>> {
        if frame.is_empty() || frame.len() > MAX_WIRE_BYTES {
            return Err(ServiceError::Protocol(ProtocolError::Oversized));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = Self::state(&tx)?;
        if state.approval != 1 {
            return Err(ServiceError::Denied);
        }
        let pair = Pair::resume_approved(
            &state.workstation,
            &state.runner,
            Secret::from_bytes(*self.key)?,
            state.epoch,
        )?;
        let checkpoint = state.checkpoint.ok_or(ServiceError::Schema)?;
        let mut endpoint = Endpoint::recover(
            Side::Runner,
            pair,
            MessagePolicy::default(),
            &checkpoint,
            state.generation,
        )?;
        let (outcome, message) = endpoint.receive_authorized(frame, clock, |_, _| true)?;
        let info = outcome.info();
        let receipt_for = match message {
            Message::Job { instruction } => {
                let count: i64 =
                    tx.query_row("SELECT COUNT(*) FROM inbox", [], |row| row.get(0))?;
                if count >= MAX_TRACKED as i64 {
                    return Err(ServiceError::Queue);
                }
                let sequence = i64::try_from(info.sequence).map_err(|_| ServiceError::Queue)?;
                let capability = format!("{:?}", info.permission.capability);
                tx.execute(
                    "INSERT INTO inbox
                       (job_id, message_seq, agent_id, capability, resource_id, instruction, owner_id, run_id, task_id, state)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending')",
                    params![
                        info.job_id,
                        sequence,
                        info.agent_id,
                        capability,
                        info.permission.resource_id,
                        instruction,
                        info.task.as_ref().map(|task| task.owner_id.as_str()),
                        info.task.as_ref().map(|task| task.run_id.as_str()),
                        info.task.as_ref().map(|task| task.task_id.as_str()),
                    ],
                )?;
                info.sequence
            }
            Message::Cancel { .. } => {
                let count = tx.execute(
                    "UPDATE inbox SET state='cancelled'
                     WHERE job_id=?1 AND state IN ('pending','granted')",
                    [&info.job_id],
                )?;
                if count != 1 {
                    return Err(ServiceError::Queue);
                }
                info.sequence
            }
            Message::ReconcileReceipt { for_sequence, .. } => {
                // The signed protocol verified the original canonical digest,
                // exact owner/run/task/scope and peer sequence in its durable
                // replay ledger. This branch never replays a Job/Cancel.
                // A new receipt gets a fresh sequence and expiry, including
                // when the original request and receipt have expired.
                for_sequence
            }
            _ => return Err(ServiceError::Denied),
        };
        let receipt = endpoint.issue_receipt(receipt_for, 30_000, clock)?;
        let generation =
            i64::try_from(endpoint.checkpoint_generation()).map_err(|_| ServiceError::Schema)?;
        tx.execute(
            "UPDATE runner_state SET checkpoint=?1, generation=?2
             WHERE id=1 AND approval=1",
            params![endpoint.checkpoint()?, generation],
        )?;
        tx.commit()?;
        Ok(receipt)
    }

    /// Inspect IDs and states only; instructions are not echoed into logs.
    pub fn list_jobs(&self) -> Result<Vec<QueuedJob>> {
        let mut stmt = self.connection.prepare(
            "SELECT job_id, agent_id, capability, resource_id, state, owner_id, run_id, task_id
             FROM inbox ORDER BY message_seq",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (job_id, agent_id, capability, resource_id, state, owner, run, task) = row?;
            let task = match (owner, run, task) {
                (None, None, None) => None,
                (Some(owner_id), Some(run_id), Some(task_id)) => Some(TaskAttribution {
                    owner_id,
                    run_id,
                    task_id,
                }),
                _ => return Err(ServiceError::Queue),
            };
            let capability = match capability.as_str() {
                "ReadEvidence" => constellation_runner_protocol::Capability::ReadEvidence,
                "DraftText" => constellation_runner_protocol::Capability::DraftText,
                "AnalyzeData" => constellation_runner_protocol::Capability::AnalyzeData,
                _ => return Err(ServiceError::Queue),
            };
            Ok(QueuedJob {
                job_id,
                agent_id,
                scope: PermissionScope {
                    capability,
                    resource_id,
                },
                task,
                state: JobState::parse(&state)?,
            })
        })
        .collect()
    }

    /// Local operator grant for one *exact* job/scope. No executor exists in
    /// this crate: a grant only makes it eligible for future audited runtime.
    pub fn grant_job(&mut self, job_id: &str, expected: &PermissionScope) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if Self::state(&tx)?.approval != 1 {
            return Err(ServiceError::Denied);
        }
        let changed = tx.execute(
            "UPDATE inbox SET state='granted'
             WHERE job_id=?1 AND capability=?2 AND resource_id=?3
               AND owner_id IS NULL AND run_id IS NULL AND task_id IS NULL
               AND state='pending'",
            params![
                job_id,
                format!("{:?}", expected.capability),
                expected.resource_id
            ],
        )?;
        if changed != 1 {
            return Err(ServiceError::Denied);
        }
        tx.commit()?;
        Ok(())
    }

    /// Local operator approves an exact, already authenticated owner/run/task
    /// and scope. A legacy `grant_job` cannot authorize this bound job.
    pub fn grant_bound_job(
        &mut self,
        job_id: &str,
        task: &TaskAttribution,
        expected: &PermissionScope,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if Self::state(&tx)?.approval != 1 {
            return Err(ServiceError::Denied);
        }
        let changed = tx.execute(
            "UPDATE inbox SET state='granted' WHERE job_id=?1 AND capability=?2
             AND resource_id=?3 AND owner_id=?4 AND run_id=?5 AND task_id=?6
             AND state='pending'",
            params![
                job_id,
                format!("{:?}", expected.capability),
                expected.resource_id,
                task.owner_id,
                task.run_id,
                task.task_id
            ],
        )?;
        if changed != 1 {
            return Err(ServiceError::Denied);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn revoke_grant(&mut self, job_id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE inbox SET state='pending' WHERE job_id=?1 AND state='granted'",
            [job_id],
        )?;
        if changed != 1 {
            return Err(ServiceError::Denied);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn authorized_for_future_executor(
        &self,
        job_id: &str,
        expected: &PermissionScope,
    ) -> Result<bool> {
        if Self::state(&self.connection)?.approval != 1 {
            return Ok(false);
        }
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM inbox WHERE job_id=?1 AND capability=?2
             AND resource_id=?3 AND state='granted'
             AND owner_id IS NULL AND run_id IS NULL AND task_id IS NULL",
                params![
                    job_id,
                    format!("{:?}", expected.capability),
                    expected.resource_id
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// An advisory read for a future executor. Actual execution still needs
    /// an independent transactional recheck, OS sandbox and local authority.
    pub fn authorized_bound_task(
        &self,
        job_id: &str,
        task: &TaskAttribution,
        expected: &PermissionScope,
    ) -> Result<bool> {
        if Self::state(&self.connection)?.approval != 1 {
            return Ok(false);
        }
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM inbox WHERE job_id=?1 AND capability=?2
                 AND resource_id=?3 AND owner_id=?4 AND run_id=?5 AND task_id=?6
                 AND state='granted'",
                params![
                    job_id,
                    format!("{:?}", expected.capability),
                    expected.resource_id,
                    task.owner_id,
                    task.run_id,
                    task.task_id
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
