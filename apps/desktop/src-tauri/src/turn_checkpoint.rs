//! Trusted host checkpoint persistence, deliberately unreachable from WebView.
//! This is NOT a native autonomous agent: the host must separately reconcile
//! pending tool approvals, authoritative task state and billable provider use.
//!
//! Each revision is authenticated with an OS-vault-only HMAC key and is
//! independently pinned to a per-task OS keyring anchor *before* the SQLite
//! commit. A crash between those writes fails closed (may require user
//! recovery) rather than restoring a stale attempt count or tool approval.
use std::{path::Path, time::Duration};

use constellation_agent_loop::{AgentTurn, HostTurnCheckpoint};
use constellation_core::{StudioStore, TaskId, TaskState};
use constellation_tools::OperationContext;
use ring::hmac;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const SERVICE: &str = "org.constellation.desktop.agent-checkpoint.v1";
const MAX_PAYLOAD: usize = 2048;
const SCHEMA: &str = "CREATE TABLE checkpoints (
           task_id TEXT PRIMARY KEY NOT NULL,
           room_id TEXT NOT NULL,
           agent_id TEXT NOT NULL,
           revision INTEGER NOT NULL CHECK(revision >= 0 AND revision <= 64),
           payload BLOB NOT NULL,
           mac BLOB NOT NULL
         );";

fn denied() -> String {
    "Trusted agent checkpoint cannot be authenticated or resumed".into()
}

/// The OS keyring stores a sealed revision *separate from the SQLite file*.
/// A test anchor models the exact failure/rollback behavior without touching
/// the user's credentials. Only the OS anchor is used outside tests.
pub(crate) trait Anchor {
    fn read(&self, task: TaskId) -> Result<Option<String>, String>;
    fn pin(&self, task: TaskId, value: &str) -> Result<(), String>;
}

pub(crate) struct OsAnchor {
    workspace: String,
}

impl OsAnchor {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn account(&self, task: TaskId) -> String {
        format!("{}-{}", self.workspace, task)
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl Anchor for OsAnchor {
    fn read(&self, task: TaskId) -> Result<Option<String>, String> {
        use keyring::{Entry, Error};
        let entry = Entry::new(SERVICE, &self.account(task)).map_err(|_| denied())?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(Error::NoEntry) => Ok(None),
            Err(_) => Err(denied()),
        }
    }

    fn pin(&self, task: TaskId, value: &str) -> Result<(), String> {
        use keyring::Entry;
        Entry::new(SERVICE, &self.account(task))
            .map_err(|_| denied())?
            .set_password(value)
            .map_err(|_| denied())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Anchor for OsAnchor {
    fn read(&self, _task: TaskId) -> Result<Option<String>, String> {
        Err(denied())
    }
    fn pin(&self, _task: TaskId, _value: &str) -> Result<(), String> {
        Err(denied())
    }
}

pub(crate) struct TrustedTurnStore<A: Anchor = OsAnchor> {
    database: std::path::PathBuf,
    key: Zeroizing<[u8; 32]>,
    anchor: A,
}

fn project_id(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err(denied());
    }
    let parent = path.parent().ok_or_else(denied)?;
    let parent = std::fs::canonicalize(parent).map_err(|_| denied())?;
    if !parent.is_dir() || path.file_name().is_none() {
        return Err(denied());
    }
    // Anchor identities must be stable across parent-directory aliases and
    // must never follow a preexisting link into somebody else's DB.
    if let Ok(metadata) = std::fs::symlink_metadata(path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(denied());
    }
    let canonical_candidate = parent.join(path.file_name().ok_or_else(denied)?);
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical_candidate.as_os_str().to_string_lossy().as_bytes())
    ))
}

fn decode_key(encoded: &str) -> Result<[u8; 32], String> {
    if encoded.len() != 64 || !encoded.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(denied());
    }
    let mut key = [0_u8; 32];
    for (i, pair) in encoded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[i] = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| denied())?, 16)
            .map_err(|_| denied())?;
    }
    Ok(key)
}

impl TrustedTurnStore<OsAnchor> {
    /// Only native trusted setup may acquire this store; no renderer-supplied
    /// path or key. Missing existing key fails, never silently rotates it.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[allow(dead_code)]
    pub(crate) fn open_os_vault(path: &Path) -> Result<Self, String> {
        use keyring::{Entry, Error};
        let id = project_id(path)?;
        let key_account = format!("{}-key", id);
        let entry = Entry::new(SERVICE, &key_account).map_err(|_| denied())?;
        let secret = Zeroizing::new(match entry.get_password() {
            Ok(encoded) => encoded,
            Err(Error::NoEntry) if !path.exists() => {
                let mut bytes = Zeroizing::new([0_u8; 32]);
                SystemRandom::new()
                    .fill(bytes.as_mut())
                    .map_err(|_| denied())?;
                let value = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
                entry.set_password(&value).map_err(|_| denied())?;
                value
            }
            Err(_) => return Err(denied()),
        });
        let key = decode_key(&secret)?;
        // The key stays in OS-vault storage, not SQLite. The anchor account
        // separately records every committed task revision/tag.
        Self::with_anchor(path, key, OsAnchor { workspace: id })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[allow(dead_code)]
    pub(crate) fn open_os_vault(_path: &Path) -> Result<Self, String> {
        Err(denied())
    }
}

impl<A: Anchor> TrustedTurnStore<A> {
    /// A completely absent checkpoint DB may be initialized by the first
    /// `persist_pending_task`; an existing but malformed DB must be verified
    /// by `restore_pending_task` and never silently rebuilt.
    pub(crate) fn has_checkpoint_database(&self) -> bool {
        self.database.exists()
    }
    /// A checkpoint does not carry task authority. Inspect *current* Core
    /// state before any trusted agent-step planning, and reject deleted,
    /// cancelled, running, finished and misbound agent/task identities.
    /// Callers must additionally hold the native task transition lock and
    /// reconcile the tool journal before a real model/tool dispatch.
    fn require_pending_task(store: &StudioStore, context: OperationContext) -> Result<(), String> {
        let snapshot = store.snapshot().map_err(|_| denied())?;
        match snapshot
            .tasks
            .iter()
            .find(|task| task.id == context.task_id)
        {
            Some(task)
                if task.room_id == context.room_id
                    && task.agent_id == context.agent_id
                    && task.state == TaskState::Pending =>
            {
                Ok(())
            }
            _ => Err(denied()),
        }
    }

    /// Host-side first half of task-checkpoint admission, with a real Core
    /// task linkage instead of accepting an arbitrary model-created context.
    #[allow(dead_code)]
    pub(crate) fn persist_pending_task(
        &self,
        store: &StudioStore,
        context: OperationContext,
        turn: &AgentTurn,
        previous_revision: Option<u64>,
    ) -> Result<(), String> {
        Self::require_pending_task(store, context)?;
        self.persist(context, turn, previous_revision)?;
        Self::require_pending_task(store, context)
    }

    /// Restored data is still not authorization for provider calls or OS
    /// actions. Require the authoritative Core record to be Pending both
    /// before and after authenticated checkpoint verification.
    #[allow(dead_code)]
    pub(crate) fn restore_pending_task(
        &self,
        store: &StudioStore,
        context: OperationContext,
    ) -> Result<Option<AgentTurn>, String> {
        Self::require_pending_task(store, context)?;
        let turn = self.restore(context)?;
        Self::require_pending_task(store, context)?;
        Ok(turn)
    }

    pub(crate) fn with_anchor(path: &Path, key: [u8; 32], anchor: A) -> Result<Self, String> {
        if key.iter().all(|b| *b == 0)
            || !path.is_absolute()
            || !path.parent().is_some_and(Path::is_dir)
        {
            return Err(denied());
        }
        Ok(Self {
            database: path.to_path_buf(),
            key: Zeroizing::new(key),
            anchor,
        })
    }

    fn open_database(&self, create: bool) -> Result<Connection, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | if create {
                OpenFlags::SQLITE_OPEN_CREATE
            } else {
                OpenFlags::empty()
            };
        let conn = Connection::open_with_flags(&self.database, flags).map_err(|_| denied())?;
        conn.busy_timeout(Duration::from_secs(4))
            .map_err(|_| denied())?;
        Ok(conn)
    }

    fn verify_layout(conn: &Connection) -> Result<bool, String> {
        let mut stmt = conn.prepare(
            "SELECT name,type,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
        ).map_err(|_| denied())?;
        let observed = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|_| denied())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| denied())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| denied())?;
        if observed.is_empty() && version == 0 {
            // sqlite_sequence left behind by a dropped foreign table is not
            // a new database and must not be adopted.
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
                .map_err(|_| denied())?;
            if total != 0 {
                return Err(denied());
            }
            return Ok(true);
        }
        let expected = Connection::open_in_memory().map_err(|_| denied())?;
        expected.execute_batch(SCHEMA).map_err(|_| denied())?;
        let expected_sql: String = expected
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='checkpoints'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| denied())?;
        if version != 1 || observed != vec![("checkpoints".into(), "table".into(), expected_sql)] {
            return Err(denied());
        }
        Ok(false)
    }

    fn tag_data(context: OperationContext, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(MAX_PAYLOAD + 160);
        data.extend_from_slice(b"constellation.agent-turn.host-checkpoint.v1\0");
        data.extend_from_slice(context.task_id.to_string().as_bytes());
        data.push(0);
        data.extend_from_slice(context.room_id.to_string().as_bytes());
        data.push(0);
        data.extend_from_slice(context.agent_id.to_string().as_bytes());
        data.push(0);
        data.extend_from_slice(payload);
        data
    }

    fn tag(&self, context: OperationContext, payload: &[u8]) -> Vec<u8> {
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.key.as_ref());
        hmac::sign(&key, &Self::tag_data(context, payload))
            .as_ref()
            .to_vec()
    }

    fn verify(&self, context: OperationContext, payload: &[u8], tag: &[u8]) -> Result<(), String> {
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.key.as_ref());
        hmac::verify(&key, &Self::tag_data(context, payload), tag).map_err(|_| denied())
    }

    fn parse_anchor(value: &str) -> Result<(u64, Vec<u8>), String> {
        let (rev, hex) = value.split_once(':').ok_or_else(denied)?;
        let rev = rev.parse::<u64>().map_err(|_| denied())?;
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(denied());
        }
        let mut tag = Vec::with_capacity(32);
        for pair in hex.as_bytes().as_chunks::<2>().0 {
            tag.push(
                u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| denied())?, 16)
                    .map_err(|_| denied())?,
            );
        }
        Ok((rev, tag))
    }

    fn anchor_value(revision: u64, tag: &[u8]) -> String {
        format!(
            "{revision}:{}",
            tag.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }

    /// Host-only CAS. The OS anchor is moved first, under an SQLite immediate
    /// writer lock; any later DB commit error makes the old record unavailable
    /// instead of allowing a stale attempt/approval replay. No secrets or
    /// approvals are stored in the checkpoint payload.
    #[allow(dead_code)]
    pub(crate) fn persist(
        &self,
        context: OperationContext,
        turn: &AgentTurn,
        previous_revision: Option<u64>,
    ) -> Result<(), String> {
        let checkpoint = turn.checkpoint_for_host().map_err(|_| denied())?;
        if context.task_id.to_string().len() != 36 {
            return Err(denied());
        }
        let payload = serde_json::to_vec(&checkpoint).map_err(|_| denied())?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD {
            return Err(denied());
        }
        let tag = self.tag(context, &payload);
        let mut conn = self.open_database(true)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| denied())?;
        let fresh = Self::verify_layout(&tx)?;
        if fresh {
            tx.execute_batch(SCHEMA).map_err(|_| denied())?;
            tx.pragma_update(None, "user_version", 1)
                .map_err(|_| denied())?;
        }
        let prior: Option<(i64, Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT revision,payload,mac FROM checkpoints WHERE task_id=?1",
                [context.task_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| denied())?;
        let anchored = self.anchor.read(context.task_id)?;
        match (prior, anchored, previous_revision) {
            (None, None, None) => {
                if checkpoint.revision() != 0 {
                    return Err(denied());
                }
            }
            (Some((rev, old_payload, old_mac)), Some(value), Some(expected)) => {
                let (anchor_rev, anchor_mac) = Self::parse_anchor(&value)?;
                if rev < 0
                    || rev as u64 != anchor_rev
                    || anchor_rev != expected
                    || old_mac != anchor_mac
                    || self.verify(context, &old_payload, &old_mac).is_err()
                    || checkpoint.revision() <= expected
                {
                    return Err(denied());
                }
            }
            _ => return Err(denied()),
        }
        // Trust the agent-loop's own identity, state and revision validator
        // before writing any durable revision anchor.
        AgentTurn::restore_from_host(context, checkpoint.clone(), checkpoint.revision())
            .map_err(|_| denied())?;
        self.anchor.pin(
            context.task_id,
            &Self::anchor_value(checkpoint.revision(), &tag),
        )?;
        tx.execute(
            "INSERT INTO checkpoints(task_id,room_id,agent_id,revision,payload,mac)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(task_id) DO UPDATE SET
             room_id=excluded.room_id,agent_id=excluded.agent_id,
             revision=excluded.revision,payload=excluded.payload,mac=excluded.mac",
            params![
                context.task_id.to_string(),
                context.room_id.to_string(),
                context.agent_id.to_string(),
                checkpoint.revision() as i64,
                payload,
                tag
            ],
        )
        .map_err(|_| denied())?;
        tx.commit().map_err(|_| denied())
    }

    /// A restored checkpoint is not permission or task-state authority.
    /// The caller must reconcile the authenticated operation journal and
    /// persisted TaskState before polling any model or file tool.
    #[allow(dead_code)]
    pub(crate) fn restore(&self, context: OperationContext) -> Result<Option<AgentTurn>, String> {
        let conn = self.open_database(false)?;
        if Self::verify_layout(&conn)? {
            return Err(denied());
        }
        type StoredRow = (String, String, i64, Vec<u8>, Vec<u8>);
        let stored: Option<StoredRow> = conn
            .query_row(
                "SELECT room_id,agent_id,revision,payload,mac FROM checkpoints WHERE task_id=?1",
                [context.task_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(|_| denied())?;
        let anchor = self.anchor.read(context.task_id)?;
        match (stored, anchor) {
            (None, None) => Ok(None),
            (Some((room, agent, revision, payload, tag)), Some(pin)) => {
                let (minimum_revision, expected_tag) = Self::parse_anchor(&pin)?;
                if room != context.room_id.to_string()
                    || agent != context.agent_id.to_string()
                    || revision < 0
                    || revision as u64 != minimum_revision
                    || payload.len() > MAX_PAYLOAD
                    || tag != expected_tag
                {
                    return Err(denied());
                }
                self.verify(context, &payload, &tag)?;
                let checkpoint: HostTurnCheckpoint =
                    serde_json::from_slice(&payload).map_err(|_| denied())?;
                AgentTurn::restore_from_host(context, checkpoint, minimum_revision)
                    .map(Some)
                    .map_err(|_| denied())
            }
            _ => Err(denied()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_core::{AgentId, RoomId, TaskId};
    use constellation_tools::{LocalExecutor, OperationContext};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct TestAnchor(Arc<Mutex<HashMap<String, String>>>);
    impl Anchor for TestAnchor {
        fn read(&self, task: TaskId) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(&task.to_string()).cloned())
        }
        fn pin(&self, task: TaskId, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(task.to_string(), value.into());
            Ok(())
        }
    }
    fn fixture() -> (
        tempfile::TempDir,
        TrustedTurnStore<TestAnchor>,
        OperationContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("trusted-turn.db");
        let store = TrustedTurnStore::with_anchor(&db, [7u8; 32], TestAnchor::default()).unwrap();
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        (dir, store, context)
    }

    #[test]
    fn empty_then_exact_authenticated_revision_survives_reopen_without_refunding_attempts() {
        let (dir, store, context) = fixture();
        let mut turn = AgentTurn::new(context);
        store.persist(context, &turn, None).unwrap();
        assert_eq!(store.restore(context).unwrap().unwrap().model_attempts(), 0);
        let executor = LocalExecutor::new(dir.path(), 4096, 4096).unwrap();
        assert!(turn.submit_model("invalid json", &executor).is_err());
        store.persist(context, &turn, Some(0)).unwrap();
        let restored = store.restore(context).unwrap().unwrap();
        assert_eq!(restored.revision(), 1);
        assert_eq!(restored.model_attempts(), 1);
        assert!(
            store
                .persist(context, &AgentTurn::new(context), Some(1))
                .is_err()
        );
        assert_eq!(store.restore(context).unwrap().unwrap().model_attempts(), 1);
    }

    #[test]
    fn sqlite_rollback_tamper_wrong_key_missing_anchor_and_swapped_identity_fail_closed() {
        let (_dir, store, context) = fixture();
        let turn = AgentTurn::new(context);
        store.persist(context, &turn, None).unwrap();
        let wrong_secret =
            TrustedTurnStore::with_anchor(&store.database, [8_u8; 32], store.anchor.clone())
                .unwrap();
        assert!(wrong_secret.restore(context).is_err());
        let restarted =
            TrustedTurnStore::with_anchor(&store.database, [7_u8; 32], store.anchor.clone())
                .unwrap();
        assert_eq!(restarted.restore(context).unwrap().unwrap().revision(), 0);
        let wrong = OperationContext {
            task_id: TaskId::new(),
            ..context
        };
        assert!(store.restore(wrong).unwrap().is_none());
        let wrong_room = OperationContext {
            room_id: RoomId::new(),
            ..context
        };
        assert!(store.restore(wrong_room).is_err());
        let db = Connection::open(&store.database).unwrap();
        db.execute("UPDATE checkpoints SET payload=x'7b7d'", [])
            .unwrap();
        assert!(store.restore(context).is_err());
        db.execute("DELETE FROM checkpoints", []).unwrap();
        assert!(
            store.restore(context).is_err(),
            "OS floor remains after DB rollback"
        );
    }

    #[test]
    fn malformed_foreign_or_partial_schema_is_never_repaired_or_adopted() {
        let (dir, store, context) = fixture();
        let db = Connection::open(&store.database).unwrap();
        db.execute_batch("CREATE TABLE checkpoints(task_id TEXT,payload BLOB)")
            .unwrap();
        assert!(
            store
                .persist(context, &AgentTurn::new(context), None)
                .is_err()
        );
        drop(db);
        let other = TrustedTurnStore::with_anchor(
            &dir.path().join("other.db"),
            [0u8; 32],
            TestAnchor::default(),
        );
        assert!(
            other.is_err(),
            "all-zero key cannot authorize a new checkpoint store"
        );
    }

    #[test]
    fn two_independent_writers_cannot_both_initialize_the_same_turn() {
        let (_dir, store, context) = fixture();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                let path = store.database.clone();
                let anchor = store.anchor.clone();
                std::thread::spawn(move || {
                    let contender =
                        TrustedTurnStore::with_anchor(&path, [7u8; 32], anchor).unwrap();
                    barrier.wait();
                    contender.persist(context, &AgentTurn::new(context), None)
                })
            })
            .collect::<Vec<_>>();
        let winners = handles
            .into_iter()
            .map(|h| h.join().unwrap().is_ok())
            .filter(|success| *success)
            .count();
        assert_eq!(winners, 1, "durable first-write CAS must have one winner");
        assert_eq!(store.restore(context).unwrap().unwrap().revision(), 0);
    }

    #[test]
    fn trusted_host_cannot_serialize_pending_tool_approval() {
        let (dir, store, context) = fixture();
        let executor = LocalExecutor::new(dir.path(), 4096, 4096).unwrap();
        let mut turn = AgentTurn::new(context);
        turn.submit_model(
            r#"{"type":"tool","action":"create_file","path":"test.txt","content":"new text"}"#,
            &executor,
        )
        .unwrap();
        assert!(store.persist(context, &turn, None).is_err());
        assert!(!store.database.exists());
    }

    #[test]
    fn real_core_task_linkage_rejects_cancelled_completed_and_forged_principal() {
        let (dir, checkpoint, _unused) = fixture();
        let core = StudioStore::open(dir.path().join("project.db")).unwrap();
        let room = core.create_room("Checkpoint authorization").unwrap();
        let agent = core.add_agent("Audited agent", "Human controlled").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Explicit coding job")
            .unwrap();
        let context = OperationContext {
            task_id: task.id,
            room_id: room.id,
            agent_id: agent.id,
        };
        assert!(
            checkpoint.restore_pending_task(&core, context).is_err(),
            "no checkpoint store can be silently created by read-only restore"
        );
        checkpoint
            .persist_pending_task(&core, context, &AgentTurn::new(context), None)
            .unwrap();
        assert_eq!(
            checkpoint
                .restore_pending_task(&core, context)
                .unwrap()
                .unwrap()
                .revision(),
            0
        );

        let forged = OperationContext {
            agent_id: constellation_core::AgentId::new(),
            ..context
        };
        assert!(checkpoint.restore_pending_task(&core, forged).is_err());
        assert!(
            checkpoint
                .persist_pending_task(&core, forged, &AgentTurn::new(forged), None)
                .is_err()
        );

        core.transition_task(
            task.id,
            TaskState::Pending,
            TaskState::Cancelled,
            constellation_core::OperationId::new(),
            None,
        )
        .unwrap();
        assert!(
            checkpoint.restore_pending_task(&core, context).is_err(),
            "cancellation revokes checkpoint recovery despite valid MAC and OS floor"
        );
        assert!(
            checkpoint
                .persist_pending_task(&core, context, &AgentTurn::new(context), None)
                .is_err()
        );
        assert_eq!(
            checkpoint.restore(context).unwrap().unwrap().revision(),
            0,
            "checkpoint remains authenticated but is no longer task authority"
        );
    }

    /// Explicit opt-in: uses a random, isolated project path and cleans only
    /// two test-specific Windows/macOS vault accounts. It never accesses the
    /// user's production Constellation profile or installed application.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn optional_real_os_vault_checkpoint_key_and_revision_survive_restart() {
        if std::env::var("CONSTELLATION_TEST_OS_TURN_CHECKPOINT").as_deref() != Ok("1") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("isolated-checkpoint.db");
        let project = project_id(&path).unwrap();
        let context = OperationContext {
            room_id: RoomId::new(),
            agent_id: AgentId::new(),
            task_id: TaskId::new(),
        };
        let key_account = format!("{project}-key");
        let floor_account = format!("{project}-{}", context.task_id);
        struct Cleanup(Vec<keyring::Entry>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for entry in &self.0 {
                    let _ = entry.delete_credential();
                }
            }
        }
        let _cleanup = Cleanup(vec![
            keyring::Entry::new(SERVICE, &key_account).unwrap(),
            keyring::Entry::new(SERVICE, &floor_account).unwrap(),
        ]);
        let store = TrustedTurnStore::open_os_vault(&path).unwrap();
        assert!(!path.exists(), "opening the vault must not create a DB");
        store
            .persist(context, &AgentTurn::new(context), None)
            .unwrap();
        drop(store);
        let reopened = TrustedTurnStore::open_os_vault(&path).unwrap();
        assert_eq!(reopened.restore(context).unwrap().unwrap().revision(), 0);
        let mut turn = AgentTurn::new(context);
        let executor = LocalExecutor::new(dir.path(), 4096, 4096).unwrap();
        assert!(turn.submit_model("broken-json", &executor).is_err());
        reopened.persist(context, &turn, Some(0)).unwrap();
        drop(reopened);
        let third = TrustedTurnStore::open_os_vault(&path).unwrap();
        assert_eq!(third.restore(context).unwrap().unwrap().model_attempts(), 1);
        assert!(
            third
                .restore(OperationContext {
                    room_id: RoomId::new(),
                    ..context
                })
                .is_err()
        );
        drop(third);
        // A lost key on an existing checkpoint must not create a new key and
        // accidentally permit replay of an old model or file approval.
        _cleanup.0[0].delete_credential().unwrap();
        assert!(TrustedTurnStore::open_os_vault(&path).is_err());
    }
}
