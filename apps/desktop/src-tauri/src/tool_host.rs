//! Explicitly user-reviewed, one-use local file operations. No provider output
//! or model callback can approve an operation. The workspace is an app-managed
//! directory: arbitrary external files and OS processes remain unavailable.
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::native_consent::{LocalFileAction, NativeConsentProof};
use constellation_core::{OperationId, PermissionGate};
#[cfg(any(target_os = "windows", target_os = "macos", test))]
use constellation_tools::AuthenticatedJournalHead;
use constellation_tools::{
    AuthenticatedJournalAnchor, CancellationToken, ExecutionOutput, JournalOperationState,
    LocalExecutor, OperationContext, OperationDetails, OperationOutcome, OperationRequest,
    PreparedOperation, WriteMode,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_FILE_BYTES: usize = 16 * 1024;
const MAX_PENDING: usize = 8;
const AGENT_TURN_DENIAL_JOURNAL: &str = "agent-turn-denials.jsonl";
#[allow(dead_code)] // Staged independent executor; native multi-step host integration follows.
/// The multi-step coordinator never adopts the older single-proposal denial
/// history as its tool outcome journal. Its independent OS-pinned head is
/// verified before a provider attempt, native review or file effect.
const MULTI_STEP_EFFECTS_JOURNAL: &str = "native-multi-step-effects.jsonl";
#[cfg(any(target_os = "windows", target_os = "macos"))]
const JOURNAL_SERVICE: &str = "org.constellation.desktop.local-tool-journal.v1";
#[cfg(any(target_os = "windows", target_os = "macos"))]
const JOURNAL_HEAD_SERVICE: &str = "org.constellation.desktop.local-tool-journal-head.v1";

/// A journal's HMAC key authenticates records, but cannot detect deletion of
/// a valid suffix. Production also pins the last authenticated chain head in
/// a separate OS credential account. Only the native host can create it.
#[cfg(any(target_os = "windows", target_os = "macos"))]
struct OsJournalHeadAnchor {
    account: String,
    /// A missing head is eligible for initialization only when the journal
    /// did not exist before the writer acquired it. Never adopt a truncated
    /// nonempty journal or silently migrate legacy unanchored history.
    fresh_journal: bool,
    previous: std::sync::Mutex<Option<AuthenticatedJournalHead>>,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl OsJournalHeadAnchor {
    fn for_path(path: &Path) -> Self {
        Self {
            account: format!(
                "head-{:x}",
                Sha256::digest(path.to_string_lossy().as_bytes())
            ),
            fresh_journal: !path.exists(),
            previous: std::sync::Mutex::new(None),
        }
    }

    fn entry(&self) -> constellation_tools::Result<keyring::Entry> {
        keyring::Entry::new(JOURNAL_HEAD_SERVICE, &self.account)
            .map_err(|_| constellation_tools::ToolError::InvalidJournal)
    }

    fn read_head(&self) -> constellation_tools::Result<Option<AuthenticatedJournalHead>> {
        use keyring::Error;
        let encoded = match self.entry()?.get_password() {
            Ok(value) => value,
            Err(Error::NoEntry) => return Ok(None),
            Err(_) => return Err(constellation_tools::ToolError::InvalidJournal),
        };
        let head: AuthenticatedJournalHead = serde_json::from_str(&encoded)
            .map_err(|_| constellation_tools::ToolError::InvalidJournal)?;
        if head.last_tag().len() != 64
            || !head.last_tag().bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(constellation_tools::ToolError::InvalidJournal);
        }
        Ok(Some(head))
    }

    /// A head write is authoritative only after reading the same bytes back.
    /// The native journal writer lock serializes this operation with appends.
    fn pin_checked(&self, head: &AuthenticatedJournalHead) -> constellation_tools::Result<()> {
        let encoded = serde_json::to_string(head)
            .map_err(|_| constellation_tools::ToolError::InvalidJournal)?;
        self.entry()?
            .set_password(&encoded)
            .map_err(|_| constellation_tools::ToolError::InvalidJournal)?;
        if self.read_head()?.as_ref() != Some(head) {
            return Err(constellation_tools::ToolError::InvalidJournal);
        }
        Ok(())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl AuthenticatedJournalAnchor for OsJournalHeadAnchor {
    fn verify_open(&self, head: &AuthenticatedJournalHead) -> constellation_tools::Result<()> {
        let mut previous = self
            .previous
            .lock()
            .map_err(|_| constellation_tools::ToolError::InvalidJournal)?;
        if previous.is_some() {
            return Err(constellation_tools::ToolError::InvalidJournal);
        }
        match self.read_head()? {
            // If a vault head survives but the corresponding journal file
            // vanished, even a genesis-valued head is not a new profile.
            Some(saved) if !self.fresh_journal && &saved == head => {}
            None if self.fresh_journal
                && head.record_count() == 0
                && head.last_tag() == "0".repeat(64) =>
            {
                self.pin_checked(head)?;
            }
            _ => return Err(constellation_tools::ToolError::InvalidJournal),
        }
        *previous = Some(head.clone());
        Ok(())
    }

    fn pin_after_sync(&self, head: &AuthenticatedJournalHead) -> constellation_tools::Result<()> {
        let mut previous = self
            .previous
            .lock()
            .map_err(|_| constellation_tools::ToolError::InvalidJournal)?;
        let expected = previous
            .as_ref()
            .ok_or(constellation_tools::ToolError::InvalidJournal)?;
        if head.record_count() != expected.record_count().saturating_add(1)
            || head.last_tag().len() != 64
            || head.last_tag() == expected.last_tag()
            || self.read_head()?.as_ref() != Some(expected)
        {
            return Err(constellation_tools::ToolError::InvalidJournal);
        }
        self.pin_checked(head)?;
        *previous = Some(head.clone());
        Ok(())
    }
}

/// The agent-loop's denial journal is independent of the manual file journal:
/// the different canonical JSONL path yields a different OS-vault head account.
/// In particular, a model response cannot nominate its own journal or anchor.
fn agent_turn_denial_journal_path(
    workspace: &Path,
    private_journal_dir: &Path,
) -> Result<PathBuf, String> {
    if !workspace.is_absolute()
        || !private_journal_dir.is_absolute()
        || !private_journal_dir.is_dir()
    {
        return Err("Agent denial audit directories are unavailable".into());
    }
    let root = fs::canonicalize(workspace).map_err(|_| "Agent workspace unavailable")?;
    let audit_dir = fs::canonicalize(private_journal_dir)
        .map_err(|_| "Agent denial audit folder unavailable")?;
    if root.starts_with(&audit_dir) || audit_dir.starts_with(&root) {
        return Err("Agent denial audit must be outside the workspace".into());
    }
    Ok(audit_dir.join(AGENT_TURN_DENIAL_JOURNAL))
}

#[allow(dead_code)] // Safe path constructor is consumed by the new native host.
fn multi_step_effects_journal_path(
    workspace: &Path,
    private_journal_dir: &Path,
) -> Result<PathBuf, String> {
    let existing = agent_turn_denial_journal_path(workspace, private_journal_dir)?;
    Ok(existing.with_file_name(MULTI_STEP_EFFECTS_JOURNAL))
}

/// Independently anchored native executor for the bounded multi-step
/// coordinator. Distinct from manual file operations and legacy model-denial
/// records. This is an executor constructor, never a consent or scope grant.
#[cfg(any(target_os = "windows", target_os = "macos"))]
#[allow(dead_code)] // Never callable by a model; new native Tauri host will own invocation.
pub(crate) fn open_anchored_multi_step_executor(
    workspace: &Path,
    private_journal_dir: &Path,
    key: [u8; 32],
) -> Result<LocalExecutor, String> {
    let journal_path = multi_step_effects_journal_path(workspace, private_journal_dir)?;
    let anchor = Arc::new(OsJournalHeadAnchor::for_path(&journal_path));
    open_agent_turn_denials_with_anchor(workspace, &journal_path, key, anchor)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[allow(dead_code)]
pub(crate) fn open_anchored_multi_step_executor(
    _workspace: &Path,
    _private_journal_dir: &Path,
    _key: [u8; 32],
) -> Result<LocalExecutor, String> {
    Err("Independent OS multi-step journal anchor is unavailable on this platform".into())
}

/// A private preflight and the actual model settlement use the identical
/// OS-pinned journal constructor. Running preflight before the provider call
/// prevents an unanchored/truncated denial journal from spending a new model
/// attempt or returning any model decision.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn open_anchored_agent_turn_denials(
    workspace: &Path,
    private_journal_dir: &Path,
    key: [u8; 32],
) -> Result<LocalExecutor, String> {
    let journal_path = agent_turn_denial_journal_path(workspace, private_journal_dir)?;
    let anchor = Arc::new(OsJournalHeadAnchor::for_path(&journal_path));
    open_agent_turn_denials_with_anchor(workspace, &journal_path, key, anchor)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn open_anchored_agent_turn_denials(
    _workspace: &Path,
    _private_journal_dir: &Path,
    _key: [u8; 32],
) -> Result<LocalExecutor, String> {
    Err("Independent OS agent-denial journal anchor is unavailable on this platform".into())
}

#[cfg_attr(not(test), allow(dead_code))]
fn open_agent_turn_denials_with_anchor(
    workspace: &Path,
    journal_path: &Path,
    key: [u8; 32],
    anchor: Arc<dyn AuthenticatedJournalAnchor>,
) -> Result<LocalExecutor, String> {
    LocalExecutor::with_anchored_authenticated_journal(
        workspace,
        MAX_FILE_BYTES as u64,
        MAX_FILE_BYTES,
        journal_path,
        key,
        anchor,
    )
    .map_err(|_| "Independently authenticated agent denial journal is unavailable".into())
}

struct PendingFileOperation {
    prepared: PreparedOperation,
    /// Unpredictable, renderer-visible handle; never the content-derived
    /// prepared digest used by native consent, execution or the journal.
    approval_ticket: String,
    context: OperationContext,
    confirmation_in_progress: bool,
    /// The bytes being created must be inspected in the UI before approval.
    reviewed_content: Option<String>,
}

/// Non-cloneable native-only session state. All mutable access must be held by
/// the outer Tauri mutex; an operation leaves pending state before execution.
pub(crate) struct ToolHost {
    executor: LocalExecutor,
    pending: HashMap<OperationId, PendingFileOperation>,
}

/// Native-only review data. All details are taken directly from an immutable
/// PreparedOperation owned by the trusted Rust executor.
pub(crate) struct PendingNativeApproval {
    pub operation_id: String,
    pub task_id: String,
    pub canonical_path: PathBuf,
    pub action: LocalFileAction,
    pub sha256: Option<String>,
    pub byte_count: usize,
    pub digest: String,
    pub create_content: Option<String>,
}

impl ToolHost {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open(
        workspace: &Path,
        private_journal_dir: &Path,
        key: [u8; 32],
    ) -> Result<Self, String> {
        let journal_path = Self::journal_path(workspace, private_journal_dir)?;
        let executor = LocalExecutor::with_authenticated_journal(
            workspace,
            MAX_FILE_BYTES as u64,
            MAX_FILE_BYTES,
            &journal_path,
            key,
        )
        .map_err(|_| "Cannot open authenticated local file-operation journal".to_string())?;
        Ok(Self {
            executor,
            pending: HashMap::new(),
        })
    }

    /// Production entry point: an independently OS-pinned head rejects
    /// deletion of otherwise valid authenticated audit records. The anchor
    /// never serves as native consent or restores a consumed approval.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(crate) fn open_anchored(
        workspace: &Path,
        private_journal_dir: &Path,
        key: [u8; 32],
    ) -> Result<Self, String> {
        let journal_path = Self::journal_path(workspace, private_journal_dir)?;
        let anchor = Arc::new(OsJournalHeadAnchor::for_path(&journal_path));
        Self::open_with_anchor(workspace, &journal_path, key, anchor)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(crate) fn open_anchored(
        _workspace: &Path,
        _private_journal_dir: &Path,
        _key: [u8; 32],
    ) -> Result<Self, String> {
        Err("Independent OS journal-head anchor is unavailable on this platform".into())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn open_with_anchor(
        workspace: &Path,
        journal_path: &Path,
        key: [u8; 32],
        anchor: Arc<dyn AuthenticatedJournalAnchor>,
    ) -> Result<Self, String> {
        let executor = LocalExecutor::with_anchored_authenticated_journal(
            workspace,
            MAX_FILE_BYTES as u64,
            MAX_FILE_BYTES,
            journal_path,
            key,
            anchor,
        )
        .map_err(|_| "Cannot reconcile independently anchored local journal".to_string())?;
        Ok(Self {
            executor,
            pending: HashMap::new(),
        })
    }

    fn journal_path(workspace: &Path, journal_dir: &Path) -> Result<PathBuf, String> {
        if !workspace.is_absolute() || !journal_dir.is_absolute() || !journal_dir.is_dir() {
            return Err("Local tool directories are unavailable".into());
        }
        let root = fs::canonicalize(workspace).map_err(|_| "Workspace unavailable")?;
        let audit_dir = fs::canonicalize(journal_dir).map_err(|_| "Journal folder unavailable")?;
        if root.starts_with(&audit_dir) || audit_dir.starts_with(&root) {
            return Err("Tool journal must be outside the workspace".into());
        }
        let digest = Sha256::digest(root.to_string_lossy().as_bytes());
        Ok(audit_dir.join(format!("file-operations-{digest:x}.jsonl")))
    }

    pub(crate) fn prepare(
        &mut self,
        context: OperationContext,
        action: &str,
        relative_path: &str,
        content: Option<&str>,
    ) -> Result<Value, String> {
        if self.pending.len() >= MAX_PENDING {
            return Err("Resolve pending file approvals before requesting another".into());
        }
        if relative_path.is_empty()
            || relative_path.len() > 1024
            || relative_path.chars().any(char::is_control)
        {
            return Err("Provide a short relative file path".into());
        }
        let path = PathBuf::from(relative_path);
        let request = match action {
            "read" => {
                if content.is_some_and(|text| !text.is_empty()) {
                    return Err("A read operation cannot include new file content".into());
                }
                OperationRequest::Read { path }
            }
            "create_new" => {
                let text = content.ok_or("Missing new file contents")?;
                if text.is_empty() || text.len() > MAX_FILE_BYTES || text.contains('\0') {
                    return Err("New file contents must be 1–16384 UTF-8 bytes".into());
                }
                OperationRequest::Write {
                    path,
                    bytes: text.as_bytes().to_vec(),
                    mode: WriteMode::CreateNew,
                }
            }
            _ => return Err("Unsupported file operation".into()),
        };
        let prepared = self
            .executor
            .prepare(context, request)
            .map_err(|error| format!("Cannot prepare scoped file operation: {error}"))?;
        let preview = prepared.preview();
        let (canonical_path, byte_count, sha256) = match preview.details {
            OperationDetails::Read { canonical_file, .. } => (canonical_file, None, None),
            OperationDetails::Write {
                canonical_file,
                bytes,
                sha256,
                mode: WriteMode::CreateNew,
            } => (canonical_file, Some(bytes), Some(sha256)),
            _ => return Err("File-operation preview is not supported".into()),
        };
        let operation_id = OperationId::new();
        // Two independent OS-CSPRNG-backed UUIDv4s give 244 random bits.
        // The public digest key remains for IPC compatibility, but cannot be
        // recomputed from known path, task IDs or guessed private file bytes.
        let approval_ticket = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple(),
        );
        let mut summary = json!({
            "operation_id": operation_id.to_string(),
            "digest": approval_ticket,
            "action": action,
            "canonical_path": canonical_path.to_string_lossy(),
            "agent_id": context.agent_id.to_string(),
            "room_id": context.room_id.to_string(),
            "task_id": context.task_id.to_string(),
            "permission_granted": false
        });
        if action == "create_new" {
            summary["byte_count"] = json!(byte_count);
            summary["sha256"] = json!(sha256);
            summary["reviewed_content"] = json!(content);
        }
        self.pending.insert(
            operation_id,
            PendingFileOperation {
                prepared,
                approval_ticket,
                context,
                confirmation_in_progress: false,
                reviewed_content: content.map(ToOwned::to_owned),
            },
        );
        Ok(summary)
    }

    pub(crate) fn pending_task(
        &self,
        operation_id: OperationId,
    ) -> Option<constellation_core::TaskId> {
        self.pending
            .get(&operation_id)
            .map(|pending| pending.context.task_id)
    }

    /// Native-only binding between a model's authenticated turn and an
    /// immutable file review. The private prepared digest is never returned
    /// to the renderer and cannot be used as its public approval ticket.
    pub(crate) fn pending_prepared_digest(
        &self,
        operation_id: OperationId,
        context: OperationContext,
    ) -> Result<String, String> {
        let pending = self
            .pending
            .get(&operation_id)
            .ok_or("Unknown or consumed native file review")?;
        // This is a read-only identity check used both before the native
        // consent dialog and after it returns. Reserving that dialog must
        // never invalidate the original immutable model-origin commitment.
        if pending.context != context {
            return Err("Native file review identity is invalid".into());
        }
        Ok(pending.prepared.digest().to_owned())
    }

    /// Reconciles only authenticated outcome metadata; never revives an
    /// approval, recovers read bytes, or replays an uncertain side effect.
    pub(crate) fn recovery_state(
        &self,
        operation_id: OperationId,
        context: OperationContext,
        prepared_digest: &str,
    ) -> Result<JournalOperationState, String> {
        self.executor
            .journal_state_bound(operation_id, prepared_digest, context)
            .map_err(|_| "Authenticated local operation outcome is unavailable".into())
    }

    /// Audit the initial native *denial to inspect* a proposed file before
    /// preparing a read. This writes only an authenticated, opaque denial
    /// commitment: the executor never stats, opens, hashes or discloses the
    /// target. A denied intent is not an approval and has no replayable ticket.
    pub(crate) fn audit_denied_read_intent(
        &mut self,
        context: OperationContext,
        relative_path: &str,
    ) -> Result<(), String> {
        self.executor
            .deny_unprepared_read(OperationId::new(), context, relative_path)
            .map_err(|_| "Could not durably record native inspection denial".to_owned())
    }

    pub(crate) fn pending_native_approval(
        &mut self,
        operation_id: OperationId,
        expected_digest: &str,
    ) -> Result<PendingNativeApproval, String> {
        let pending = self
            .pending
            .get_mut(&operation_id)
            .ok_or("Unknown or used approval")?;
        if pending.confirmation_in_progress {
            return Err("This operation is already awaiting native confirmation".into());
        }
        if pending.approval_ticket != expected_digest {
            return Err("Native approval ticket differs from the pending operation".into());
        }
        let preview = pending.prepared.preview();
        let (action, canonical_path, sha256, byte_count) = match preview.details {
            OperationDetails::Read {
                canonical_file,
                sha256,
                bytes,
            } => (LocalFileAction::Read, canonical_file, Some(sha256), bytes),
            OperationDetails::Write {
                canonical_file,
                sha256,
                bytes,
                mode: WriteMode::CreateNew,
            } => (
                LocalFileAction::CreateNew,
                canonical_file,
                Some(sha256),
                bytes,
            ),
            _ => return Err("Unsupported operation in native confirmation".into()),
        };
        let create_content = match action {
            LocalFileAction::Read => None,
            LocalFileAction::CreateNew => {
                let content = pending
                    .reviewed_content
                    .as_deref()
                    .ok_or("Native review text is unavailable")?;
                let actual = format!("{:x}", Sha256::digest(content.as_bytes()));
                if sha256.as_deref() != Some(actual.as_str()) {
                    return Err("Native review bytes no longer match prepared content".into());
                }
                Some(content.to_owned())
            }
        };
        let approval = PendingNativeApproval {
            operation_id: operation_id.to_string(),
            task_id: pending.context.task_id.to_string(),
            canonical_path,
            action,
            sha256,
            byte_count,
            digest: preview.digest,
            create_content,
        };
        pending.confirmation_in_progress = true;
        Ok(approval)
    }

    pub(crate) fn deny(&mut self, operation_id: OperationId) -> Result<Value, String> {
        let pending = self
            .pending
            .remove(&operation_id)
            .ok_or("Unknown or used approval")?;
        self.executor
            .deny(operation_id, &pending.prepared)
            .map_err(|_| "Could not durably record rejected file operation")?;
        Ok(json!({"operation_id":operation_id.to_string(),"status":"rejected","executed":false}))
    }

    /// Called only from the explicit human-operated confirmation UI. This
    /// native function never accepts a permission scope or an approval boolean
    /// from the frontend. It grants exactly the immutable prepared scope and
    /// consumes the operation ID before dispatch, including uncertain errors.
    pub(crate) fn approve_and_execute(
        &mut self,
        operation_id: OperationId,
        expected_digest: &str,
        native_proof: NativeConsentProof,
    ) -> Result<Value, String> {
        let pending = self
            .pending
            .remove(&operation_id)
            .ok_or("Unknown or used approval")?;
        if !pending.confirmation_in_progress
            || pending.approval_ticket != expected_digest
            || !native_proof.verify(&operation_id.to_string(), pending.prepared.digest())
        {
            self.executor
                .deny(operation_id, &pending.prepared)
                .map_err(|_| "Could not durably record missing or invalid native approval")?;
            return Err("Independent native approval was missing or mismatched".into());
        }
        let mut gate = PermissionGate::new();
        gate.grant(pending.prepared.scope().clone())
            .map_err(|_| "Invalid exact file permission")?;
        let approval = self
            .executor
            .approve(&gate, operation_id, &pending.prepared)
            .map_err(|_| "Could not durably approve the file action")?;
        // No Process, Git, outbound network, or replacement actions are
        // reachable from this host. The token is fresh and not reusable.
        let (receipt, output) = self
            .executor
            .execute(
                &gate,
                &approval,
                pending.prepared,
                &CancellationToken::new(),
            )
            .map_err(|_| "File operation has an uncertain result; check journal")?;
        let succeeded = receipt.outcome == OperationOutcome::Succeeded;
        let (content, bytes, sha256) = match output {
            Some(ExecutionOutput::Read(read)) => {
                (Some(read.content), read.bytes_read, Some(read.sha256))
            }
            Some(ExecutionOutput::Write(write)) => (None, write.bytes_written, Some(write.sha256)),
            _ => (None, 0, None),
        };
        Ok(json!({
            "operation_id":operation_id.to_string(),
            "status":if succeeded { "succeeded" } else { "failed" },
            "executed":true,
            "bytes":bytes,
            "sha256":sha256,
            "content":content,
            "error":receipt.failure_reason,
            "receipt_digest":receipt.digest,
            "reviewed_content_bytes":pending.reviewed_content.as_ref().map(String::len)
        }))
    }
}

/// A stable 32-byte key lives in the OS user credential vault, outside the
/// journal. If the journal exists but its key is missing, refuse to rotate
/// silently because the previous approvals can no longer be authenticated.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn load_or_create_journal_key(
    journal_dir: &Path,
    workspace: &Path,
) -> Result<[u8; 32], String> {
    use keyring::{Entry, Error};
    let path = ToolHost::journal_path(workspace, journal_dir)?;
    // Isolate OS credentials between project profiles. In particular, an
    // isolated QA profile can never overwrite the user's real audit key.
    let account = format!(
        "journal-{:x}",
        Sha256::digest(path.to_string_lossy().as_bytes())
    );
    let entry =
        Entry::new(JOURNAL_SERVICE, &account).map_err(|_| "OS credential vault is unavailable")?;
    let encoded = match entry.get_password() {
        Ok(secret) => secret,
        Err(Error::NoEntry) if !path.exists() => {
            // UUID v4 uses the OS CSPRNG; two independent UUIDs provide
            // 244 random bits before the fixed version/variant bits.
            let mut key = [0u8; 32];
            key[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            key[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            let value = key
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            entry
                .set_password(&value)
                .map_err(|_| "Could not save authenticated audit key to OS vault")?;
            value
        }
        Err(_) => return Err("Authenticated audit key is unavailable; action denied".into()),
    };
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Stored audit key is invalid; action denied".into());
    }
    let mut key = [0u8; 32];
    for (index, pair) in encoded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[index] = u8::from_str_radix(
            std::str::from_utf8(pair).map_err(|_| "Invalid audit key")?,
            16,
        )
        .map_err(|_| "Invalid audit key")?;
    }
    Ok(key)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn load_or_create_journal_key(
    _journal_dir: &Path,
    _workspace: &Path,
) -> Result<[u8; 32], String> {
    Err("OS-authenticated tool journal is not available on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_core::{AgentId, RoomId, TaskId};
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };

    /// Independent in-memory vault simulator: survives dropping/reopening the
    /// host and cannot be modified by truncating its journal file.
    #[derive(Default)]
    struct FakeHeadAnchor {
        pinned: Mutex<Option<AuthenticatedJournalHead>>,
        fail_pin: AtomicBool,
    }

    impl AuthenticatedJournalAnchor for FakeHeadAnchor {
        fn verify_open(&self, head: &AuthenticatedJournalHead) -> constellation_tools::Result<()> {
            let mut pinned = self.pinned.lock().unwrap();
            match pinned.as_ref() {
                Some(previous) if previous == head => Ok(()),
                None if head.record_count() == 0 => {
                    *pinned = Some(head.clone());
                    Ok(())
                }
                _ => Err(constellation_tools::ToolError::InvalidJournal),
            }
        }

        fn pin_after_sync(
            &self,
            head: &AuthenticatedJournalHead,
        ) -> constellation_tools::Result<()> {
            if self.fail_pin.load(Ordering::SeqCst) {
                return Err(constellation_tools::ToolError::InvalidJournal);
            }
            let mut pinned = self.pinned.lock().unwrap();
            let previous = pinned
                .as_ref()
                .ok_or(constellation_tools::ToolError::InvalidJournal)?;
            if head.record_count() != previous.record_count() + 1 {
                return Err(constellation_tools::ToolError::InvalidJournal);
            }
            *pinned = Some(head.clone());
            Ok(())
        }
    }

    #[test]
    fn multi_step_effect_journal_is_private_distinct_and_cannot_alias_legacy_journals() {
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let step = multi_step_effects_journal_path(workspace.path(), audit.path()).unwrap();
        let denial = agent_turn_denial_journal_path(workspace.path(), audit.path()).unwrap();
        let manual = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        assert_eq!(step.file_name().unwrap(), MULTI_STEP_EFFECTS_JOURNAL);
        assert_eq!(
            step.parent().unwrap(),
            fs::canonicalize(audit.path()).unwrap()
        );
        assert_ne!(step, denial);
        assert_ne!(step, manual);
        assert_ne!(denial, manual);
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            let step_head = OsJournalHeadAnchor::for_path(&step).account;
            assert_ne!(step_head, OsJournalHeadAnchor::for_path(&denial).account);
            assert_ne!(step_head, OsJournalHeadAnchor::for_path(&manual).account);
        }
        assert!(multi_step_effects_journal_path(workspace.path(), workspace.path()).is_err());
        assert!(
            multi_step_effects_journal_path(workspace.path(), &workspace.path().join("missing"))
                .is_err()
        );
    }

    #[test]
    fn model_denial_journal_has_distinct_identity_and_rejects_signed_tail_rollback() {
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let denial_path = agent_turn_denial_journal_path(workspace.path(), audit.path()).unwrap();
        let manual_path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        assert_ne!(denial_path, manual_path);
        assert_eq!(denial_path.file_name().unwrap(), AGENT_TURN_DENIAL_JOURNAL);
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        assert_ne!(
            OsJournalHeadAnchor::for_path(&denial_path).account,
            OsJournalHeadAnchor::for_path(&manual_path).account
        );

        let anchor = Arc::new(FakeHeadAnchor::default());
        let context = context();
        {
            let executor = open_agent_turn_denials_with_anchor(
                workspace.path(),
                &denial_path,
                [0x91; 32],
                anchor.clone(),
            )
            .unwrap();
            executor
                .deny_unprepared_read(OperationId::new(), context, "never-read.txt")
                .unwrap();
            executor
                .deny_unprepared_read(OperationId::new(), context, "no-inspection.txt")
                .unwrap();
            assert_eq!(
                executor
                    .authenticated_journal_head()
                    .unwrap()
                    .record_count(),
                2
            );
        }
        {
            let reopened = open_agent_turn_denials_with_anchor(
                workspace.path(),
                &denial_path,
                [0x91; 32],
                anchor.clone(),
            )
            .unwrap();
            assert_eq!(
                reopened
                    .authenticated_journal_head()
                    .unwrap()
                    .record_count(),
                2
            );
        }

        let bytes = fs::read(&denial_path).unwrap();
        let tail = bytes[..bytes.len() - 1]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .unwrap();
        fs::write(&denial_path, &bytes[..=tail]).unwrap();
        // This is still a fully valid, HMAC-signed one-event prefix. The
        // independent head, rather than the chain verifier, rejects rollback.
        let inspected = LocalExecutor::with_authenticated_journal(
            workspace.path(),
            MAX_FILE_BYTES as u64,
            MAX_FILE_BYTES,
            &denial_path,
            [0x91; 32],
        )
        .unwrap();
        assert_eq!(
            inspected
                .authenticated_journal_head()
                .unwrap()
                .record_count(),
            1
        );
        drop(inspected);
        assert!(open_agent_turn_denials_with_anchor(
            workspace.path(), &denial_path, [0x91; 32], anchor,
        ).is_err());
    }

    #[test]
    fn model_denial_preflight_rejects_missing_pin_before_provider_dispatch() {
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let denial_path = agent_turn_denial_journal_path(workspace.path(), audit.path()).unwrap();
        // A valid HMAC journal predating the independent head cannot silently
        // become a trusted model ledger on the next provider request.
        {
            let legacy = LocalExecutor::with_authenticated_journal(
                workspace.path(),
                MAX_FILE_BYTES as u64,
                MAX_FILE_BYTES,
                &denial_path,
                [0x92; 32],
            )
            .unwrap();
            legacy
                .deny_unprepared_read(OperationId::new(), context(), "legacy.txt")
                .unwrap();
        }
        let mut provider_calls = 0_usize;
        let admitted = open_agent_turn_denials_with_anchor(
            workspace.path(),
            &denial_path,
            [0x92; 32],
            Arc::new(FakeHeadAnchor::default()),
        )
        .map(|guard| {
            // Mirrors the production pre-network gate: only a successful
            // guard can lead to a model attempt or provider transport.
            provider_calls += 1;
            drop(guard);
        });
        assert!(admitted.is_err());
        assert_eq!(provider_calls, 0);
    }

    /// Strictly opt-in. Every real vault account is scoped to a fresh
    /// temporary journal path; the guard deletes only that credential even
    /// when the test panics. Never read or mutate a user's real journal pin.
    #[cfg(windows)]
    #[test]
    fn optional_real_os_agent_denial_head_survives_restart_and_rejects_rollback() {
        if std::env::var("CONSTELLATION_TEST_OS_AGENT_DENIAL_HEAD").as_deref() != Ok("1") {
            return;
        }
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let denial_path = agent_turn_denial_journal_path(workspace.path(), audit.path()).unwrap();
        let denial_anchor = OsJournalHeadAnchor::for_path(&denial_path);
        let manual_path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        assert_ne!(
            denial_anchor.account,
            OsJournalHeadAnchor::for_path(&manual_path).account
        );
        let entry = keyring::Entry::new(JOURNAL_HEAD_SERVICE, &denial_anchor.account).unwrap();
        // Never overwrite or delete a preexisting account, even in the
        // astronomically unlikely event a temporary path ID is reused.
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
        struct Cleanup(Option<keyring::Entry>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Some(entry) = &self.0 {
                    let _ = entry.delete_credential();
                }
            }
        }
        impl Cleanup {
            fn erase(&mut self) {
                // An explicit checked cleanup verifies successful removal;
                // if it fails, Drop makes another best-effort attempt.
                self.0.as_ref().unwrap().delete_credential().unwrap();
                self.0.take();
            }
        }
        let mut cleanup = Cleanup(Some(entry));
        {
            let executor =
                open_anchored_agent_turn_denials(workspace.path(), audit.path(), [0x93; 32])
                    .unwrap();
            executor
                .deny_unprepared_read(OperationId::new(), context(), "refused.txt")
                .unwrap();
        }
        {
            let reopened =
                open_anchored_agent_turn_denials(workspace.path(), audit.path(), [0x93; 32])
                    .unwrap();
            assert_eq!(
                reopened
                    .authenticated_journal_head()
                    .unwrap()
                    .record_count(),
                1
            );
        }
        fs::write(&denial_path, []).unwrap();
        assert!(
            open_anchored_agent_turn_denials(workspace.path(), audit.path(), [0x93; 32],).is_err()
        );
        cleanup.erase();
        let removed = keyring::Entry::new(JOURNAL_HEAD_SERVICE, &denial_anchor.account).unwrap();
        assert!(matches!(
            removed.get_password(),
            Err(keyring::Error::NoEntry)
        ));
    }

    /// Opt-in: exercise the actual Windows credential vault on a unique
    /// temporary workspace and independently pinned manual file-action log.
    /// NativeConsentProof remains a test fixture, not an OS dialog result.
    /// No permanent user credential or existing app installation is accessed.
    #[cfg(windows)]
    #[test]
    fn optional_real_os_manual_head_detects_signed_finish_tail_rollback() {
        if std::env::var("CONSTELLATION_TEST_OS_MANUAL_HEAD").as_deref() != Ok("1") {
            return;
        }
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        let anchor = OsJournalHeadAnchor::for_path(&path);
        let entry = keyring::Entry::new(JOURNAL_HEAD_SERVICE, &anchor.account).unwrap();
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
        struct Cleanup(keyring::Entry);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.delete_credential();
            }
        }
        let cleanup = Cleanup(entry);
        {
            let mut host =
                ToolHost::open_anchored(workspace.path(), audit.path(), [0xa4; 32]).unwrap();
            let preview = host
                .prepare(
                    context(),
                    "create_new",
                    "vault-backed.txt",
                    Some("safe fixture"),
                )
                .unwrap();
            let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
            assert_eq!(
                native_approved_for_test(&mut host, id, preview["digest"].as_str().unwrap())
                    .unwrap()["status"],
                "succeeded"
            );
        }
        assert_eq!(
            fs::read_to_string(workspace.path().join("vault-backed.txt")).unwrap(),
            "safe fixture"
        );
        assert!(ToolHost::open_anchored(workspace.path(), audit.path(), [0xa4; 32]).is_ok());
        let bytes = fs::read(&path).unwrap();
        let tail = bytes[..bytes.len() - 1]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .unwrap()
            + 1;
        // Removing a complete authenticated Finish is invisible to a plain
        // HMAC-chain verifier, but a distinct real OS-vault head must reject.
        fs::write(&path, &bytes[..tail]).unwrap();
        assert!(ToolHost::open_anchored(workspace.path(), audit.path(), [0xa4; 32]).is_err());
        assert_eq!(
            fs::read_to_string(workspace.path().join("vault-backed.txt")).unwrap(),
            "safe fixture"
        );
        cleanup.0.delete_credential().unwrap();
        assert!(matches!(
            keyring::Entry::new(JOURNAL_HEAD_SERVICE, &anchor.account)
                .unwrap()
                .get_password(),
            Err(keyring::Error::NoEntry)
        ));
    }

    #[test]
    fn independently_anchored_host_reopens_and_detects_valid_suffix_rollback() {
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        let anchor = Arc::new(FakeHeadAnchor::default());
        {
            let mut host =
                ToolHost::open_with_anchor(workspace.path(), &path, [0x37; 32], anchor.clone())
                    .unwrap();
            let preview = host
                .prepare(
                    context(),
                    "create_new",
                    "verified.txt",
                    Some("pinned bytes"),
                )
                .unwrap();
            let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
            assert_eq!(
                native_approved_for_test(&mut host, id, preview["digest"].as_str().unwrap())
                    .unwrap()["status"],
                "succeeded"
            );
            assert_eq!(
                anchor
                    .pinned
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .record_count(),
                3
            );
        }
        {
            let host =
                ToolHost::open_with_anchor(workspace.path(), &path, [0x37; 32], anchor.clone())
                    .unwrap();
            assert_eq!(
                host.executor
                    .authenticated_journal_head()
                    .unwrap()
                    .record_count(),
                3
            );
        }
        let mut bytes = fs::read(&path).unwrap();
        let last_line = bytes[..bytes.len() - 1]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .unwrap();
        bytes.truncate(last_line + 1);
        // Still a valid authenticated sequence of approve/start; the only
        // missing record is the complete signed Finish suffix.
        fs::write(&path, bytes).unwrap();
        assert!(
            ToolHost::open_with_anchor(workspace.path(), &path, [0x37; 32], anchor.clone())
                .is_err()
        );
        // A missing journal must also be refused when its independent vault
        // head survives, even though a newly created empty log can parse.
        fs::remove_file(&path).unwrap();
        assert!(ToolHost::open_with_anchor(workspace.path(), &path, [0x37; 32], anchor).is_err());
        assert_eq!(
            fs::read_to_string(workspace.path().join("verified.txt")).unwrap(),
            "pinned bytes"
        );
    }

    #[test]
    fn absent_anchor_cannot_adopt_nonempty_legacy_journal() {
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        {
            let mut legacy = ToolHost::open(workspace.path(), audit.path(), [0x38; 32]).unwrap();
            let preview = legacy
                .prepare(context(), "create_new", "no.txt", Some("unwritten"))
                .unwrap();
            let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
            legacy.deny(id).unwrap();
        }
        assert!(
            ToolHost::open_with_anchor(
                workspace.path(),
                &path,
                [0x38; 32],
                Arc::new(FakeHeadAnchor::default()),
            )
            .is_err()
        );
    }

    #[test]
    fn independent_pin_failure_blocks_side_effect_and_quarantines_journal() {
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        let anchor = Arc::new(FakeHeadAnchor::default());
        {
            let mut host =
                ToolHost::open_with_anchor(workspace.path(), &path, [0x39; 32], anchor.clone())
                    .unwrap();
            let preview = host
                .prepare(
                    context(),
                    "create_new",
                    "blocked.txt",
                    Some("no side effect"),
                )
                .unwrap();
            let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
            anchor.fail_pin.store(true, Ordering::SeqCst);
            assert!(
                native_approved_for_test(&mut host, id, preview["digest"].as_str().unwrap())
                    .is_err()
            );
            assert!(!workspace.path().join("blocked.txt").exists());
            assert!(host.executor.authenticated_journal_head().is_err());
        }
        assert!(ToolHost::open_with_anchor(workspace.path(), &path, [0x39; 32], anchor,).is_err());
    }

    fn native_approved_for_test(
        host: &mut ToolHost,
        id: OperationId,
        digest: &str,
    ) -> Result<Value, String> {
        let native = host.pending_native_approval(id, digest)?;
        host.approve_and_execute(
            id,
            digest,
            NativeConsentProof::fixture_for_test(&id.to_string(), &native.digest),
        )
    }

    fn context() -> OperationContext {
        OperationContext {
            agent_id: AgentId::new(),
            room_id: RoomId::new(),
            task_id: TaskId::new(),
        }
    }

    #[test]
    fn approval_is_one_use_and_write_is_no_clobber_across_restart() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [7; 32]).unwrap();
        let preview = host
            .prepare(context(), "create_new", "file.txt", Some("hello"))
            .unwrap();
        assert_eq!(preview["permission_granted"], false);
        assert_eq!(preview["reviewed_content"], "hello");
        assert!(!workspace.path().join("file.txt").exists());
        let op: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
        let digest = preview["digest"].as_str().unwrap();
        let saved = native_approved_for_test(&mut host, op, digest).unwrap();
        assert_eq!(saved["status"], "succeeded");
        assert_eq!(
            fs::read_to_string(workspace.path().join("file.txt")).unwrap(),
            "hello"
        );
        assert!(native_approved_for_test(&mut host, op, digest).is_err());
        drop(host);
        let mut reopened = ToolHost::open(workspace.path(), journal.path(), [7; 32]).unwrap();
        let second = reopened
            .prepare(context(), "create_new", "file.txt", Some("changed"))
            .unwrap();
        let attempt: OperationId = second["operation_id"].as_str().unwrap().parse().unwrap();
        let result =
            native_approved_for_test(&mut reopened, attempt, second["digest"].as_str().unwrap())
                .unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(
            fs::read_to_string(workspace.path().join("file.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn read_only_recovery_binds_principal_and_reports_only_authenticated_outcome() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        let context = context();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [0x42; 32]).unwrap();
        let preview = host
            .prepare(
                context,
                "create_new",
                "recover.txt",
                Some("sensitive contents"),
            )
            .unwrap();
        let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
        let ticket = preview["digest"].as_str().unwrap();
        let digest = host.pending_prepared_digest(id, context).unwrap();
        assert!(matches!(
            host.recovery_state(id, context, &digest).unwrap(),
            JournalOperationState::Unknown
        ));
        assert_eq!(
            native_approved_for_test(&mut host, id, ticket).unwrap()["status"],
            "succeeded"
        );
        assert!(matches!(
            host.recovery_state(id, context, &digest).unwrap(),
            JournalOperationState::Completed {
                outcome: OperationOutcome::Succeeded,
                ..
            }
        ));
        drop(host);
        let reopened = ToolHost::open(workspace.path(), journal.path(), [0x42; 32]).unwrap();
        assert!(matches!(
            reopened.recovery_state(id, context, &digest).unwrap(),
            JournalOperationState::Completed {
                outcome: OperationOutcome::Succeeded,
                ..
            }
        ));
        assert!(
            reopened
                .recovery_state(
                    id,
                    OperationContext {
                        agent_id: AgentId::new(),
                        ..context
                    },
                    &digest
                )
                .is_err()
        );
        assert!(
            reopened
                .recovery_state(id, context, &"0".repeat(64))
                .is_err()
        );
        assert!(matches!(
            reopened
                .recovery_state(OperationId::new(), context, &digest)
                .unwrap(),
            JournalOperationState::Unknown
        ));
        assert_eq!(
            fs::read_to_string(workspace.path().join("recover.txt")).unwrap(),
            "sensitive contents"
        );
    }

    #[test]
    fn reserved_native_review_keeps_model_origin_digest_verifiable_until_one_use_execution() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        let context = context();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [0x53; 32]).unwrap();
        let preview = host
            .prepare(
                context,
                "create_new",
                "review.md",
                Some("human reviewed bytes"),
            )
            .unwrap();
        let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
        let ticket = preview["digest"].as_str().unwrap();
        let expected = host.pending_prepared_digest(id, context).unwrap();

        // The first native review check precedes the independently controlled
        // OS dialog. A forged context or public ticket still cannot stand in
        // for the private prepared digest.
        assert_ne!(expected, ticket);
        assert!(
            host.pending_prepared_digest(
                id,
                OperationContext {
                    agent_id: AgentId::new(),
                    ..context
                }
            )
            .is_err()
        );
        let approval = host.pending_native_approval(id, ticket).unwrap();
        assert_eq!(approval.digest, expected);
        assert!(host.pending_native_approval(id, ticket).is_err());

        // After human consent, the host authenticates the exact same private
        // digest again before consuming its one-use approval.
        assert_eq!(host.pending_prepared_digest(id, context).unwrap(), expected);
        assert!(
            host.pending_prepared_digest(
                id,
                OperationContext {
                    room_id: RoomId::new(),
                    ..context
                }
            )
            .is_err()
        );
        let result = host
            .approve_and_execute(
                id,
                ticket,
                NativeConsentProof::fixture_for_test(&id.to_string(), &approval.digest),
            )
            .unwrap();
        assert_eq!(result["status"], "succeeded");
        assert!(host.pending_prepared_digest(id, context).is_err());
        assert!(host.pending_native_approval(id, ticket).is_err());
        assert_eq!(
            fs::read_to_string(workspace.path().join("review.md")).unwrap(),
            "human reviewed bytes"
        );
    }

    #[test]
    fn rejecting_or_tampering_with_preview_cannot_write() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [9; 32]).unwrap();
        let first = host
            .prepare(context(), "create_new", "denied.txt", Some("secret"))
            .unwrap();
        let id: OperationId = first["operation_id"].as_str().unwrap().parse().unwrap();
        assert_eq!(host.deny(id).unwrap()["executed"], false);
        assert!(
            native_approved_for_test(&mut host, id, first["digest"].as_str().unwrap()).is_err()
        );
        assert!(!workspace.path().join("denied.txt").exists());
        let second = host
            .prepare(context(), "create_new", "tampered.txt", Some("hello"))
            .unwrap();
        let id: OperationId = second["operation_id"].as_str().unwrap().parse().unwrap();
        assert!(
            host.approve_and_execute(
                id,
                "forged",
                NativeConsentProof::fixture_for_test(&id.to_string(), "forged"),
            )
            .is_err()
        );
        assert!(
            native_approved_for_test(&mut host, id, second["digest"].as_str().unwrap()).is_err()
        );
        assert!(!workspace.path().join("tampered.txt").exists());
    }

    #[test]
    fn native_confirmation_is_reserved_once_and_rejection_consumes_it() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [0x41; 32]).unwrap();
        let task = context();
        let preview = host
            .prepare(task, "create_new", "review.md", Some("human review needed"))
            .unwrap();
        let operation_id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
        let digest = preview["digest"].as_str().unwrap();
        assert!(
            host.pending_native_approval(operation_id, "forged")
                .is_err()
        );
        let native = host.pending_native_approval(operation_id, digest).unwrap();
        assert_eq!(native.task_id, task.task_id.to_string());
        assert_eq!(native.action, LocalFileAction::CreateNew);
        assert_eq!(native.byte_count, "human review needed".len());
        assert!(native.canonical_path.ends_with("review.md"));
        assert_ne!(native.digest, digest);
        assert_eq!(
            native.digest,
            host.pending.get(&operation_id).unwrap().prepared.digest()
        );
        assert!(host.pending_native_approval(operation_id, digest).is_err());
        assert_eq!(host.deny(operation_id).unwrap()["executed"], false);
        assert!(host.pending_native_approval(operation_id, digest).is_err());
        assert!(!workspace.path().join("review.md").exists());
    }

    #[test]
    fn read_requires_consent_and_cannot_escape_workspace() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        fs::write(workspace.path().join("note.txt"), "not exported").unwrap();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [3; 32]).unwrap();
        assert!(
            host.prepare(context(), "read", "../note.txt", None)
                .is_err()
        );
        assert!(
            host.prepare(context(), "create_new", "a.txt", Some(""))
                .is_err()
        );
        assert!(host.prepare(context(), "process", "a.txt", None).is_err());
        let preview = host.prepare(context(), "read", "note.txt", None).unwrap();
        assert!(preview.get("reviewed_content").is_none());
        assert!(preview.get("sha256").is_none());
        assert!(preview.get("byte_count").is_none());
        let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
        let private_digest = host.pending.get(&id).unwrap().prepared.digest().to_owned();
        assert_ne!(preview["digest"].as_str().unwrap(), private_digest);
        assert_eq!(preview["digest"].as_str().unwrap().len(), 64);
        assert!(host.pending_native_approval(id, &private_digest).is_err());
        let native = host
            .pending_native_approval(id, preview["digest"].as_str().unwrap())
            .unwrap();
        assert_eq!(native.digest, private_digest);
        assert_eq!(native.byte_count, b"not exported".len());
        assert_eq!(
            native.sha256.as_deref(),
            Some(format!("{:x}", Sha256::digest(b"not exported")).as_str())
        );
        assert!(native.create_content.is_none());
        // The first reservation must not be repeatable. The test helper below
        // would reserve again, so execute using the already obtained proof.
        let result = host
            .approve_and_execute(
                id,
                preview["digest"].as_str().unwrap(),
                NativeConsentProof::fixture_for_test(&id.to_string(), &private_digest),
            )
            .unwrap();
        assert_eq!(result["content"], "not exported");
        drop(host);
        assert!(ToolHost::open(workspace.path(), journal.path(), [4; 32]).is_err());
    }

    #[test]
    fn denied_initial_read_intent_is_authenticated_without_file_disclosure() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        let private = "never-read-the-file-contents";
        fs::write(workspace.path().join("secret.txt"), private).unwrap();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [0x61; 32]).unwrap();
        let identity = context();
        assert!(host.pending.is_empty());
        host.audit_denied_read_intent(identity, "secret.txt")
            .unwrap();
        host.audit_denied_read_intent(identity, "absent.txt")
            .unwrap();
        assert!(host.pending.is_empty());
        assert!(
            host.audit_denied_read_intent(identity, "../external.txt")
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(workspace.path().join("secret.txt")).unwrap(),
            private
        );
        let journal_file = ToolHost::journal_path(workspace.path(), journal.path()).unwrap();
        let recorded = fs::read_to_string(&journal_file).unwrap();
        assert!(!recorded.contains("secret.txt"));
        assert!(!recorded.contains("absent.txt"));
        assert!(!recorded.contains(private));
        assert_eq!(recorded.matches("\"event\":\"deny_scoped\"").count(), 2);
        drop(host);
        let reopened = ToolHost::open(workspace.path(), journal.path(), [0x61; 32]);
        assert!(
            reopened.is_ok(),
            "authenticated denials must replay on reopening"
        );
    }

    #[test]
    fn identical_reads_have_distinct_opaque_tickets_and_stale_or_forged_tickets_fail() {
        let workspace = tempfile::tempdir().unwrap();
        let journal = tempfile::tempdir().unwrap();
        fs::write(workspace.path().join("fixed.txt"), "predictable secret").unwrap();
        let mut host = ToolHost::open(workspace.path(), journal.path(), [0x75; 32]).unwrap();
        let ctx = context();
        let first = host.prepare(ctx, "read", "fixed.txt", None).unwrap();
        let second = host.prepare(ctx, "read", "fixed.txt", None).unwrap();
        let id1: OperationId = first["operation_id"].as_str().unwrap().parse().unwrap();
        let id2: OperationId = second["operation_id"].as_str().unwrap().parse().unwrap();
        let ticket1 = first["digest"].as_str().unwrap();
        let ticket2 = second["digest"].as_str().unwrap();
        assert_ne!(ticket1, ticket2);
        for preview in [&first, &second] {
            assert_eq!(preview["action"], "read");
            assert!(!preview.as_object().unwrap().contains_key("sha256"));
            assert!(!preview.as_object().unwrap().contains_key("byte_count"));
            assert!(
                !preview
                    .as_object()
                    .unwrap()
                    .contains_key("reviewed_content")
            );
            assert!(preview.to_string().find("predictable secret").is_none());
        }
        let private1 = host.pending.get(&id1).unwrap().prepared.digest().to_owned();
        let private2 = host.pending.get(&id2).unwrap().prepared.digest().to_owned();
        assert_eq!(private1, private2);
        assert_ne!(private1, ticket1);
        assert_ne!(private2, ticket2);
        assert!(host.pending_native_approval(id2, ticket1).is_err());
        assert!(host.pending_native_approval(id2, &private2).is_err());
        assert!(host.pending_native_approval(id2, "0").is_err());
        assert_eq!(host.deny(id1).unwrap()["executed"], false);
        assert!(host.pending_native_approval(id1, ticket1).is_err());

        let native = host.pending_native_approval(id2, ticket2).unwrap();
        assert_eq!(native.digest, private2);
        assert_eq!(native.byte_count, b"predictable secret".len());
        // A forged native proof carrying only the public ticket is rejected;
        // even matching operation IDs cannot authorize it.
        assert!(
            host.approve_and_execute(
                id2,
                ticket2,
                NativeConsentProof::fixture_for_test(&id2.to_string(), ticket2)
            )
            .is_err()
        );
        assert!(host.pending_native_approval(id2, ticket2).is_err());
        assert!(!host.pending.contains_key(&id2));
        let journal_bytes = fs::read_dir(journal.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
            .map(|entry| fs::read_to_string(entry.path()).unwrap())
            .collect::<String>();
        assert!(!journal_bytes.contains(ticket1));
        assert!(!journal_bytes.contains(ticket2));
        assert!(journal_bytes.contains(&private1));
    }

    /// Explicit opt-in and unique randomly named OS account. Ordinary CI and
    /// users' real credential entries are never touched by this test.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn optional_real_os_journal_key_survives_restart_without_global_collision() {
        if std::env::var("CONSTELLATION_TEST_OS_TOOL_JOURNAL").as_deref() != Ok("1") {
            return;
        }
        let workspace = tempfile::tempdir().unwrap();
        let audit = tempfile::tempdir().unwrap();
        let path = ToolHost::journal_path(workspace.path(), audit.path()).unwrap();
        let account = format!(
            "journal-{:x}",
            Sha256::digest(path.to_string_lossy().as_bytes())
        );
        let entry = keyring::Entry::new(JOURNAL_SERVICE, &account).unwrap();
        struct Cleanup(keyring::Entry);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.delete_credential();
            }
        }
        let _cleanup = Cleanup(entry);
        let first = load_or_create_journal_key(audit.path(), workspace.path()).unwrap();
        let reopened = load_or_create_journal_key(audit.path(), workspace.path()).unwrap();
        assert_eq!(first, reopened);
        {
            let host = ToolHost::open(workspace.path(), audit.path(), first).unwrap();
            drop(host);
        }
        let _again = ToolHost::open(workspace.path(), audit.path(), reopened).unwrap();
    }
}
