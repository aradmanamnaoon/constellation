//! Private, native-only persistent evidence for MultiStepRun. NOT an approval,
//! provider dispatch ticket, or credential. A separate journal and explicit
//! human consent are mandatory before any action in a future prime-owned host.
//!
//! The monotonic OS-vault head counts *writes*, NOT agent-loop revisions:
//! reserving a provider increments the reservation number with no change to
//! the last settled revision. Recovery from any unsettled phase is terminally
//! quarantined for this session; the host must not retry uncertain spend.
#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use constellation_agent_loop::{
    AgentTurn, HostTurnCheckpoint, MultiStepBudget, MultiStepCheckpoint, MultiStepPhase,
    MultiStepRun, TurnStatus,
};
use constellation_core::{ProviderProfile, StudioStore, TaskId, TaskState};
use constellation_tools::OperationContext;
use ring::hmac;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const SERVICE: &str = "org.constellation.desktop.native-multi-step-checkpoint.v1";
const DOMAIN: &[u8] = b"constellation.native-multi-step-checkpoint.v1\0";
const MAX_PAYLOAD: usize = 2048;
const MAX_SEQUENCE: u64 = 128;
const SCHEMA: &str = "CREATE TABLE multi_step_checkpoints (
    task_id TEXT PRIMARY KEY NOT NULL,
    room_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    provider_profile_sha256 TEXT NOT NULL CHECK(length(provider_profile_sha256) = 64),
    task_context_sha256 TEXT NOT NULL CHECK(length(task_context_sha256) = 64),
    sequence INTEGER NOT NULL CHECK(sequence >= 0 AND sequence <= 128),
    revision INTEGER NOT NULL CHECK(revision >= 0 AND revision <= 64),
    reservations INTEGER NOT NULL CHECK(reservations >= 0 AND reservations <= 16),
    phase TEXT NOT NULL CHECK(phase IN ('settled','provider_in_flight','native_review_required')),
    payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 2048),
    mac BLOB NOT NULL CHECK(length(mac) = 32)
);";

fn denied() -> String {
    "Native multi-step checkpoint cannot be authenticated; operation quarantined".into()
}

/// The OS vault is not readable by model/renderer/provider. A fake anchor is
/// injected only by isolated tests. Each task uses its own independent head.
pub(crate) trait MultiStepAnchor: Send + Sync {
    fn read(&self, task: TaskId) -> Result<Option<String>, String>;
    fn pin(&self, task: TaskId, value: &str) -> Result<(), String>;
}

pub(crate) struct OsMultiStepAnchor {
    project: String,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl MultiStepAnchor for OsMultiStepAnchor {
    fn read(&self, task: TaskId) -> Result<Option<String>, String> {
        let account = format!("{}-{task}", self.project);
        let entry = keyring::Entry::new(SERVICE, &account).map_err(|_| denied())?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(denied()),
        }
    }

    fn pin(&self, task: TaskId, value: &str) -> Result<(), String> {
        let account = format!("{}-{task}", self.project);
        let entry = keyring::Entry::new(SERVICE, &account).map_err(|_| denied())?;
        entry.set_password(value).map_err(|_| denied())?;
        if entry.get_password().map_err(|_| denied())?.as_str() != value {
            return Err(denied());
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl MultiStepAnchor for OsMultiStepAnchor {
    fn read(&self, _task: TaskId) -> Result<Option<String>, String> {
        Err(denied())
    }
    fn pin(&self, _task: TaskId, _value: &str) -> Result<(), String> {
        Err(denied())
    }
}

fn canonical_identity(path: &Path) -> Result<(PathBuf, String), String> {
    if !path.is_absolute() || !path.parent().is_some_and(Path::is_dir) {
        return Err(denied());
    }
    let parent = std::fs::canonicalize(path.parent().ok_or_else(denied)?).map_err(|_| denied())?;
    if std::fs::symlink_metadata(path)
        .is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(denied());
    }
    let canonical = parent.join(path.file_name().ok_or_else(denied)?);
    let id = format!(
        "{:x}",
        Sha256::digest(canonical.as_os_str().to_string_lossy().as_bytes())
    );
    Ok((canonical, id))
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(denied());
    }
    let mut key = [0_u8; 32];
    for (i, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[i] = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| denied())?, 16)
            .map_err(|_| denied())?;
    }
    if key.iter().all(|b| *b == 0) {
        return Err(denied());
    }
    Ok(key)
}

/// Quarantine has no `MultiStepRun`, ticket, operation ID or tool authority.
/// A trusted native user must inspect the separate journal/task before any
/// NEW session; the old in-flight provider/approval is never replayed.
pub(crate) enum NativeMultiStepRecovery {
    Settled {
        sequence: u64,
        run: Box<MultiStepRun>,
    },
    Quarantined {
        sequence: u64,
        revision: u64,
        reservations: u8,
        phase: MultiStepPhase,
    },
}

pub(crate) struct NativeMultiStepCheckpointStore<A: MultiStepAnchor = OsMultiStepAnchor> {
    database: PathBuf,
    key: Zeroizing<[u8; 32]>,
    anchor: A,
}

impl NativeMultiStepCheckpointStore<OsMultiStepAnchor> {
    /// Never accepts a renderer path. Prime must derive a fixed path in its
    /// canonical private audit folder, separately from old agent-turn vault.
    /// A lost existing vault key never causes automatic rekey or adoption.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(crate) fn open_os_vault(path: &Path) -> Result<Self, String> {
        let (canonical, project) = canonical_identity(path)?;
        let account = format!("{project}-key");
        let entry = keyring::Entry::new(SERVICE, &account).map_err(|_| denied())?;
        let value = Zeroizing::new(match entry.get_password() {
            Ok(secret) if canonical.exists() => secret,
            Ok(_) => return Err(denied()), // missing DB with preexisting key: deletion/crash
            Err(keyring::Error::NoEntry) if !canonical.exists() => {
                let mut random = Zeroizing::new([0_u8; 32]);
                SystemRandom::new()
                    .fill(random.as_mut())
                    .map_err(|_| denied())?;
                let encoded = random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                entry.set_password(&encoded).map_err(|_| denied())?;
                if entry.get_password().map_err(|_| denied())? != encoded {
                    return Err(denied());
                }
                encoded
            }
            Err(_) => return Err(denied()),
        });
        Self::with_anchor(
            &canonical,
            decode_key(&value)?,
            OsMultiStepAnchor { project },
        )
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(crate) fn open_os_vault(_path: &Path) -> Result<Self, String> {
        Err(denied())
    }
}

impl<A: MultiStepAnchor> NativeMultiStepCheckpointStore<A> {
    pub(crate) fn with_anchor(path: &Path, key: [u8; 32], anchor: A) -> Result<Self, String> {
        if key.iter().all(|b| *b == 0) {
            return Err(denied());
        }
        let (database, _) = canonical_identity(path)?;
        Ok(Self {
            database,
            key: Zeroizing::new(key),
            anchor,
        })
    }

    fn require_scope(
        core: &StudioStore,
        context: OperationContext,
        provider: &ProviderProfile,
    ) -> Result<(String, String), String> {
        provider.validate().map_err(|_| denied())?;
        let snapshot = core.snapshot().map_err(|_| denied())?;
        let task = snapshot
            .tasks
            .iter()
            .find(|t| t.id == context.task_id)
            .filter(|t| {
                t.room_id == context.room_id
                    && t.agent_id == context.agent_id
                    && t.state == TaskState::Pending
            })
            .ok_or_else(denied)?;
        let agent = snapshot
            .agents
            .iter()
            .find(|a| a.id == context.agent_id)
            .ok_or_else(denied)?;
        if !snapshot.providers.iter().any(|p| p == provider) {
            return Err(denied());
        }
        // Bind immutable task/role, even if future Core APIs add in-place
        // editors. Avoid persisting those potentially private strings.
        let task_bytes = serde_json::to_vec(&(
            "constellation.native-multi-step-task.v1",
            task.id,
            task.room_id,
            task.agent_id,
            &task.instructions,
            &agent.role,
        ))
        .map_err(|_| denied())?;
        let provider_bytes = serde_json::to_vec(&(
            "constellation.native-multi-step-provider.v1",
            provider.id,
            provider.kind,
            &provider.model_id,
            &provider.base_url,
            &provider.credentials,
        ))
        .map_err(|_| denied())?;
        Ok((
            format!("{:x}", Sha256::digest(provider_bytes)),
            format!("{:x}", Sha256::digest(task_bytes)),
        ))
    }

    fn open_database(&self, create: bool) -> Result<Connection, String> {
        if std::fs::symlink_metadata(&self.database)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(denied());
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | if create {
                OpenFlags::SQLITE_OPEN_CREATE
            } else {
                OpenFlags::empty()
            };
        let conn = Connection::open_with_flags(&self.database, flags).map_err(|_| denied())?;
        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|_| denied())?;
        Ok(conn)
    }

    fn layout(conn: &Connection) -> Result<bool, String> {
        let mut stmt = conn.prepare(
            "SELECT name,type,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name"
        ).map_err(|_| denied())?;
        let objects: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|_| denied())?
            .collect::<Result<_, _>>()
            .map_err(|_| denied())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| denied())?;
        if objects.is_empty() && version == 0 {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
                .map_err(|_| denied())?;
            return if total == 0 { Ok(true) } else { Err(denied()) };
        }
        let reference = Connection::open_in_memory().map_err(|_| denied())?;
        reference.execute_batch(SCHEMA).map_err(|_| denied())?;
        let expected: String = reference
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='multi_step_checkpoints'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| denied())?;
        if version != 1
            || objects != vec![("multi_step_checkpoints".into(), "table".into(), expected)]
        {
            return Err(denied());
        }
        Ok(false)
    }

    fn tag(
        &self,
        context: OperationContext,
        provider: &ProviderProfile,
        route: &str,
        task_hash: &str,
        sequence: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let bytes = Self::tag_data(context, provider, route, task_hash, sequence, payload);
        hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, self.key.as_ref()),
            &bytes,
        )
        .as_ref()
        .to_vec()
    }

    fn pin_value(sequence: u64, mac: &[u8]) -> String {
        format!(
            "{sequence}:{}",
            mac.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }

    fn verify_pin(value: &str, sequence: u64, mac: &[u8]) -> Result<(), String> {
        if value != Self::pin_value(sequence, mac) {
            return Err(denied());
        }
        Ok(())
    }

    fn phase_label(phase: MultiStepPhase) -> &'static str {
        match phase {
            MultiStepPhase::Settled => "settled",
            MultiStepPhase::ProviderInFlight => "provider_in_flight",
            MultiStepPhase::NativeReviewRequired => "native_review_required",
        }
    }

    /// Parse the *untrusted stored bytes*, including quarantined states.
    /// A quarantined checkpoint embeds only the LAST settled AgentTurn, and
    /// deliberately cannot be promoted back into an executable run.
    fn validate(context: OperationContext, payload: &[u8]) -> Result<MultiStepCheckpoint, String> {
        if payload.is_empty() || payload.len() > MAX_PAYLOAD {
            return Err(denied());
        }
        let cp: MultiStepCheckpoint = serde_json::from_slice(payload).map_err(|_| denied())?;
        if cp.to_bytes().map_err(|_| denied())? != payload {
            return Err(denied());
        }
        let obj: Value = serde_json::from_slice(payload).map_err(|_| denied())?;
        let budget: MultiStepBudget =
            serde_json::from_value(obj.get("budget").cloned().ok_or_else(denied)?)
                .map_err(|_| denied())?;
        MultiStepBudget::new(budget.model_attempts, budget.tool_steps).map_err(|_| denied())?;
        let prior: HostTurnCheckpoint =
            serde_json::from_value(obj.get("settled").cloned().ok_or_else(denied)?)
                .map_err(|_| denied())?;
        let turn =
            AgentTurn::restore_from_host(context, prior, cp.revision()).map_err(|_| denied())?;
        if cp.revision() > 64
            || cp.provider_reservations() > budget.model_attempts
            || turn.model_attempts() > budget.model_attempts
            || turn.tool_steps() > budget.tool_steps
        {
            return Err(denied());
        }
        match cp.phase() {
            MultiStepPhase::Settled => {
                MultiStepRun::restore_from_host(context, payload, cp.revision())
                    .map_err(|_| denied())?;
            }
            MultiStepPhase::ProviderInFlight | MultiStepPhase::NativeReviewRequired => {
                if turn.status() != TurnStatus::AwaitingModel
                    || cp.provider_reservations() != turn.model_attempts().saturating_add(1)
                {
                    return Err(denied());
                }
            }
        }
        Ok(cp)
    }

    fn read_verified_row(
        &self,
        conn: &Connection,
        context: OperationContext,
        provider: &ProviderProfile,
        route: &str,
        task_hash: &str,
    ) -> Result<Option<(u64, MultiStepCheckpoint, Vec<u8>)>, String> {
        type Row = (
            String,
            String,
            String,
            String,
            String,
            i64,
            i64,
            i64,
            String,
            Vec<u8>,
            Vec<u8>,
        );
        let row: Option<Row> = conn
            .query_row(
                "SELECT room_id,agent_id,provider_id,provider_profile_sha256,task_context_sha256,
                    sequence,revision,reservations,phase,payload,mac
               FROM multi_step_checkpoints WHERE task_id=?1",
                [context.task_id.to_string()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| denied())?;
        let head = self.anchor.read(context.task_id)?;
        match (row, head) {
            (None, None) => Ok(None),
            (
                Some((
                    room,
                    agent,
                    provider_id,
                    profile_hash,
                    task_hash_row,
                    seq,
                    rev,
                    res,
                    phase,
                    payload,
                    mac,
                )),
                Some(saved),
            ) => {
                if room != context.room_id.to_string()
                    || agent != context.agent_id.to_string()
                    || provider_id != provider.id.to_string()
                    || profile_hash != route
                    || task_hash_row != task_hash
                    || !(0..=MAX_SEQUENCE as i64).contains(&seq)
                    || !(0..=64).contains(&rev)
                    || !(0..=16).contains(&res)
                {
                    return Err(denied());
                }
                let sequence = seq as u64;
                Self::verify_pin(&saved, sequence, &mac)?;
                // Constant-time compare via ring's HMAC verification.
                hmac::verify(
                    &hmac::Key::new(hmac::HMAC_SHA256, self.key.as_ref()),
                    &Self::tag_data(context, provider, route, task_hash, sequence, &payload),
                    &mac,
                )
                .map_err(|_| denied())?;
                let cp = Self::validate(context, &payload)?;
                if cp.revision() != rev as u64
                    || cp.provider_reservations() != res as u8
                    || Self::phase_label(cp.phase()) != phase
                {
                    return Err(denied());
                }
                Ok(Some((sequence, cp, mac)))
            }
            _ => Err(denied()), // never adopt a missing/rollback vault head
        }
    }

    fn tag_data(
        context: OperationContext,
        provider: &ProviderProfile,
        route: &str,
        task_hash: &str,
        sequence: u64,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut bytes = DOMAIN.to_vec();
        for field in [
            context.task_id.to_string(),
            context.room_id.to_string(),
            context.agent_id.to_string(),
            provider.id.to_string(),
            route.to_owned(),
            task_hash.to_owned(),
            sequence.to_string(),
        ] {
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        bytes.extend_from_slice(payload);
        bytes
    }

    /// The *native task transition lock* must be held by the future prime
    /// caller. No provider polling is allowed until a reservation checkpoint
    /// was pinned in the vault AND SQLite commit returned success.
    pub(crate) fn persist_pending_task(
        &self,
        core: &StudioStore,
        context: OperationContext,
        provider: &ProviderProfile,
        checkpoint: &MultiStepCheckpoint,
        previous_sequence: Option<u64>,
    ) -> Result<u64, String> {
        let (route, task_hash) = Self::require_scope(core, context, provider)?;
        let payload = checkpoint.to_bytes().map_err(|_| denied())?;
        let cp = Self::validate(context, &payload)?;
        let existed_before_open = self.database.exists();
        let mut conn = self.open_database(true)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| denied())?;
        if Self::layout(&tx)? {
            if existed_before_open {
                return Err(denied());
            }
            tx.execute_batch(SCHEMA).map_err(|_| denied())?;
            tx.pragma_update(None, "user_version", 1)
                .map_err(|_| denied())?;
        }
        let prior = self.read_verified_row(&tx, context, provider, &route, &task_hash)?;
        let sequence = match (prior, previous_sequence) {
            (None, None)
                if cp.phase() == MultiStepPhase::Settled
                    && cp.revision() == 0
                    && cp.provider_reservations() == 0 =>
            {
                0
            }
            (Some((last, old, _)), Some(expected))
                if last == expected
                    && last < MAX_SEQUENCE
                    && cp.revision() >= old.revision()
                    && cp.provider_reservations() >= old.provider_reservations()
                    && Self::valid_transition(&old, &cp) =>
            {
                last + 1
            }
            _ => return Err(denied()),
        };
        let tag = self.tag(context, provider, &route, &task_hash, sequence, &payload);
        // Durable OS head moves FIRST. DB commit/fsync failure then leaves a
        // mismatched head, deliberately quarantining that task, never refunding
        // the reservation or promoting a stale signed prefix.
        self.anchor
            .pin(context.task_id, &Self::pin_value(sequence, &tag))?;
        if self.anchor.read(context.task_id)?.as_deref()
            != Some(Self::pin_value(sequence, &tag).as_str())
        {
            return Err(denied());
        }
        tx.execute(
            "INSERT INTO multi_step_checkpoints(task_id,room_id,agent_id,provider_id,
                    provider_profile_sha256,task_context_sha256,sequence,revision,
                    reservations,phase,payload,mac)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(task_id) DO UPDATE SET
               room_id=excluded.room_id, agent_id=excluded.agent_id,
               provider_id=excluded.provider_id,
               provider_profile_sha256=excluded.provider_profile_sha256,
               task_context_sha256=excluded.task_context_sha256,
               sequence=excluded.sequence,revision=excluded.revision,
               reservations=excluded.reservations,phase=excluded.phase,
               payload=excluded.payload,mac=excluded.mac",
            params![
                context.task_id.to_string(),
                context.room_id.to_string(),
                context.agent_id.to_string(),
                provider.id.to_string(),
                route,
                task_hash,
                sequence as i64,
                cp.revision() as i64,
                cp.provider_reservations() as i64,
                Self::phase_label(cp.phase()),
                payload,
                tag,
            ],
        )
        .map_err(|_| denied())?;
        tx.commit().map_err(|_| denied())?;
        Self::require_scope(core, context, provider)?;
        Ok(sequence)
    }

    fn valid_transition(old: &MultiStepCheckpoint, next: &MultiStepCheckpoint) -> bool {
        // A signed run's spending and tool-step ceilings are immutable.
        // Neither an in-flight marker nor a later settled revision can raise
        // budgets without creating a separate user-reviewed *new* run.
        let budget = |cp: &MultiStepCheckpoint| {
            serde_json::to_value(cp)
                .ok()
                .and_then(|value| value.get("budget").cloned())
        };
        let old_budget = budget(old);
        if old_budget.is_none() || old_budget != budget(next) {
            return false;
        }
        match (old.phase(), next.phase()) {
            (MultiStepPhase::Settled, MultiStepPhase::ProviderInFlight) => {
                next.revision() == old.revision()
                    && next.provider_reservations() == old.provider_reservations() + 1
            }
            (MultiStepPhase::ProviderInFlight, MultiStepPhase::NativeReviewRequired) => {
                next.revision() == old.revision()
                    && next.provider_reservations() == old.provider_reservations()
            }
            (
                MultiStepPhase::ProviderInFlight | MultiStepPhase::NativeReviewRequired,
                MultiStepPhase::Settled,
            ) => {
                next.revision() > old.revision()
                    && next.provider_reservations() == old.provider_reservations()
            }
            (MultiStepPhase::Settled, MultiStepPhase::Settled) => {
                next.revision() > old.revision()
                    && next.provider_reservations() == old.provider_reservations()
            }
            _ => false,
        }
    }

    /// Read-only; never create or repair missing/legacy schemas, keys or heads.
    /// In-flight provider request or native review is quarantined, not
    /// reconstructed as an approval or a billable provider retry.
    pub(crate) fn restore_pending_task(
        &self,
        core: &StudioStore,
        context: OperationContext,
        provider: &ProviderProfile,
    ) -> Result<Option<NativeMultiStepRecovery>, String> {
        let (route, task_hash) = Self::require_scope(core, context, provider)?;
        let conn = self.open_database(false)?;
        if Self::layout(&conn)? {
            return Err(denied());
        }
        let state = self.read_verified_row(&conn, context, provider, &route, &task_hash)?;
        Self::require_scope(core, context, provider)?;
        let Some((sequence, checkpoint, _)) = state else {
            return Ok(None);
        };
        match checkpoint.phase() {
            MultiStepPhase::Settled => {
                let bytes = checkpoint.to_bytes().map_err(|_| denied())?;
                let run = MultiStepRun::restore_from_host(context, &bytes, checkpoint.revision())
                    .map_err(|_| denied())?;
                Ok(Some(NativeMultiStepRecovery::Settled {
                    sequence,
                    run: Box::new(run),
                }))
            }
            phase => Ok(Some(NativeMultiStepRecovery::Quarantined {
                sequence,
                revision: checkpoint.revision(),
                reservations: checkpoint.provider_reservations(),
                phase,
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_core::{CredentialSource, OperationId, ProviderId, ProviderKind};
    use constellation_tools::LocalExecutor;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct FakeAnchor(Arc<Mutex<HashMap<TaskId, String>>>);
    impl MultiStepAnchor for FakeAnchor {
        fn read(&self, task: TaskId) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(&task).cloned())
        }
        fn pin(&self, task: TaskId, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(task, value.into());
            Ok(())
        }
    }

    struct RefusePin;
    impl MultiStepAnchor for RefusePin {
        fn read(&self, _task: TaskId) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn pin(&self, _task: TaskId, _value: &str) -> Result<(), String> {
            Err(denied())
        }
    }
    fn fixture() -> (
        tempfile::TempDir,
        StudioStore,
        OperationContext,
        ProviderProfile,
        FakeAnchor,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let core = StudioStore::open(dir.path().join("project.sqlite3")).unwrap();
        let room = core.create_room("Native test").unwrap();
        let agent = core.add_agent("Writer", "Role").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Private context")
            .unwrap();
        let profile = ProviderProfile {
            id: ProviderId::new(),
            display_name: "Fixture".into(),
            model_id: "m1".into(),
            base_url: "https://example.com/v1".into(),
            kind: ProviderKind::OpenaiChat,
            credentials: CredentialSource::None,
        };
        core.save_provider_profile(profile.clone()).unwrap();
        (
            dir,
            core,
            OperationContext {
                task_id: task.id,
                room_id: room.id,
                agent_id: agent.id,
            },
            profile,
            FakeAnchor::default(),
        )
    }
    #[test]
    fn settlement_restart_and_reservation_quarantine_never_authorize_model_replay() {
        let (dir, core, ctx, profile, anchor) = fixture();
        let path = dir.path().join("multi-step-checkpoints.sqlite3");
        let store =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x45; 32], anchor.clone()).unwrap();
        assert!(store.restore_pending_task(&core, ctx, &profile).is_err());
        assert!(!path.exists());
        let mut run = MultiStepRun::new(ctx, MultiStepBudget::new(3, 2).unwrap());
        assert_eq!(
            store
                .persist_pending_task(&core, ctx, &profile, &run.checkpoint_for_host(), None)
                .unwrap(),
            0
        );
        assert!(matches!(
            store.restore_pending_task(&core, ctx, &profile).unwrap(),
            Some(NativeMultiStepRecovery::Settled { sequence: 0, .. })
        ));
        let (ticket, in_flight) = run.reserve_model(0).unwrap();
        let mut inflated = serde_json::to_value(&in_flight).unwrap();
        inflated["budget"]["model_attempts"] = serde_json::json!(4);
        let inflated: MultiStepCheckpoint = serde_json::from_value(inflated).unwrap();
        assert!(
            store
                .persist_pending_task(&core, ctx, &profile, &inflated, Some(0))
                .is_err(),
            "original signed budget cannot be raised without a new run"
        );
        assert_eq!(
            store
                .persist_pending_task(&core, ctx, &profile, &in_flight, Some(0))
                .unwrap(),
            1
        );
        let recovery =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x45; 32], anchor.clone()).unwrap();
        match recovery
            .restore_pending_task(&core, ctx, &profile)
            .unwrap()
            .unwrap()
        {
            NativeMultiStepRecovery::Quarantined {
                sequence,
                revision,
                reservations,
                phase,
            } => {
                assert_eq!((sequence, revision, reservations), (1, 0, 1));
                assert_eq!(phase, MultiStepPhase::ProviderInFlight);
            }
            _ => panic!("uncertain provider cannot be restored as a run"),
        }
        // A real provider failure must be charged by the in-memory run;
        // the host has no right to replay the uncertain request after crash.
        run.provider_failed(ticket).unwrap();
        assert_eq!(
            store
                .persist_pending_task(&core, ctx, &profile, &run.checkpoint_for_host(), Some(1))
                .unwrap(),
            2
        );
        match recovery
            .restore_pending_task(&core, ctx, &profile)
            .unwrap()
            .unwrap()
        {
            NativeMultiStepRecovery::Settled { sequence, run } => {
                assert_eq!(sequence, 2);
                assert_eq!(run.model_attempts(), 1);
                assert_eq!(run.provider_reservations(), 1);
            }
            _ => panic!("valid charged failure must restore settled"),
        }
        assert!(
            store
                .persist_pending_task(
                    &core,
                    ctx,
                    &profile,
                    &MultiStepRun::new(ctx, MultiStepBudget::new(3, 2).unwrap())
                        .checkpoint_for_host(),
                    Some(2)
                )
                .is_err()
        );
        assert!(
            recovery
                .persist_pending_task(&core, ctx, &profile, &run.checkpoint_for_host(), Some(1))
                .is_err()
        );
    }

    #[test]
    fn signed_provider_principal_core_cancellation_and_forged_vault_head_fail_closed() {
        let (dir, core, ctx, profile, anchor) = fixture();
        let path = dir.path().join("multi-step-checkpoints.sqlite3");
        let store =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x56; 32], anchor.clone()).unwrap();
        let cp = MultiStepRun::new(ctx, MultiStepBudget::new(2, 1).unwrap()).checkpoint_for_host();
        store
            .persist_pending_task(&core, ctx, &profile, &cp, None)
            .unwrap();
        let mut changed = profile.clone();
        changed.base_url = "https://other.example/v1".into();
        assert!(store.restore_pending_task(&core, ctx, &changed).is_err());
        assert!(
            store
                .restore_pending_task(
                    &core,
                    OperationContext {
                        room_id: constellation_core::RoomId::new(),
                        ..ctx
                    },
                    &profile
                )
                .is_err()
        );
        let wrong =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x57; 32], anchor.clone()).unwrap();
        assert!(wrong.restore_pending_task(&core, ctx, &profile).is_err());
        anchor
            .pin(
                ctx.task_id,
                "0:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .unwrap();
        assert!(store.restore_pending_task(&core, ctx, &profile).is_err());
        // Recreate a fresh fixture for cancellation; a fully correct MAC
        // and OS head do not grant authority over a cancelled Core task.
        let (dir, core, ctx, profile, anchor) = fixture();
        let store = NativeMultiStepCheckpointStore::with_anchor(
            &dir.path().join("multi.db"),
            [0x58; 32],
            anchor,
        )
        .unwrap();
        let cp = MultiStepRun::new(ctx, MultiStepBudget::new(2, 1).unwrap()).checkpoint_for_host();
        store
            .persist_pending_task(&core, ctx, &profile, &cp, None)
            .unwrap();
        core.transition_task(
            ctx.task_id,
            TaskState::Pending,
            TaskState::Cancelled,
            OperationId::new(),
            None,
        )
        .unwrap();
        assert!(store.restore_pending_task(&core, ctx, &profile).is_err());
    }

    #[test]
    fn valid_hmac_sqlite_rollback_missing_anchor_and_foreign_schema_are_rejected() {
        let (dir, core, ctx, profile, anchor) = fixture();
        let path = dir.path().join("multi.db");
        let store =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x59; 32], anchor.clone()).unwrap();
        let mut run = MultiStepRun::new(ctx, MultiStepBudget::new(2, 1).unwrap());
        let initial = run.checkpoint_for_host();
        store
            .persist_pending_task(&core, ctx, &profile, &initial, None)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let (_, in_flight) = run.reserve_model(0).unwrap();
        store
            .persist_pending_task(&core, ctx, &profile, &in_flight, Some(0))
            .unwrap();
        // SQLite hot-replacement while connection is closed: even valid old
        // HMAC cannot match the independent higher OS write-sequence head.
        std::fs::write(&path, &original).unwrap();
        assert!(store.restore_pending_task(&core, ctx, &profile).is_err());
        let extra = dir.path().join("foreign.sqlite3");
        rusqlite::Connection::open(&extra)
            .unwrap()
            .execute_batch("CREATE TABLE checkpoints(task_id TEXT)")
            .unwrap();
        let other =
            NativeMultiStepCheckpointStore::with_anchor(&extra, [0x60; 32], FakeAnchor::default())
                .unwrap();
        assert!(
            other
                .persist_pending_task(&core, ctx, &profile, &initial, None)
                .is_err()
        );
        let empty_legacy = dir.path().join("preexisting-empty.db");
        std::fs::File::create(&empty_legacy).unwrap();
        let legacy = NativeMultiStepCheckpointStore::with_anchor(
            &empty_legacy,
            [0x62; 32],
            FakeAnchor::default(),
        )
        .unwrap();
        assert!(
            legacy
                .persist_pending_task(&core, ctx, &profile, &initial, None)
                .is_err(),
            "an already-present empty legacy DB cannot be adopted"
        );
        let valid = dir.path().join("valid.sqlite3");
        let missing =
            NativeMultiStepCheckpointStore::with_anchor(&valid, [0x61; 32], FakeAnchor::default())
                .unwrap();
        missing
            .persist_pending_task(&core, ctx, &profile, &initial, None)
            .unwrap();
        let fake = FakeAnchor::default();
        let lost = NativeMultiStepCheckpointStore::with_anchor(&valid, [0x61; 32], fake).unwrap();
        assert!(lost.restore_pending_task(&core, ctx, &profile).is_err());
        assert!(
            NativeMultiStepCheckpointStore::with_anchor(&valid, [0; 32], FakeAnchor::default())
                .is_err()
        );
    }

    #[test]
    fn native_review_is_only_quarantine_and_never_restores_a_pending_tool() {
        let (dir, core, context, provider, anchor) = fixture();
        let store = NativeMultiStepCheckpointStore::with_anchor(
            &dir.path().join("review-checkpoint.sqlite3"),
            [0x71; 32],
            anchor,
        )
        .unwrap();
        let mut run = MultiStepRun::new(context, MultiStepBudget::new(3, 2).unwrap());
        store
            .persist_pending_task(&core, context, &provider, &run.checkpoint_for_host(), None)
            .unwrap();
        let (ticket, pending) = run.reserve_model(0).unwrap();
        store
            .persist_pending_task(&core, context, &provider, &pending, Some(0))
            .unwrap();
        let workspace = dir.path().join("private-artifacts");
        std::fs::create_dir_all(&workspace).unwrap();
        let executor = LocalExecutor::new(&workspace, 4096, 4096).unwrap();
        run.receive_model(
            ticket,
            r#"{"type":"tool","action":"create_file","path":"unapproved.txt","content":"UNTRUSTED"}"#,
            &executor,
        ).unwrap();
        assert_eq!(run.phase(), MultiStepPhase::NativeReviewRequired);
        assert!(run.pending_for_trusted_host().is_some());
        assert_eq!(
            store
                .persist_pending_task(
                    &core,
                    context,
                    &provider,
                    &run.checkpoint_for_host(),
                    Some(1),
                )
                .unwrap(),
            2
        );
        match store
            .restore_pending_task(&core, context, &provider)
            .unwrap()
            .unwrap()
        {
            NativeMultiStepRecovery::Quarantined {
                phase,
                reservations,
                sequence,
                ..
            } => {
                assert_eq!(phase, MultiStepPhase::NativeReviewRequired);
                assert_eq!((reservations, sequence), (1, 2));
            }
            _ => panic!("pending native review must never grant an approval upon reopen"),
        }
        assert!(!workspace.join("unapproved.txt").exists());
    }

    #[test]
    fn vault_pin_failure_leaves_no_trusted_sqlite_record() {
        let (dir, core, context, provider, _anchor) = fixture();
        let db = dir.path().join("failed-pin.sqlite3");
        let store =
            NativeMultiStepCheckpointStore::with_anchor(&db, [0x72; 32], RefusePin).unwrap();
        let cp =
            MultiStepRun::new(context, MultiStepBudget::new(2, 1).unwrap()).checkpoint_for_host();
        assert!(
            store
                .persist_pending_task(&core, context, &provider, &cp, None)
                .is_err()
        );
        assert!(
            store
                .restore_pending_task(&core, context, &provider)
                .is_err()
        );
        let conn = Connection::open(&db).unwrap();
        let rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='multi_step_checkpoints'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            rows, 0,
            "a failed vault pin must roll back even schema creation"
        );
    }

    #[test]
    fn competing_independent_writers_have_one_initial_checkpoint_winner() {
        let (dir, core, context, provider, anchor) = fixture();
        let core = Arc::new(core);
        let path = dir.path().join("concurrent-checkpoint.sqlite3");
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let checkpoint =
            MultiStepRun::new(context, MultiStepBudget::new(2, 1).unwrap()).checkpoint_for_host();
        let workers = (0..2)
            .map(|_| {
                let core = Arc::clone(&core);
                let barrier = Arc::clone(&barrier);
                let path = path.clone();
                let provider = provider.clone();
                let anchor = anchor.clone();
                let checkpoint = checkpoint.clone();
                std::thread::spawn(move || {
                    let store =
                        NativeMultiStepCheckpointStore::with_anchor(&path, [0x73; 32], anchor)
                            .unwrap();
                    barrier.wait();
                    store.persist_pending_task(&core, context, &provider, &checkpoint, None)
                })
            })
            .collect::<Vec<_>>();
        let successes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap().is_ok())
            .filter(|success| *success)
            .count();
        assert_eq!(successes, 1, "two instances must not each reserve genesis");
        let reopened =
            NativeMultiStepCheckpointStore::with_anchor(&path, [0x73; 32], anchor).unwrap();
        assert!(matches!(
            reopened
                .restore_pending_task(&core, context, &provider)
                .unwrap(),
            Some(NativeMultiStepRecovery::Settled { sequence: 0, .. }),
        ));
    }
}
