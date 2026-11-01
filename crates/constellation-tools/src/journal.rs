//! Opt-in append-only log. The record of starting an attempt is synced to
//! disk *before* its side effects. An interrupted attempt is never replayed.
//! A private, exclusive sidecar lock protects against two live executors
//! loading the same log. A crash leaves that lock for deliberate recovery.

use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use constellation_core::OperationId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ExecutionReceipt, OperationContext};
use crate::{Result, ToolError};

const MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
const MAX_RECORD_BYTES: usize = 32 * 1024;
const INITIAL_TAG: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone)]
pub(super) struct History {
    pub digest: String,
    pub receipt: Option<ExecutionReceipt>,
    pub context: Option<OperationContext>,
    pub started: bool,
    pub denied: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    Approve {
        id: OperationId,
        digest: String,
    },
    Deny {
        id: OperationId,
        digest: String,
    },
    Start {
        id: OperationId,
        digest: String,
    },
    Finish {
        receipt: ExecutionReceipt,
    },
    /// Scoped event names preserve the original authenticated JSON wire format.
    /// Re-serializing legacy events must not invalidate existing HMAC tags.
    ApproveScoped {
        id: OperationId,
        digest: String,
        context: OperationContext,
    },
    DenyScoped {
        id: OperationId,
        digest: String,
        context: OperationContext,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthenticatedRecord {
    sequence: usize,
    previous_tag: String,
    record: Record,
    tag: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedJournalHead {
    record_count: usize,
    last_tag: String,
}

/// Independently retained trusted chain head. An OS-vault-backed host must
/// verify the head before exposing any operation and durably pin each new head
/// after the journal fsync, before a caller can dispatch the corresponding
/// effect. This is *not* an approval or a replay token.
pub trait AuthenticatedJournalAnchor: Send + Sync {
    fn verify_open(&self, head: &AuthenticatedJournalHead) -> Result<()>;
    fn pin_after_sync(&self, head: &AuthenticatedJournalHead) -> Result<()>;
}

impl AuthenticatedJournalHead {
    pub fn record_count(&self) -> usize {
        self.record_count
    }

    pub fn last_tag(&self) -> &str {
        &self.last_tag
    }
}

pub(super) struct JournalInspection {
    pub history: HashMap<OperationId, History>,
    pub head: AuthenticatedJournalHead,
}

pub(super) struct Journal {
    stream: File,
    journal_identity: PathBuf,
    lock_file: Option<File>,
    lock_path: PathBuf,
    history: HashMap<OperationId, History>,
    event_count: usize,
    current_bytes: u64,
    broken: bool,
    authentication_key: Option<[u8; 32]>,
    anchor: Option<Arc<dyn AuthenticatedJournalAnchor>>,
    last_tag: String,
}

impl Journal {
    /// Never creates a journal, a lock, or an executable; safe while a writer
    /// owns the exclusive lock. A torn/partial snapshot fails authentication.
    pub fn inspect_authenticated(
        path: &Path,
        workspace: &Path,
        key: [u8; 32],
    ) -> Result<JournalInspection> {
        let path = validate_path(path, workspace, true)?;
        let file = OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|_| ToolError::InvalidJournal)?;
        let (history, head, _) = load_history(&file, &path, Some(&key))?;
        Ok(JournalInspection { history, head })
    }

    pub fn authenticated_head(&self) -> Result<AuthenticatedJournalHead> {
        if self.authentication_key.is_none() || self.broken {
            return Err(ToolError::InvalidJournal);
        }
        Ok(AuthenticatedJournalHead {
            record_count: self.event_count,
            last_tag: self.last_tag.clone(),
        })
    }

    pub fn open(path: &Path, workspace: &Path) -> Result<Self> {
        Self::open_inner(path, workspace, None, None)
    }

    pub fn open_authenticated(path: &Path, workspace: &Path, key: [u8; 32]) -> Result<Self> {
        Self::open_inner(path, workspace, Some(key), None)
    }

    pub fn open_anchored_authenticated(
        path: &Path,
        workspace: &Path,
        key: [u8; 32],
        anchor: Arc<dyn AuthenticatedJournalAnchor>,
    ) -> Result<Self> {
        Self::open_inner(path, workspace, Some(key), Some(anchor))
    }

    fn open_inner(
        path: &Path,
        workspace: &Path,
        authentication_key: Option<[u8; 32]>,
        anchor: Option<Arc<dyn AuthenticatedJournalAnchor>>,
    ) -> Result<Self> {
        let path = validate_path(path, workspace, false)?;
        let parent = path.parent().ok_or(ToolError::InvalidJournal)?;
        let name = path.file_name().ok_or(ToolError::InvalidJournal)?;

        let mut lock_name = OsString::from(name);
        lock_name.push(".lock");
        let lock_path = parent.join(lock_name);
        let mut lock_options = OpenOptions::new();
        lock_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            lock_options.mode(0o600);
        }
        let lock_file = lock_options
            .open(&lock_path)
            .map_err(|_| ToolError::InvalidJournal)?;

        let loaded = (|| -> Result<Self> {
            let mut options = OpenOptions::new();
            options.read(true).append(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let stream = options.open(&path).map_err(|_| ToolError::InvalidJournal)?;
            if !stream
                .metadata()
                .map_err(|_| ToolError::InvalidJournal)?
                .is_file()
            {
                return Err(ToolError::InvalidJournal);
            }
            let (history, head, size) = load_history(&stream, &path, authentication_key.as_ref())?;
            if let Some(pin) = &anchor {
                pin.verify_open(&head)?;
            }
            Ok(Self {
                stream,
                journal_identity: path.clone(),
                lock_file: None,
                lock_path: lock_path.clone(),
                history,
                event_count: head.record_count,
                current_bytes: size,
                broken: false,
                authentication_key,
                anchor,
                last_tag: head.last_tag,
            })
        })();
        match loaded {
            Ok(mut journal) => {
                journal.lock_file = Some(lock_file);
                Ok(journal)
            }
            Err(error) => {
                drop(lock_file);
                let _ = fs::remove_file(lock_path);
                Err(error)
            }
        }
    }

    pub fn history(&self) -> &HashMap<OperationId, History> {
        &self.history
    }

    pub fn approve(
        &mut self,
        id: OperationId,
        digest: &str,
        context: OperationContext,
    ) -> Result<()> {
        self.append(Record::ApproveScoped {
            id,
            digest: digest.into(),
            context,
        })
    }

    pub fn start(&mut self, id: OperationId, digest: &str) -> Result<()> {
        self.append(Record::Start {
            id,
            digest: digest.into(),
        })
    }

    pub fn deny(&mut self, id: OperationId, digest: &str, context: OperationContext) -> Result<()> {
        self.append(Record::DenyScoped {
            id,
            digest: digest.into(),
            context,
        })
    }

    pub fn finish(&mut self, receipt: &ExecutionReceipt) -> Result<()> {
        self.append(Record::Finish {
            receipt: receipt.clone(),
        })
    }

    fn append(&mut self, record: Record) -> Result<()> {
        if self.broken || self.event_count >= MAX_RECORDS {
            return Err(ToolError::InvalidJournal);
        }
        // An authenticated append modifies only the addressed operation.
        // Never clone the complete history: that made a long-running audit
        // journal quadratic in operation count, even for unrelated tasks.
        // Serialization and quota validation precede the state mutation.
        let (mut bytes, new_tag) = if let Some(key) = &self.authentication_key {
            let tag = record_tag(
                key,
                &self.journal_identity,
                self.event_count,
                &self.last_tag,
                &record,
            )?;
            let wrapped = AuthenticatedRecord {
                sequence: self.event_count,
                previous_tag: self.last_tag.clone(),
                record: record.clone(),
                tag: tag.clone(),
            };
            (
                serde_json::to_vec(&wrapped).map_err(|_| ToolError::InvalidJournal)?,
                Some(tag),
            )
        } else {
            (
                serde_json::to_vec(&record).map_err(|_| ToolError::InvalidJournal)?,
                None,
            )
        };
        if bytes.len() > MAX_RECORD_BYTES
            || self.current_bytes + bytes.len() as u64 + 1 > MAX_JOURNAL_BYTES
        {
            return Err(ToolError::InvalidJournal);
        }
        // Validate against a one-operation snapshot rather than cloning the
        // complete journal. No authoritative in-memory history may reflect a
        // transition until its authenticated record has been synced to disk.
        // All event variants address exactly one operation ID.
        let id = match &record {
            Record::Approve { id, .. }
            | Record::Deny { id, .. }
            | Record::Start { id, .. }
            | Record::ApproveScoped { id, .. }
            | Record::DenyScoped { id, .. } => *id,
            Record::Finish { receipt } => receipt.operation_id,
        };
        let mut staged = HashMap::new();
        if let Some(existing) = self.history.get(&id) {
            staged.insert(id, existing.clone());
        }
        apply(&mut staged, &record)?;
        let next_entry = staged.remove(&id).ok_or(ToolError::InvalidJournal)?;
        bytes.push(b'\n');
        let written = self
            .stream
            .write_all(&bytes)
            .and_then(|()| self.stream.sync_all());
        if written.is_err() {
            // A partial append or failed sync has an uncertain durable state.
            // Quarantine this session, retain the last committed in-memory
            // state, and require authenticated recovery before any new action.
            self.broken = true;
            return Err(ToolError::InvalidJournal);
        }
        if let Some(pin) = &self.anchor {
            let next_head = AuthenticatedJournalHead {
                record_count: self.event_count + 1,
                last_tag: new_tag.clone().ok_or(ToolError::InvalidJournal)?,
            };
            if pin.pin_after_sync(&next_head).is_err() {
                // Data is already synced, but the independent anchor is not
                // confirmed durable. Do not dispatch and never self-heal by
                // trusting this now-ahead journal on a subsequent open.
                self.broken = true;
                return Err(ToolError::InvalidJournal);
            }
        }
        self.history.insert(id, next_entry);
        self.current_bytes += bytes.len() as u64;
        self.event_count += 1;
        if let Some(tag) = new_tag {
            self.last_tag = tag;
        }
        Ok(())
    }
}

fn validate_path(path: &Path, workspace: &Path, must_exist: bool) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(ToolError::InvalidJournal);
    }
    let parent = path.parent().ok_or(ToolError::InvalidJournal)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| ToolError::InvalidJournal)?;
    if !canonical_parent.is_dir() || canonical_parent.starts_with(workspace) {
        return Err(ToolError::InvalidJournal);
    }
    let name = path.file_name().ok_or(ToolError::InvalidJournal)?;
    let resolved = canonical_parent.join(name);
    match fs::symlink_metadata(&resolved) {
        Ok(meta) if super::is_reparse(&meta) || !meta.is_file() => {
            return Err(ToolError::InvalidJournal);
        }
        Ok(_) => {}
        Err(_) if must_exist || resolved.exists() => return Err(ToolError::InvalidJournal),
        Err(_) => {}
    }
    Ok(resolved)
}

/// Same bounded and authenticated parser for reopening and read-only inspection.
fn load_history(
    stream: &File,
    path: &Path,
    authentication_key: Option<&[u8; 32]>,
) -> Result<(HashMap<OperationId, History>, AuthenticatedJournalHead, u64)> {
    let size = stream
        .metadata()
        .map_err(|_| ToolError::InvalidJournal)?
        .len();
    if size > MAX_JOURNAL_BYTES {
        return Err(ToolError::InvalidJournal);
    }
    let mut buffer = Vec::new();
    stream
        .try_clone()
        .map_err(|_| ToolError::InvalidJournal)?
        .take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| ToolError::InvalidJournal)?;
    if buffer.len() as u64 != size
        || (buffer.last().is_some() && buffer.last() != Some(&b'\n'))
        || stream
            .metadata()
            .map_err(|_| ToolError::InvalidJournal)?
            .len()
            != size
    {
        return Err(ToolError::InvalidJournal);
    }
    let mut history = HashMap::new();
    let mut event_count = 0;
    let mut last_tag = INITIAL_TAG.to_owned();
    // Only a single terminating newline is allowed. Blank interior records,
    // including appended blank lines, are not authenticated events.
    let lines = buffer.strip_suffix(b"\n").unwrap_or(&buffer);
    for line in lines.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            if buffer.is_empty() {
                break;
            }
            return Err(ToolError::InvalidJournal);
        }
        if line.len() > MAX_RECORD_BYTES || event_count >= MAX_RECORDS {
            return Err(ToolError::InvalidJournal);
        }
        if let Some(key) = authentication_key {
            let wrapped: AuthenticatedRecord =
                serde_json::from_slice(line).map_err(|_| ToolError::InvalidJournal)?;
            if wrapped.sequence != event_count || wrapped.previous_tag != last_tag {
                return Err(ToolError::InvalidJournal);
            }
            let expected = record_tag(
                key,
                path,
                wrapped.sequence,
                &wrapped.previous_tag,
                &wrapped.record,
            )?;
            if !tags_equal(&expected, &wrapped.tag) {
                return Err(ToolError::InvalidJournal);
            }
            apply(&mut history, &wrapped.record)?;
            last_tag = wrapped.tag;
        } else {
            let event: Record =
                serde_json::from_slice(line).map_err(|_| ToolError::InvalidJournal)?;
            apply(&mut history, &event)?;
        }
        event_count += 1;
    }
    Ok((
        history,
        AuthenticatedJournalHead {
            record_count: event_count,
            last_tag,
        },
        size,
    ))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn record_tag(
    key: &[u8; 32],
    journal_identity: &Path,
    sequence: usize,
    previous_tag: &str,
    record: &Record,
) -> Result<String> {
    // HMAC-SHA256, domain separated from other application signatures. The
    // tuple uses deterministic JSON element order independent of map order.
    let data = serde_json::to_vec(&(
        "constellation-tools-journal-hmac-v1",
        journal_identity.to_string_lossy(),
        sequence,
        previous_tag,
        record,
    ))
    .map_err(|_| ToolError::InvalidJournal)?;
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(data);
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner.finalize());
    Ok(format!("{:x}", outer.finalize()))
}

fn tags_equal(expected: &str, actual: &str) -> bool {
    if expected.len() != 64 || actual.len() != expected.len() {
        return false;
    }
    expected
        .bytes()
        .zip(actual.bytes())
        .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn apply(history: &mut HashMap<OperationId, History>, record: &Record) -> Result<()> {
    match record {
        Record::Approve { id, digest } | Record::ApproveScoped { id, digest, .. } => {
            if !valid_digest(digest) || history.contains_key(id) {
                return Err(ToolError::InvalidJournal);
            }
            history.insert(
                *id,
                History {
                    digest: digest.clone(),
                    receipt: None,
                    context: match record {
                        Record::ApproveScoped { context, .. } => Some(*context),
                        _ => None,
                    },
                    started: false,
                    denied: false,
                },
            );
        }
        Record::Deny { id, digest } | Record::DenyScoped { id, digest, .. } => {
            if !valid_digest(digest) || history.contains_key(id) {
                return Err(ToolError::InvalidJournal);
            }
            history.insert(
                *id,
                History {
                    digest: digest.clone(),
                    receipt: None,
                    context: match record {
                        Record::DenyScoped { context, .. } => Some(*context),
                        _ => None,
                    },
                    started: false,
                    denied: true,
                },
            );
        }
        Record::Start { id, digest } => {
            let entry = history.get_mut(id).ok_or(ToolError::InvalidJournal)?;
            if entry.started || entry.denied || entry.digest != *digest {
                return Err(ToolError::InvalidJournal);
            }
            entry.started = true;
        }
        Record::Finish { receipt } => {
            let entry = history
                .get_mut(&receipt.operation_id)
                .ok_or(ToolError::InvalidJournal)?;
            if !entry.started || entry.receipt.is_some() || entry.digest != receipt.digest {
                return Err(ToolError::InvalidJournal);
            }
            if let Some(context) = entry.context {
                if receipt.scope.agent_id != context.agent_id
                    || receipt.scope.room_id != context.room_id
                    || receipt.scope.task_id != context.task_id
                {
                    return Err(ToolError::InvalidJournal);
                }
            }
            entry.receipt = Some(receipt.clone());
        }
    }
    Ok(())
}

impl Drop for Journal {
    fn drop(&mut self) {
        drop(self.lock_file.take());
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::{JournalOperationState, LocalExecutor, OperationOutcome};
    use constellation_core::{AgentId, PermissionScope, RoomId, TaskId, ToolAction};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };

    #[derive(Default)]
    struct TestIndependentAnchor {
        saved: Mutex<Option<AuthenticatedJournalHead>>,
        reject_next_pin: AtomicBool,
    }

    impl AuthenticatedJournalAnchor for TestIndependentAnchor {
        fn verify_open(&self, head: &AuthenticatedJournalHead) -> Result<()> {
            let mut saved = self.saved.lock().map_err(|_| ToolError::InvalidJournal)?;
            match saved.as_ref() {
                Some(expected) if expected == head => Ok(()),
                None if head.record_count() == 0 => {
                    *saved = Some(head.clone());
                    Ok(())
                }
                _ => Err(ToolError::InvalidJournal),
            }
        }

        fn pin_after_sync(&self, head: &AuthenticatedJournalHead) -> Result<()> {
            if self.reject_next_pin.swap(false, Ordering::SeqCst) {
                return Err(ToolError::InvalidJournal);
            }
            let mut saved = self.saved.lock().map_err(|_| ToolError::InvalidJournal)?;
            if saved.as_ref().is_none_or(|previous| {
                previous.record_count().checked_add(1) != Some(head.record_count())
            }) {
                return Err(ToolError::InvalidJournal);
            }
            *saved = Some(head.clone());
            Ok(())
        }
    }

    #[test]
    fn independent_anchor_rejects_complete_valid_signed_tail_deletion() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("pinned.jsonl");
        let anchor = Arc::new(TestIndependentAnchor::default());
        let key = [0x39; 32];
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        {
            let mut journal =
                Journal::open_anchored_authenticated(&path, workspace.path(), key, anchor.clone())
                    .unwrap();
            journal
                .deny(OperationId::new(), &"c".repeat(64), context)
                .unwrap();
            journal
                .deny(OperationId::new(), &"c".repeat(64), context)
                .unwrap();
            assert_eq!(journal.authenticated_head().unwrap().record_count(), 2);
            assert_eq!(
                anchor
                    .saved
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .record_count(),
                2
            );
        }
        let bytes = fs::read(&path).unwrap();
        let earlier_end = bytes[..bytes.len() - 1]
            .iter()
            .rposition(|b| *b == b'\n')
            .unwrap()
            + 1;
        fs::write(&path, &bytes[..earlier_end]).unwrap();
        // A complete signed prefix authenticates in isolation, but cannot be
        // accepted by the host while its separate head is still at count 2.
        assert_eq!(
            Journal::inspect_authenticated(&path, workspace.path(), key)
                .unwrap()
                .head
                .record_count(),
            1
        );
        assert!(
            Journal::open_anchored_authenticated(&path, workspace.path(), key, anchor,).is_err()
        );
    }

    #[test]
    fn anchored_append_refuses_dispatch_when_head_pin_fails_after_sync() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("pin-failure.jsonl");
        let anchor = Arc::new(TestIndependentAnchor::default());
        let key = [0x51; 32];
        let mut journal =
            Journal::open_anchored_authenticated(&path, workspace.path(), key, anchor.clone())
                .unwrap();
        anchor.reject_next_pin.store(true, Ordering::SeqCst);
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let id = OperationId::new();
        assert!(journal.approve(id, &"d".repeat(64), context).is_err());
        assert!(journal.broken);
        assert!(!journal.history().contains_key(&id));
        assert!(journal.authenticated_head().is_err());
        assert!(journal.start(id, &"d".repeat(64)).is_err());
        drop(journal);
        assert_eq!(
            Journal::inspect_authenticated(&path, workspace.path(), key)
                .unwrap()
                .head
                .record_count(),
            1
        );
        assert!(
            Journal::open_anchored_authenticated(&path, workspace.path(), key, anchor,).is_err()
        );
    }

    #[test]
    fn missing_independent_anchor_must_not_adopt_an_existing_journal() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("legacy-no-anchor.jsonl");
        let key = [0x21; 32];
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let mut original = Journal::open_authenticated(&path, workspace.path(), key).unwrap();
        original
            .deny(OperationId::new(), &"b".repeat(64), context)
            .unwrap();
        drop(original);
        assert!(Journal::open_anchored_authenticated(
            &path,
            workspace.path(),
            key,
            Arc::new(TestIndependentAnchor::default()),
        )
        .is_err());
    }

    #[test]
    fn synced_start_without_finish_is_ambiguous_after_restart() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("incomplete.jsonl");
        let key = [17; 32];
        let id = OperationId::new();
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let digest = "a".repeat(64);
        {
            let mut journal = Journal::open_authenticated(&path, workspace.path(), key).unwrap();
            journal.approve(id, &digest, context).unwrap();
            journal.start(id, &digest).unwrap();
        }
        let inspected = Journal::inspect_authenticated(&path, workspace.path(), key).unwrap();
        let entry = inspected.history.get(&id).unwrap();
        assert_eq!(entry.context, Some(context));
        assert!(entry.started);
        assert!(entry.receipt.is_none());
        assert_eq!(inspected.head.record_count, 2);
        let reopened =
            LocalExecutor::with_authenticated_journal(workspace.path(), 1024, 1024, &path, key)
                .unwrap();
        assert!(matches!(
            reopened.journal_state_bound(id, &digest, context).unwrap(),
            JournalOperationState::StartedUnfinished { .. }
        ));
    }

    #[test]
    fn previously_signed_legacy_events_keep_the_same_hmac_encoding() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("legacy-authenticated.jsonl");
        let key = [23; 32];
        let id = OperationId::new();
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let digest = "f".repeat(64);
        {
            let mut journal = Journal::open_authenticated(&path, workspace.path(), key).unwrap();
            // Original event serialization, without the context extension.
            journal
                .append(Record::Approve {
                    id,
                    digest: digest.clone(),
                })
                .unwrap();
            journal.start(id, &digest).unwrap();
            journal
                .finish(&ExecutionReceipt {
                    operation_id: id,
                    digest: digest.clone(),
                    scope: PermissionScope {
                        agent_id: context.agent_id,
                        room_id: context.room_id,
                        task_id: context.task_id,
                        action: ToolAction::WriteFile,
                        resource: "hidden-source.txt".to_owned(),
                    },
                    outcome: OperationOutcome::Succeeded,
                    exit_code: None,
                    output_sha256: None,
                    failure_reason: None,
                })
                .unwrap();
        }
        let reopened =
            LocalExecutor::with_authenticated_journal(workspace.path(), 1024, 1024, &path, key)
                .unwrap();
        assert!(matches!(
            reopened.journal_state_bound(id, &digest, context).unwrap(),
            JournalOperationState::Completed {
                outcome: OperationOutcome::Succeeded,
                ..
            }
        ));
        assert!(!format!("{:?}", reopened.journal_state(id).unwrap()).contains("hidden-source.txt"));
    }

    #[test]
    fn invalid_append_leaves_authenticated_history_and_file_exactly_unchanged() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("invalid-transitions.jsonl");
        let key = [0x5a; 32];
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let id = OperationId::new();
        let digest = "5".repeat(64);
        let mut journal = Journal::open_authenticated(&path, workspace.path(), key).unwrap();
        journal.approve(id, &digest, context).unwrap();
        let head = journal.authenticated_head().unwrap();
        let bytes = fs::read(&path).unwrap();
        // Duplicate approval, start with wrong digest and finish before start
        // must all refuse without a speculative in-memory or disk transition.
        assert!(journal.approve(id, &digest, context).is_err());
        assert!(journal.start(id, &"6".repeat(64)).is_err());
        assert!(journal
            .finish(&ExecutionReceipt {
                operation_id: id,
                digest: digest.clone(),
                scope: PermissionScope {
                    agent_id: context.agent_id,
                    room_id: context.room_id,
                    task_id: context.task_id,
                    action: ToolAction::WriteFile,
                    resource: "nothing.txt".into(),
                },
                outcome: OperationOutcome::Succeeded,
                exit_code: None,
                output_sha256: None,
                failure_reason: None,
            })
            .is_err());
        assert_eq!(journal.authenticated_head().unwrap(), head);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(!journal.history()[&id].started);
        journal.start(id, &digest).unwrap();
        drop(journal);
        let trusted = Journal::inspect_authenticated(&path, workspace.path(), key).unwrap();
        assert_eq!(trusted.head.record_count(), 2);
        assert!(trusted.history[&id].started);
    }

    #[test]
    fn failed_write_quarantines_journal_without_exposing_staged_approval() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("failed-write.jsonl");
        let key = [0x47; 32];
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let first_id = OperationId::new();
        let second_id = OperationId::new();
        let digest = "e".repeat(64);
        let mut journal = Journal::open_authenticated(&path, workspace.path(), key).unwrap();
        journal.approve(first_id, &digest, context).unwrap();
        let head_before = journal.authenticated_head().unwrap();
        let bytes_before = fs::read(&path).unwrap();

        // Inject a real OS write error: the stream remains a valid readable
        // file handle but no longer has write permission. Do not simulate a
        // validation failure, which would miss the durability boundary.
        journal.stream = File::open(&path).unwrap();
        assert!(journal.approve(second_id, &digest, context).is_err());
        assert!(journal.broken);
        assert!(!journal.history().contains_key(&second_id));
        assert_eq!(journal.history()[&first_id].digest, digest);
        assert_eq!(journal.event_count, head_before.record_count());
        assert!(journal.authenticated_head().is_err());
        assert!(journal.start(first_id, &digest).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes_before);

        drop(journal);
        let inspected = Journal::inspect_authenticated(&path, workspace.path(), key).unwrap();
        assert_eq!(inspected.head, head_before);
        assert!(inspected.history.contains_key(&first_id));
        assert!(!inspected.history.contains_key(&second_id));
    }

    #[test]
    fn many_distinct_authenticated_operations_reopen_without_losing_history() {
        let workspace = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let path = private.path().join("many-operations.jsonl");
        let key = [0x38; 32];
        let context = OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        };
        let mut ids = Vec::new();
        {
            let mut journal = Journal::open_authenticated(&path, workspace.path(), key).unwrap();
            for _ in 0..256 {
                let id = OperationId::new();
                journal.deny(id, &"b".repeat(64), context).unwrap();
                ids.push(id);
            }
            assert_eq!(
                journal.authenticated_head().unwrap().record_count(),
                ids.len()
            );
        }
        let recovered = Journal::inspect_authenticated(&path, workspace.path(), key).unwrap();
        assert_eq!(recovered.head.record_count(), 256);
        assert_eq!(recovered.history.len(), ids.len());
        for id in ids {
            let history = &recovered.history[&id];
            assert!(history.denied);
            assert_eq!(history.context, Some(context));
        }
    }
}
