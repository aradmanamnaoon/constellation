//! Explicitly approved local effects. Host code owns the approval decision;
//! model output must never call `approve` directly.
//!
//! The permission gate and one-shot approval are independent requirements.
//! An optional external journal durably records approvals and receipts before
//! returning. Host code must also link them to the authoritative task store.
//! This crate is not an OS sandbox. Path swaps by another privileged process,
//! process network egress require additional OS policy. Windows process trees
//! are terminated using Job Objects, but this is not privilege isolation.

use std::{
    collections::HashMap,
    fs::{self},
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use constellation_core::{
    AgentId, OperationId, PermissionGate, PermissionScope, RoomId, TaskId, ToolAction,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::verified_open::{self, DirectoryIdentity};
use crate::{ReadReceipt, Result, ToolError, WorkspaceReader};

#[path = "journal.rs"]
mod journal;
pub use journal::{AuthenticatedJournalAnchor, AuthenticatedJournalHead};
use journal::{History, Journal};

#[cfg(windows)]
#[path = "windows_process.rs"]
mod windows_process;

#[cfg(unix)]
#[path = "unix_process.rs"]
mod unix_process;

const MAX_COMMAND_ARGS: usize = 128;
const MAX_ARGUMENT_BYTES: usize = 32_768;
const MAX_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationContext {
    pub agent_id: AgentId,
    pub room_id: RoomId,
    pub task_id: TaskId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WriteMode {
    /// Refuses to overwrite an existing file.
    CreateNew,
    /// Replaces a regular file only if its exact contents match this SHA-256.
    ReplaceIfSha256(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitAction {
    Status,
    Diff,
    /// This command requires a separate GitPush grant and an approval bound
    /// to an explicit HTTPS endpoint, instead of a mutable remote alias.
    Push {
        remote_url: String,
        branch: String,
    },
}

/// All paths except executable paths are relative to the selected workspace.
/// Executables must be existing, absolute regular files and are never found
/// through PATH. A trusted host must explicitly resolve package-manager
/// executable aliases (including Homebrew's symlinked Git) to a canonical
/// regular file before requesting approval; raw symlink/reparse paths are
/// rejected instead of silently redirecting an approval. The preview and
/// one-use digest bind the canonical file and its SHA-256.
/// Git uses fixed argv with hooks and external diff disabled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationRequest {
    Read {
        path: PathBuf,
    },
    Write {
        path: PathBuf,
        bytes: Vec<u8>,
        mode: WriteMode,
    },
    Process {
        executable: PathBuf,
        working_dir: PathBuf,
        args: Vec<String>,
        timeout: Duration,
    },
    Git {
        executable: PathBuf,
        repository: PathBuf,
        action: GitAction,
        timeout: Duration,
    },
}

enum Resolved {
    Read {
        file: PathBuf,
        sha256: String,
        bytes: usize,
    },
    Write {
        file: PathBuf,
        bytes: Vec<u8>,
        mode: WriteMode,
        parent_identity: DirectoryIdentity,
    },
    Command {
        executable: PathBuf,
        executable_sha256: String,
        cwd: PathBuf,
        cwd_identity: DirectoryIdentity,
        argv: Vec<String>,
        timeout: Duration,
        git: bool,
    },
}

/// Immutable operation plan. The digest covers the principal, resource,
/// canonical executable/cwd, argv, timeout, write mode and exact file bytes.
/// Its private fields prevent modifying a plan after approval.
pub struct PreparedOperation {
    scope: PermissionScope,
    digest: String,
    resolved: Resolved,
}

/// Display this preview to the user before granting its matching scope and
/// calling `approve`. Only the SHA and size, never file content, are included
/// for writes. Command arguments are returned verbatim for exact consent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationPreview {
    pub scope: PermissionScope,
    pub digest: String,
    pub details: OperationDetails,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationDetails {
    Read {
        canonical_file: PathBuf,
        sha256: String,
        bytes: usize,
    },
    Write {
        canonical_file: PathBuf,
        bytes: usize,
        sha256: String,
        mode: WriteMode,
    },
    Command {
        executable: PathBuf,
        executable_sha256: String,
        cwd: PathBuf,
        argv: Vec<String>,
        timeout: Duration,
        git: bool,
    },
}

impl PreparedOperation {
    pub fn scope(&self) -> &PermissionScope {
        &self.scope
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn preview(&self) -> OperationPreview {
        let details = match &self.resolved {
            Resolved::Read {
                file,
                sha256,
                bytes,
            } => OperationDetails::Read {
                canonical_file: file.clone(),
                sha256: sha256.clone(),
                bytes: *bytes,
            },
            Resolved::Write {
                file, bytes, mode, ..
            } => OperationDetails::Write {
                canonical_file: file.clone(),
                bytes: bytes.len(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
                mode: mode.clone(),
            },
            Resolved::Command {
                executable,
                executable_sha256,
                cwd,
                argv,
                timeout,
                git,
                ..
            } => OperationDetails::Command {
                executable: executable.clone(),
                executable_sha256: executable_sha256.clone(),
                cwd: cwd.clone(),
                argv: argv.clone(),
                timeout: *timeout,
                git: *git,
            },
        };
        OperationPreview {
            scope: self.scope.clone(),
            digest: self.digest.clone(),
            details,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationApproval {
    pub operation_id: OperationId,
    pub digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum OperationOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    OutputLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub operation_id: OperationId,
    pub digest: String,
    pub scope: PermissionScope,
    pub outcome: OperationOutcome,
    pub exit_code: Option<i32>,
    pub output_sha256: Option<String>,
    pub failure_reason: Option<String>,
}

/// Metadata suitable for recovery UI. Never includes read/write payloads,
/// process output, private resource paths, or free-form failure messages.
/// An authenticated journal must be verified before producing this state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalOperationState {
    Unknown,
    ApprovedUnstarted {
        digest: String,
        context: OperationContext,
    },
    Denied {
        digest: String,
        context: OperationContext,
    },
    StartedUnfinished {
        digest: String,
        context: OperationContext,
    },
    Completed {
        digest: String,
        context: OperationContext,
        outcome: OperationOutcome,
    },
}

impl JournalOperationState {
    /// A known operation must match both the exact preview commitment and all
    /// three principal identifiers. Unknown IDs remain explicitly unknown.
    pub fn verify_binding(&self, digest: &str, context: OperationContext) -> Result<()> {
        let (stored_digest, stored_context) = match self {
            Self::Unknown => return Ok(()),
            Self::ApprovedUnstarted { digest, context }
            | Self::Denied { digest, context }
            | Self::StartedUnfinished { digest, context }
            | Self::Completed {
                digest, context, ..
            } => (digest, context),
        };
        if stored_digest != digest || *stored_context != context {
            return Err(ToolError::ApprovalMismatch);
        }
        Ok(())
    }
}

fn state_from_history(
    history: &HashMap<OperationId, History>,
    operation_id: OperationId,
) -> Result<JournalOperationState> {
    let Some(entry) = history.get(&operation_id) else {
        return Ok(JournalOperationState::Unknown);
    };
    // Older signed records omitted principal metadata on approval/denial.
    // Completed old records can prove it via the authenticated receipt; old
    // incomplete/denied records cannot prove it and must fail closed.
    let context = entry
        .context
        .or_else(|| {
            entry.receipt.as_ref().map(|r| OperationContext {
                agent_id: r.scope.agent_id,
                room_id: r.scope.room_id,
                task_id: r.scope.task_id,
            })
        })
        .ok_or(ToolError::InvalidJournal)?;
    let digest = entry.digest.clone();
    Ok(if let Some(receipt) = &entry.receipt {
        JournalOperationState::Completed {
            digest,
            context,
            outcome: receipt.outcome.clone(),
        }
    } else if entry.denied {
        JournalOperationState::Denied { digest, context }
    } else if entry.started {
        JournalOperationState::StartedUnfinished { digest, context }
    } else {
        JournalOperationState::ApprovedUnstarted { digest, context }
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteReceipt {
    pub canonical_file: PathBuf,
    pub bytes_written: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

pub enum ExecutionOutput {
    Read(ReadReceipt),
    Write(WriteReceipt),
    Process(ProcessOutput),
}

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct ApprovalRecord {
    digest: String,
    consumed: bool,
    receipt: Option<ExecutionReceipt>,
}

/// A one-use local runner. Keep the approval ledger alive for the lifetime of
/// the desktop session. Always re-check the core permission gate at dispatch.
pub struct LocalExecutor {
    reader: WorkspaceReader,
    max_output_bytes: usize,
    approvals: Mutex<HashMap<OperationId, ApprovalRecord>>,
    journal: Option<Mutex<Journal>>,
}

impl LocalExecutor {
    pub fn new(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_output_bytes: usize,
    ) -> Result<Self> {
        if max_output_bytes == 0 || max_output_bytes > 1_048_576 {
            return Err(ToolError::InvalidCommand);
        }
        Ok(Self {
            reader: WorkspaceReader::new(root, max_file_bytes)?,
            max_output_bytes,
            approvals: Mutex::new(HashMap::new()),
            journal: None,
        })
    }

    /// Optional durable fail-closed approval/attempt/receipt log. The host
    /// must select a private journal outside the agent workspace. An existing
    /// journal is replayed without reviving prior unconsumed approvals; an
    /// unfinished attempt is never automatically retried after a crash.
    /// A second live owner or a stale crash lock prevents opening the journal.
    pub fn with_journal(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_output_bytes: usize,
        journal_path: impl AsRef<Path>,
    ) -> Result<Self> {
        Self::open_with_journal(
            root,
            max_file_bytes,
            max_output_bytes,
            journal_path,
            None,
            None,
        )
    }

    /// Durable HMAC-SHA256 authenticated audit log. The trusted host must
    /// retrieve a stable, unpredictable 32-byte key from storage separate
    /// from the journal (such as the OS credential store). A missing/wrong key
    /// or altered journal refuses to open; there is no unsealed fallback.
    /// An existing unsealed journal must not be opened in authenticated mode.
    pub fn with_authenticated_journal(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_output_bytes: usize,
        journal_path: impl AsRef<Path>,
        authentication_key: [u8; 32],
    ) -> Result<Self> {
        Self::open_with_journal(
            root,
            max_file_bytes,
            max_output_bytes,
            journal_path,
            Some(authentication_key),
            None,
        )
    }

    /// Unlike the legacy unanchored constructor, every authenticated append
    /// must also persist a separately held trusted chain head before the
    /// corresponding operation can execute. A host-provided anchor failure
    /// permanently quarantines the journal session; no approval is recovered.
    pub fn with_anchored_authenticated_journal(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_output_bytes: usize,
        journal_path: impl AsRef<Path>,
        authentication_key: [u8; 32],
        anchor: Arc<dyn AuthenticatedJournalAnchor>,
    ) -> Result<Self> {
        Self::open_with_journal(
            root,
            max_file_bytes,
            max_output_bytes,
            journal_path,
            Some(authentication_key),
            Some(anchor),
        )
    }

    fn open_with_journal(
        root: impl AsRef<Path>,
        max_file_bytes: u64,
        max_output_bytes: usize,
        journal_path: impl AsRef<Path>,
        authentication_key: Option<[u8; 32]>,
        anchor: Option<Arc<dyn AuthenticatedJournalAnchor>>,
    ) -> Result<Self> {
        let mut executor = Self::new(root, max_file_bytes, max_output_bytes)?;
        let journal = match authentication_key {
            Some(key) => match anchor {
                Some(pin) => Journal::open_anchored_authenticated(
                    journal_path.as_ref(),
                    executor.root(),
                    key,
                    pin,
                )?,
                None => Journal::open_authenticated(journal_path.as_ref(), executor.root(), key)?,
            },
            None => Journal::open(journal_path.as_ref(), executor.root())?,
        };
        executor.approvals = Mutex::new(
            journal
                .history()
                .iter()
                .map(|(id, item)| {
                    (
                        *id,
                        ApprovalRecord {
                            digest: item.digest.clone(),
                            consumed: true,
                            receipt: item.receipt.clone(),
                        },
                    )
                })
                .collect(),
        );
        executor.journal = Some(Mutex::new(journal));
        Ok(executor)
    }

    /// Query a journal already authenticated during executor construction.
    /// A status query never dispatches, revives, or consumes an operation.
    /// Only authenticated journals qualify; unsealed sessions fail closed.
    pub fn journal_state(&self, operation_id: OperationId) -> Result<JournalOperationState> {
        let journal = self.journal.as_ref().ok_or(ToolError::InvalidJournal)?;
        let journal = journal.lock().map_err(|_| ToolError::InvalidJournal)?;
        journal.authenticated_head()?;
        state_from_history(journal.history(), operation_id)
    }

    /// Check the exact digest and principal before disclosing a known status.
    pub fn journal_state_bound(
        &self,
        operation_id: OperationId,
        digest: &str,
        context: OperationContext,
    ) -> Result<JournalOperationState> {
        let state = self.journal_state(operation_id)?;
        state.verify_binding(digest, context)?;
        Ok(state)
    }

    /// Current authenticated chain head for storage in a separate trusted
    /// checkpoint. Only an independently retained head can detect deletion
    /// of an otherwise valid *complete suffix* of signed records.
    pub fn authenticated_journal_head(&self) -> Result<AuthenticatedJournalHead> {
        let journal = self.journal.as_ref().ok_or(ToolError::InvalidJournal)?;
        journal
            .lock()
            .map_err(|_| ToolError::InvalidJournal)?
            .authenticated_head()
    }

    /// Reopen an existing log for a read-only, fully authenticated lookup.
    /// This does not acquire/create the writer lock and can inspect a log
    /// whose live writer still owns it. A partial concurrent append fails
    /// closed; callers may retry after the writer finishes. To detect valid
    /// suffix removal, pass a chain head saved independently by the host.
    pub fn lookup_authenticated_journal(
        root: impl AsRef<Path>,
        journal_path: impl AsRef<Path>,
        authentication_key: [u8; 32],
        operation_id: OperationId,
        digest: &str,
        context: OperationContext,
        trusted_head: Option<&AuthenticatedJournalHead>,
    ) -> Result<JournalOperationState> {
        let root = WorkspaceReader::new(root, 1)?;
        let verified =
            Journal::inspect_authenticated(journal_path.as_ref(), root.root(), authentication_key)?;
        if trusted_head.is_some_and(|expected| expected != &verified.head) {
            return Err(ToolError::InvalidJournal);
        }
        let state = state_from_history(&verified.history, operation_id)?;
        state.verify_binding(digest, context)?;
        Ok(state)
    }

    pub fn root(&self) -> &Path {
        self.reader.root()
    }

    /// Preparing confers no privileges; the host should display this exact
    /// scope and digest before calling approve as a result of user consent.
    pub fn prepare(
        &self,
        context: OperationContext,
        request: OperationRequest,
    ) -> Result<PreparedOperation> {
        let (action, resource, resolved) = match request {
            OperationRequest::Read { path } => {
                let file = self.reader.canonical_resource(path)?;
                // Bind approval to the exact bytes observed at preparation.
                // Nothing is returned to the renderer until a separate native
                // confirmation and the same SHA has been checked at dispatch.
                let content = self.reader.read_candidate(&file)?;
                if std::str::from_utf8(&content).is_err() {
                    return Err(ToolError::NonTextFile);
                }
                let sha256 = format!("{:x}", Sha256::digest(&content));
                let bytes = content.len();
                (
                    ToolAction::ReadFile,
                    file.clone(),
                    Resolved::Read {
                        file,
                        sha256,
                        bytes,
                    },
                )
            }
            OperationRequest::Write { path, bytes, mode } => {
                if bytes.len() as u64 > self.reader_limit() || bytes.is_empty() {
                    return Err(ToolError::InvalidWrite);
                }
                if let WriteMode::ReplaceIfSha256(expected) = &mode {
                    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(ToolError::InvalidWrite);
                    }
                }
                let file = self.write_target(&path)?;
                let parent_identity =
                    verified_open::pin_parent(self.reader.root(), &path)?.identity();
                (
                    ToolAction::WriteFile,
                    file.clone(),
                    Resolved::Write {
                        file,
                        bytes,
                        mode,
                        parent_identity,
                    },
                )
            }
            OperationRequest::Process {
                executable,
                working_dir,
                args,
                timeout,
            } => {
                let executable = executable_file(&executable)?;
                let executable_sha256 = hash_executable(&executable)?;
                let cwd = self.workspace_dir(&working_dir)?;
                let cwd_identity = verified_open::pin_parent(
                    self.reader.root(),
                    &cwd.strip_prefix(self.reader.root())
                        .map_err(|_| ToolError::ScopeEscape)?
                        .join(".constellation-cwd-pin"),
                )?
                .identity();
                validate_args(&args, timeout)?;
                (
                    ToolAction::ExecuteCommand,
                    executable.clone(),
                    Resolved::Command {
                        executable,
                        executable_sha256,
                        cwd,
                        cwd_identity,
                        argv: args,
                        timeout,
                        git: false,
                    },
                )
            }
            OperationRequest::Git {
                executable,
                repository,
                action,
                timeout,
            } => {
                let executable = executable_file(&executable)?;
                let executable_sha256 = hash_executable(&executable)?;
                let cwd = self.workspace_dir(&repository)?;
                let cwd_identity = verified_open::pin_parent(
                    self.reader.root(),
                    &cwd.strip_prefix(self.reader.root())
                        .map_err(|_| ToolError::ScopeEscape)?
                        .join(".constellation-cwd-pin"),
                )?
                .identity();
                if !cwd.join(".git").exists() {
                    return Err(ToolError::InvalidCommand);
                }
                validate_git_configuration(&cwd, matches!(action, GitAction::Push { .. }))?;
                let (permission, argv) = git_args(&action)?;
                validate_args(&argv, timeout)?;
                (
                    permission,
                    executable.clone(),
                    Resolved::Command {
                        executable,
                        executable_sha256,
                        cwd,
                        cwd_identity,
                        argv,
                        timeout,
                        git: true,
                    },
                )
            }
        };
        let resource = resource
            .to_str()
            .ok_or(ToolError::InvalidRelativePath)?
            .to_owned();
        // Command permissions are bound to their exact invocation, not merely
        // an executable that could be reused with different arguments or cwd.
        // The preview carries the complete argv for human review; the grant
        // holds a fingerprint to avoid leaking sensitive arguments into logs.
        let resource = if action == ToolAction::GitPush {
            let Resolved::Command { cwd, argv, .. } = &resolved else {
                return Err(ToolError::InvalidCommand);
            };
            let remote = argv
                .get(argv.len().saturating_sub(2))
                .ok_or(ToolError::InvalidCommand)?;
            let reference = argv.last().ok_or(ToolError::InvalidCommand)?;
            format!("git-push executable={resource:?} repository={cwd:?} remote={remote:?} ref={reference:?}")
        } else if let Resolved::Command {
            cwd,
            argv,
            timeout,
            git,
            ..
        } = &resolved
        {
            let mut arguments_hash = Sha256::new();
            for arg in argv {
                feed(&mut arguments_hash, arg.as_bytes());
            }
            format!(
                "command executable={resource:?} cwd={cwd:?} args_sha256={:x} timeout_ns={} git={git}",
                arguments_hash.finalize(),
                timeout.as_nanos(),
            )
        } else {
            resource
        };
        let scope = PermissionScope {
            agent_id: context.agent_id,
            room_id: context.room_id,
            task_id: context.task_id,
            action,
            resource,
        };
        let digest = operation_digest(&scope, &resolved);
        Ok(PreparedOperation {
            scope,
            digest,
            resolved,
        })
    }

    /// This call MUST be made only by trusted host code after an explicit
    /// user approval, not by an agent or frontend-supplied boolean.
    pub fn approve(
        &self,
        gate: &PermissionGate,
        operation_id: OperationId,
        operation: &PreparedOperation,
    ) -> Result<OperationApproval> {
        ensure_permission(gate, &operation.scope)?;
        let mut approvals = self
            .approvals
            .lock()
            .map_err(|_| ToolError::ApprovalMismatch)?;
        if approvals.contains_key(&operation_id) {
            return Err(ToolError::OperationReplay);
        }
        if let Some(journal) = &self.journal {
            journal
                .lock()
                .map_err(|_| ToolError::InvalidJournal)?
                .approve(
                    operation_id,
                    &operation.digest,
                    OperationContext {
                        agent_id: operation.scope.agent_id,
                        room_id: operation.scope.room_id,
                        task_id: operation.scope.task_id,
                    },
                )?;
        }
        approvals.insert(
            operation_id,
            ApprovalRecord {
                digest: operation.digest.clone(),
                consumed: false,
                receipt: None,
            },
        );
        Ok(OperationApproval {
            operation_id,
            digest: operation.digest.clone(),
        })
    }

    /// Record an explicit user rejection against this exact prepared action.
    /// The operation ID becomes unusable even after reopening a durable log;
    /// rejection never grants a permission scope or executes the action.
    /// Trusted host code must call this after receiving the user's decision.
    pub fn deny(&self, operation_id: OperationId, operation: &PreparedOperation) -> Result<()> {
        let mut approvals = self
            .approvals
            .lock()
            .map_err(|_| ToolError::ApprovalMismatch)?;
        if approvals.contains_key(&operation_id) {
            return Err(ToolError::OperationReplay);
        }
        if let Some(journal) = &self.journal {
            journal
                .lock()
                .map_err(|_| ToolError::InvalidJournal)?
                .deny(
                    operation_id,
                    &operation.digest,
                    OperationContext {
                        agent_id: operation.scope.agent_id,
                        room_id: operation.scope.room_id,
                        task_id: operation.scope.task_id,
                    },
                )?;
        }
        approvals.insert(
            operation_id,
            ApprovalRecord {
                digest: operation.digest.clone(),
                consumed: true,
                receipt: None,
            },
        );
        Ok(())
    }

    /// Durably reject a model-proposed read before inspecting its target.
    /// The lexical path and principal identify this denial for journal replay,
    /// but no stat, canonicalization, read, size or file-existence check occurs.
    /// Only the trusted host may invoke this on an independently denied read.
    /// The pathname itself is not persisted in the audit record.
    pub fn deny_unprepared_read(
        &self,
        operation_id: OperationId,
        context: OperationContext,
        relative_path: impl AsRef<Path>,
    ) -> Result<()> {
        let relative_path = relative_path.as_ref();
        let path = relative_path
            .to_str()
            .ok_or(ToolError::InvalidRelativePath)?;
        if path.is_empty()
            || path.len() > 1024
            || !path.is_ascii()
            || !relative_path
                .components()
                .all(|item| matches!(item, Component::Normal(_)))
        {
            return Err(ToolError::InvalidRelativePath);
        }
        // Length-delimited, domain-separated commitment; no filesystem read.
        // This is an audit identity, not a filesystem-content checksum or an
        // authorization token, and must not be accepted as an approval digest.
        let identity = serde_json::to_vec(&(
            "constellation-rejected-unprepared-read-v1",
            self.root().to_string_lossy().as_ref(),
            context.agent_id.to_string(),
            context.room_id.to_string(),
            context.task_id.to_string(),
            path,
        ))
        .map_err(|_| ToolError::InvalidRelativePath)?;
        let digest = format!("{:x}", Sha256::digest(&identity));
        let mut approvals = self
            .approvals
            .lock()
            .map_err(|_| ToolError::ApprovalMismatch)?;
        if approvals.contains_key(&operation_id) {
            return Err(ToolError::OperationReplay);
        }
        if let Some(journal) = &self.journal {
            journal
                .lock()
                .map_err(|_| ToolError::InvalidJournal)?
                .deny(operation_id, &digest, context)?;
        }
        approvals.insert(
            operation_id,
            ApprovalRecord {
                digest,
                consumed: true,
                receipt: None,
            },
        );
        Ok(())
    }

    /// Uses approval exactly once, even when permission was revoked, execution
    /// fails, or the attempt is cancelled. A matching attempted dispatch is
    /// recorded before checking the current grant, so regranting cannot revive
    /// a stale approval. Returns an attempt receipt plus optional output.
    pub fn execute(
        &self,
        gate: &PermissionGate,
        approval: &OperationApproval,
        operation: PreparedOperation,
        cancellation: &CancellationToken,
    ) -> Result<(ExecutionReceipt, Option<ExecutionOutput>)> {
        if approval.digest != operation.digest {
            // An unapproved *different* request is not a dispatch of this
            // approval. Preserve the distinct scope-denial response without
            // consuming the unrelated, correctly bound operation.
            ensure_permission(gate, &operation.scope)?;
            return Err(ToolError::ApprovalMismatch);
        }
        {
            let mut approvals = self
                .approvals
                .lock()
                .map_err(|_| ToolError::ApprovalMismatch)?;
            let record = approvals
                .get_mut(&approval.operation_id)
                .ok_or(ToolError::ApprovalMismatch)?;
            if record.digest != operation.digest {
                return Err(ToolError::ApprovalMismatch);
            }
            if record.consumed {
                return Err(ToolError::OperationReplay);
            }
            if let Some(journal) = &self.journal {
                journal
                    .lock()
                    .map_err(|_| ToolError::InvalidJournal)?
                    .start(approval.operation_id, &operation.digest)?;
            }
            record.consumed = true;
        }

        // A revoked grant must terminate this *attempt*, not leave the old
        // one-use approval pending until the user grants the scope again.
        // The Start record has already been fsynced (if journaling is enabled)
        // and the in-memory approval consumed before this authorization check.
        let outcome = match ensure_permission(gate, &operation.scope) {
            Err(error) => Err(error),
            Ok(()) if cancellation.is_cancelled() => Ok((OperationOutcome::Cancelled, None, None)),
            Ok(()) => match operation.resolved {
                Resolved::Read {
                    file,
                    sha256,
                    bytes,
                } => self
                    .reader
                    .read(
                        gate,
                        operation.scope.agent_id,
                        operation.scope.room_id,
                        operation.scope.task_id,
                        file.strip_prefix(self.reader.root())
                            .map_err(|_| ToolError::ScopeEscape)?,
                    )
                    .and_then(|data| {
                        if data.sha256 != sha256 || data.bytes_read != bytes {
                            return Err(ToolError::ReadContentConflict);
                        }
                        Ok((
                            OperationOutcome::Succeeded,
                            Some(ExecutionOutput::Read(data)),
                            None,
                        ))
                    }),
                Resolved::Write {
                    file,
                    bytes,
                    mode,
                    parent_identity,
                } => self
                    .write(&file, &bytes, &mode, parent_identity, cancellation)
                    .map(|data| {
                        (
                            OperationOutcome::Succeeded,
                            Some(ExecutionOutput::Write(data)),
                            None,
                        )
                    }),
                Resolved::Command {
                    executable,
                    executable_sha256,
                    cwd,
                    cwd_identity,
                    argv,
                    timeout,
                    git,
                } => {
                    // The workspace may have been changed since the user saw
                    // the preview. Refuse a moved or reparse-linked directory
                    // before opening a process with access to it.
                    let validated = cwd
                        .strip_prefix(self.reader.root())
                        .map_err(|_| ToolError::ScopeEscape)
                        .and_then(|relative| self.workspace_dir(relative))
                        .and_then(|current| {
                            if current == cwd {
                                Ok(())
                            } else {
                                Err(ToolError::ScopeEscape)
                            }
                        })
                        .and_then(|()| {
                            if git {
                                validate_git_configuration(
                                    &cwd,
                                    operation.scope.action == ToolAction::GitPush,
                                )
                            } else {
                                Ok(())
                            }
                        });
                    validated.and_then(|()| {
                        run_process(
                            executable,
                            executable_sha256,
                            cwd,
                            argv,
                            timeout,
                            git,
                            ProcessRunControl {
                                max_output: self.max_output_bytes,
                                cancel: cancellation,
                                workspace_root: self.reader.root(),
                                cwd_identity,
                                git_push: operation.scope.action == ToolAction::GitPush,
                            },
                        )
                    })
                }
            },
        };
        let (status, output, exit_code, failure_reason) = match outcome {
            Ok((status, output, code)) => {
                let reason = match status {
                    OperationOutcome::Succeeded => None,
                    OperationOutcome::Failed => {
                        Some("process returned a nonzero exit status".into())
                    }
                    OperationOutcome::Cancelled => Some("operation cancelled".into()),
                    OperationOutcome::TimedOut => Some("operation exceeded its deadline".into()),
                    OperationOutcome::OutputLimit => {
                        Some("process output exceeded its byte limit".into())
                    }
                };
                (status, output, code, reason)
            }
            Err(ToolError::Cancelled) => (
                OperationOutcome::Cancelled,
                None,
                None,
                Some("operation cancelled".into()),
            ),
            Err(ToolError::TimedOut) => (
                OperationOutcome::TimedOut,
                None,
                None,
                Some("operation exceeded its deadline".into()),
            ),
            Err(error) => (
                OperationOutcome::Failed,
                None,
                None,
                Some(error.to_string()),
            ),
        };
        let output_sha256 = match &output {
            Some(ExecutionOutput::Read(data)) => Some(data.sha256.clone()),
            Some(ExecutionOutput::Write(data)) => Some(data.sha256.clone()),
            Some(ExecutionOutput::Process(data)) => {
                let mut hasher = Sha256::new();
                hasher.update(&data.stdout);
                hasher.update(&data.stderr);
                Some(format!("{:x}", hasher.finalize()))
            }
            None => None,
        };
        let receipt = ExecutionReceipt {
            operation_id: approval.operation_id,
            digest: approval.digest.clone(),
            scope: operation.scope,
            outcome: status,
            exit_code,
            output_sha256,
            failure_reason,
        };
        let mut approvals = self
            .approvals
            .lock()
            .map_err(|_| ToolError::ApprovalMismatch)?;
        if let Some(journal) = &self.journal {
            journal
                .lock()
                .map_err(|_| ToolError::InvalidJournal)?
                .finish(&receipt)?;
        }
        if let Some(record) = approvals.get_mut(&approval.operation_id) {
            record.receipt = Some(receipt.clone());
        }
        Ok((receipt, output))
    }

    pub fn receipt(&self, operation_id: OperationId) -> Option<ExecutionReceipt> {
        self.approvals
            .lock()
            .ok()?
            .get(&operation_id)?
            .receipt
            .clone()
    }

    fn reader_limit(&self) -> u64 {
        self.reader.max_bytes()
    }

    fn workspace_dir(&self, relative: &Path) -> Result<PathBuf> {
        if !relative.as_os_str().is_empty() && relative != Path::new(".") {
            validate_relative(relative)?;
        }
        let absolute = self.reader.root().join(relative);
        deny_reparse_chain(self.reader.root(), relative)?;
        let canonical = fs::canonicalize(absolute)?;
        if !canonical.starts_with(self.reader.root()) {
            return Err(ToolError::ScopeEscape);
        }
        if !fs::metadata(&canonical)?.is_dir() {
            return Err(ToolError::InvalidRelativePath);
        }
        Ok(canonical)
    }

    fn write_target(&self, relative: &Path) -> Result<PathBuf> {
        validate_relative(relative)?;
        deny_reparse_chain(self.reader.root(), relative)?;
        let proposed = self.reader.root().join(relative);
        let parent = relative.parent().unwrap_or(Path::new(""));
        let canonical_parent = self.workspace_dir(parent)?;
        let target =
            canonical_parent.join(relative.file_name().ok_or(ToolError::InvalidRelativePath)?);
        if !target.starts_with(self.reader.root()) {
            return Err(ToolError::ScopeEscape);
        }
        if proposed.exists() {
            if !fs::metadata(&proposed)?.is_file() {
                return Err(ToolError::NotRegularFile);
            }
            let existing = fs::canonicalize(proposed)?;
            if existing != target {
                return Err(ToolError::ScopeEscape);
            }
        }
        Ok(target)
    }

    fn write(
        &self,
        file: &Path,
        bytes: &[u8],
        mode: &WriteMode,
        parent_identity: DirectoryIdentity,
        cancel: &CancellationToken,
    ) -> Result<WriteReceipt> {
        let relative = file
            .strip_prefix(self.reader.root())
            .map_err(|_| ToolError::ScopeEscape)?;
        if self.write_target(relative)? != file {
            return Err(ToolError::ScopeEscape);
        }
        // The directory must be the exact object present during approval.
        // Keep the handle alive through staging and publishing.
        let pinned = verified_open::pin_parent(self.reader.root(), relative)?;
        if pinned.identity() != parent_identity {
            return Err(ToolError::ScopeEscape);
        }
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        match mode {
            WriteMode::CreateNew => {
                // Stage and sync before publishing a name in the workspace.
                // A failed/cancelled write never exposes a partial target and
                // cannot delete an unrelated file placed there meanwhile.
                let mut staged = pinned.stage(file, bytes)?;
                let result = (|| -> Result<()> {
                    if self.write_target(relative)? != file {
                        return Err(ToolError::ScopeEscape);
                    }
                    if verified_open::pin_parent(self.reader.root(), relative)?.identity()
                        != parent_identity
                    {
                        return Err(ToolError::ScopeEscape);
                    }
                    if cancel.is_cancelled() {
                        return Err(ToolError::Cancelled);
                    }
                    // Hard-link creation is atomic and refuses all existing
                    // destination entries, including dangling symlinks.
                    pinned.publish_new(&staged, file)?;
                    Ok(())
                })();
                let _ = pinned.cleanup(&mut staged);
                result?;
            }
            WriteMode::ReplaceIfSha256(expected) => {
                if !file.exists() {
                    return Err(ToolError::ContentConflict);
                }
                let original = self.reader.read_candidate(file)?;
                if !format!("{:x}", Sha256::digest(original)).eq_ignore_ascii_case(expected) {
                    return Err(ToolError::ContentConflict);
                }
                if cancel.is_cancelled() {
                    return Err(ToolError::Cancelled);
                }
                let mut tmp = pinned.stage(file, bytes)?;
                let staged = (|| -> Result<()> {
                    // Re-check before replacement. A malicious concurrent local
                    // process can still race this check; OS handles are needed.
                    if self.write_target(relative)? != file
                        || !format!("{:x}", Sha256::digest(self.reader.read_candidate(file)?))
                            .eq_ignore_ascii_case(expected)
                    {
                        return Err(ToolError::ContentConflict);
                    }
                    if verified_open::pin_parent(self.reader.root(), relative)?.identity()
                        != parent_identity
                    {
                        return Err(ToolError::ScopeEscape);
                    }
                    if cancel.is_cancelled() {
                        return Err(ToolError::Cancelled);
                    }
                    pinned.publish_replace(&mut tmp, file)?;
                    Ok(())
                })();
                if staged.is_err() {
                    let _ = pinned.cleanup(&mut tmp);
                }
                staged?;
            }
        };
        Ok(WriteReceipt {
            canonical_file: file.to_path_buf(),
            bytes_written: bytes.len(),
            sha256: digest,
        })
    }
}

fn ensure_permission(gate: &PermissionGate, scope: &PermissionScope) -> Result<()> {
    gate.authorize(scope)
        .map_err(|_| ToolError::PermissionDenied)
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(ToolError::InvalidRelativePath);
    }
    Ok(())
}

fn deny_reparse_chain(root: &Path, relative: &Path) -> Result<()> {
    let mut candidate = root.to_path_buf();
    for segment in relative.components() {
        if let Component::Normal(name) = segment {
            candidate.push(name);
            match fs::symlink_metadata(&candidate) {
                Ok(meta) if is_reparse(&meta) => return Err(ToolError::SymlinkDenied),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn is_reparse(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

fn executable_file(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || is_reparse(&fs::symlink_metadata(path)?) {
        return Err(ToolError::InvalidCommand);
    }
    let canonical = fs::canonicalize(path)?;
    if !fs::metadata(&canonical)?.is_file() || canonical.to_str().is_none() {
        return Err(ToolError::InvalidCommand);
    }
    #[cfg(windows)]
    if !canonical
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        // Do not let Command invoke .cmd/.bat scripts through a shell.
        return Err(ToolError::InvalidCommand);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&canonical)?.permissions().mode() & 0o111 == 0 {
            return Err(ToolError::InvalidCommand);
        }
    }
    Ok(canonical)
}

/// Streams an executable's SHA-256 with bounded memory. The fingerprint is
/// shown at approval and checked again at dispatch. A privileged local process
/// may still race the final check and spawn, so this is not an OS sandbox.
fn hash_executable(path: &Path) -> Result<String> {
    let mut executable = fs::File::open(path)?;
    if !executable.metadata()?.is_file() {
        return Err(ToolError::InvalidCommand);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 32768];
    loop {
        let count = executable.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_bounded(file: &Path, limit: u64) -> Result<Vec<u8>> {
    let stream = fs::File::open(file)?;
    if stream.metadata()?.len() > limit {
        return Err(ToolError::FileTooLarge);
    }
    let mut bytes = Vec::new();
    stream.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ToolError::FileTooLarge);
    }
    Ok(bytes)
}

fn validate_args(args: &[String], timeout: Duration) -> Result<()> {
    if timeout.is_zero()
        || timeout > MAX_TIMEOUT
        || args.len() > MAX_COMMAND_ARGS
        || args.iter().map(String::len).sum::<usize>() > MAX_ARGUMENT_BYTES
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err(ToolError::InvalidCommand);
    }
    Ok(())
}

fn git_args(action: &GitAction) -> Result<(ToolAction, Vec<String>)> {
    // Git local configuration can still contain aliases and helper programs.
    // These commands use fixed subcommands and disable hooks and external diff.
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut args = vec![
        "--no-pager".into(),
        "-c".into(),
        format!("core.hooksPath={null_path}"),
        "-c".into(),
        "diff.external=".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        "credential.helper=".into(),
        "-c".into(),
        "protocol.ext.allow=never".into(),
        "-c".into(),
        "protocol.file.allow=never".into(),
        "-c".into(),
        "push.gpgSign=false".into(),
        "-c".into(),
        "push.recurseSubmodules=no".into(),
    ];
    let action_type = match action {
        GitAction::Status => {
            args.extend(["status".into(), "--porcelain=v1".into()]);
            ToolAction::ExecuteCommand
        }
        GitAction::Diff => {
            args.extend([
                "diff".into(),
                "--no-ext-diff".into(),
                "--no-textconv".into(),
            ]);
            ToolAction::ExecuteCommand
        }
        GitAction::Push { remote_url, branch } => {
            if !safe_git_name(branch) {
                return Err(ToolError::InvalidCommand);
            }
            let endpoint = Url::parse(remote_url).map_err(|_| ToolError::InvalidCommand)?;
            if endpoint.scheme() != "https"
                || endpoint.host_str().is_none()
                || !endpoint.username().is_empty()
                || endpoint.password().is_some()
                || endpoint.query().is_some()
                || endpoint.fragment().is_some()
            {
                return Err(ToolError::InvalidCommand);
            }
            args.extend([
                "push".into(),
                "--porcelain".into(),
                "--".into(),
                endpoint.to_string(),
                format!("refs/heads/{branch}:refs/heads/{branch}"),
            ]);
            ToolAction::GitPush
        }
    };
    Ok((action_type, args))
}

fn validate_git_configuration(repo: &Path, push: bool) -> Result<()> {
    // Git's local configuration may launch programs through filter.clean,
    // filter.process, diff drivers, credential helpers or includes even when
    // fixed argv, hooksPath, environment and external diff are sanitized.
    // Inspect before approval and immediately before dispatch. Fail closed on
    // executable-capable sections; ordinary core/branch/remote/user settings
    // remain usable. This is not a parser or race-proof filesystem sandbox.
    let git_dir = repo.join(".git");
    let config = git_dir.join("config");
    if !git_dir.is_dir() || is_reparse(&fs::symlink_metadata(&git_dir)?) {
        return Err(ToolError::InvalidCommand);
    }
    if !config.exists() && !push {
        return Ok(());
    }
    if is_reparse(&fs::symlink_metadata(&config)?) || !fs::metadata(&config)?.is_file() {
        return Err(ToolError::InvalidCommand);
    }
    let content = String::from_utf8(read_bounded(&config, 262_144)?)
        .map_err(|_| ToolError::InvalidCommand)?;
    let mut section = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            // No malformed/unknown section can hide an include or command.
            let header = header.split_once(']').ok_or(ToolError::InvalidCommand)?.0;
            section = header
                .split(|c: char| c.is_ascii_whitespace() || c == '.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(
                section.as_str(),
                "core"
                    | "user"
                    | "branch"
                    | "remote"
                    | "init"
                    | "gc"
                    | "maintenance"
                    | "extensions"
                    | "fetch"
                    | "push"
                    | "pack"
                    | "repack"
                    | "index"
                    | "status"
                    | "log"
                    | "commit"
            ) {
                return Err(ToolError::InvalidCommand);
            }
        } else {
            if section.is_empty() {
                return Err(ToolError::InvalidCommand);
            }
            let key = line
                .split(|c: char| c == '=' || c.is_ascii_whitespace())
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if key.is_empty()
                || (section == "core"
                    && matches!(
                        key.as_str(),
                        "worktree"
                            | "fsmonitor"
                            | "hookspath"
                            | "gitproxy"
                            | "sshcommand"
                            | "pager"
                            | "attributesfile"
                            | "excludesfile"
                            | "alternate"
                    ))
                || (section == "extensions" && key == "worktreeconfig")
                || (section == "remote"
                    && matches!(key.as_str(), "uploadpack" | "receivepack" | "vcs"))
            {
                return Err(ToolError::InvalidCommand);
            }
        }
    }
    Ok(())
}

fn safe_git_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.contains("//")
        && !value.ends_with('/')
        && !value.ends_with(".lock")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.'))
}

fn operation_digest(scope: &PermissionScope, resolved: &Resolved) -> String {
    let mut hasher = Sha256::new();
    feed(&mut hasher, b"constellation-operation-v1");
    for value in [
        scope.agent_id.to_string(),
        scope.room_id.to_string(),
        scope.task_id.to_string(),
        format!("{:?}", scope.action),
        scope.resource.clone(),
    ] {
        feed(&mut hasher, value.as_bytes());
    }
    match resolved {
        Resolved::Read {
            file,
            sha256,
            bytes,
        } => {
            feed(&mut hasher, b"read");
            feed(&mut hasher, file.to_str().unwrap_or("").as_bytes());
            feed(&mut hasher, sha256.as_bytes());
            feed(&mut hasher, &(*bytes as u64).to_le_bytes());
        }
        Resolved::Write {
            file,
            bytes,
            mode,
            parent_identity,
        } => {
            feed(&mut hasher, b"write");
            feed(&mut hasher, file.to_str().unwrap_or("").as_bytes());
            feed(&mut hasher, format!("{mode:?}").as_bytes());
            feed(&mut hasher, format!("{parent_identity:?}").as_bytes());
            feed(&mut hasher, bytes);
        }
        Resolved::Command {
            executable,
            executable_sha256,
            cwd,
            cwd_identity,
            argv,
            timeout,
            git,
        } => {
            feed(&mut hasher, if *git { b"git" } else { b"process" });
            feed(&mut hasher, executable.to_str().unwrap_or("").as_bytes());
            feed(&mut hasher, executable_sha256.as_bytes());
            feed(&mut hasher, cwd.to_str().unwrap_or("").as_bytes());
            feed(&mut hasher, format!("{cwd_identity:?}").as_bytes());
            feed(&mut hasher, &timeout.as_nanos().to_le_bytes());
            for argument in argv {
                feed(&mut hasher, argument.as_bytes());
            }
        }
    }
    format!("{:x}", hasher.finalize())
}

fn feed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

struct ProcessRunControl<'a> {
    max_output: usize,
    cancel: &'a CancellationToken,
    workspace_root: &'a Path,
    cwd_identity: DirectoryIdentity,
    git_push: bool,
}

fn run_process(
    executable: PathBuf,
    executable_sha256: String,
    cwd: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    git: bool,
    control: ProcessRunControl<'_>,
) -> Result<(OperationOutcome, Option<ExecutionOutput>, Option<i32>)> {
    let ProcessRunControl {
        max_output,
        cancel,
        workspace_root,
        cwd_identity,
        git_push,
    } = control;
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    if executable_file(&executable)? != executable
        || hash_executable(&executable)? != executable_sha256
    {
        return Err(ToolError::InvalidCommand);
    }
    // GitAction must never be dispatched through an arbitrary approved
    // interpreter/binary posing as Git. This filename check is intentionally
    // narrow: it prevents an obvious confused-deputy invocation but is not a
    // signature check or proof that a file named git is trustworthy.
    if git && !git_executable_name(&executable) {
        return Err(ToolError::InvalidCommand);
    }
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    // Pin the *directory object* originally approved, not only its pathname.
    // Windows holds non-FILE_SHARE_DELETE handles for every ancestor through
    // CreateProcess; Unix uses an O_NOFOLLOW fd in the child via fchdir.
    let cwd_relative = cwd
        .strip_prefix(workspace_root)
        .map_err(|_| ToolError::ScopeEscape)?;
    let pinned_cwd =
        verified_open::pin_parent(workspace_root, &cwd_relative.join(".constellation-cwd-pin"))?;
    if pinned_cwd.identity() != cwd_identity {
        return Err(ToolError::ScopeEscape);
    }
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    let git_executable_parent = if git {
        Some(
            executable
                .parent()
                .ok_or(ToolError::InvalidCommand)?
                .to_path_buf(),
        )
    } else {
        None
    };
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    // Windows needs pathname cwd, but its entire ancestor chain is pinned.
    // Unix never consults a mutable cwd pathname when spawning the child.
    #[cfg(windows)]
    command.current_dir(cwd);
    if git {
        // Never pass the caller's PATH to Git: its subprocess helpers would
        // otherwise be resolved through attacker-supplied ambient directories.
        // Constrain helper lookup to the reviewed executable's own directory.
        command.env(
            "PATH",
            git_executable_parent.ok_or(ToolError::InvalidCommand)?,
        );
    } else if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    if cfg!(windows) {
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    if git {
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_ALLOW_PROTOCOL", if git_push { "https" } else { "" })
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_COUNT", "0")
            .env("GCM_INTERACTIVE", "never");
    }
    #[cfg(windows)]
    let job = windows_process::ProcessJob::create()?;
    #[cfg(windows)]
    let mut child = job.spawn_suspended_then_assign(&mut command)?;
    #[cfg(unix)]
    let (mut child, mut process_group) =
        unix_process::UnixProcessGroup::spawn(&mut command, &pinned_cwd)?;
    #[cfg(not(any(windows, unix)))]
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or(ToolError::InvalidCommand)?;
    let stderr = child.stderr.take().ok_or(ToolError::InvalidCommand)?;
    let (sender, receiver) = mpsc::channel();
    let output_exceeded = Arc::new(AtomicBool::new(false));
    for (index, mut stream) in [
        (0, Box::new(stdout) as Box<dyn Read + Send>),
        (1, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let sender = sender.clone();
        let output_exceeded = output_exceeded.clone();
        thread::spawn(move || {
            let mut output = Vec::new();
            let mut oversized = false;
            let mut buffer = [0_u8; 8192];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if output.len().saturating_add(read) > max_output {
                            oversized = true;
                            output_exceeded.store(true, Ordering::Release);
                        }
                        let available = max_output.saturating_sub(output.len()).min(read);
                        output.extend_from_slice(&buffer[..available]);
                    }
                    Err(_) => break,
                }
            }
            let _ = sender.send((index, output, oversized));
        });
    }
    drop(sender);
    let start = Instant::now();
    let (outcome, exit_code) = loop {
        if output_exceeded.load(Ordering::Acquire) {
            terminate_process(
                &mut child,
                #[cfg(windows)]
                &job,
                #[cfg(unix)]
                &mut process_group,
            )?;
            break (OperationOutcome::OutputLimit, None);
        }
        if cancel.is_cancelled() {
            terminate_process(
                &mut child,
                #[cfg(windows)]
                &job,
                #[cfg(unix)]
                &mut process_group,
            )?;
            break (OperationOutcome::Cancelled, None);
        }
        if start.elapsed() >= timeout {
            terminate_process(
                &mut child,
                #[cfg(windows)]
                &job,
                #[cfg(unix)]
                &mut process_group,
            )?;
            break (OperationOutcome::TimedOut, None);
        }
        #[cfg(unix)]
        let parent_exited = process_group.parent_exited_without_reaping()?;
        #[cfg(not(unix))]
        let parent_exit_status = child.try_wait()?;
        #[cfg(unix)]
        if parent_exited {
            // Holding the direct child as an unreaped zombie pins its PID,
            // preventing another process group from reusing that PGID.
            process_group.terminate()?;
            let status = child.wait()?;
            break (
                if status.success() {
                    OperationOutcome::Succeeded
                } else {
                    OperationOutcome::Failed
                },
                status.code(),
            );
        }
        #[cfg(not(unix))]
        if let Some(status) = parent_exit_status {
            // A successfully exited direct child may leave grandchildren and
            // inherited pipe writers alive; reap the entire Windows job.
            #[cfg(windows)]
            job.terminate()?;
            break (
                if status.success() {
                    OperationOutcome::Succeeded
                } else {
                    OperationOutcome::Failed
                },
                status.code(),
            );
        }
        thread::sleep(POLL);
    };
    let remaining = timeout
        .saturating_sub(start.elapsed())
        .max(Duration::from_millis(100));
    let mut outputs = [Vec::new(), Vec::new()];
    let mut oversized = false;
    for _ in 0..2 {
        match receiver.recv_timeout(remaining) {
            Ok((index, output, exceeded)) => {
                outputs[index] = output;
                oversized |= exceeded;
            }
            Err(_) => return Ok((OperationOutcome::TimedOut, None, exit_code)),
        }
    }
    let outcome = if oversized {
        OperationOutcome::OutputLimit
    } else {
        outcome
    };
    Ok((
        outcome,
        Some(ExecutionOutput::Process(ProcessOutput {
            stdout: std::mem::take(&mut outputs[0]),
            stderr: std::mem::take(&mut outputs[1]),
            exit_code,
        })),
        exit_code,
    ))
}

fn git_executable_name(executable: &Path) -> bool {
    let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if cfg!(windows) {
        name.eq_ignore_ascii_case("git.exe")
    } else {
        name == "git"
    }
}

fn terminate_process(
    child: &mut Child,
    #[cfg(windows)] job: &windows_process::ProcessJob,
    #[cfg(unix)] process_group: &mut unix_process::UnixProcessGroup,
) -> Result<()> {
    #[cfg(windows)]
    {
        if let Err(error) = job.terminate() {
            // KILL_ON_JOB_CLOSE still applies on drop; best-effort direct kill
            // is only fallback. Never return a successful cleanup receipt.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    }
    #[cfg(unix)]
    process_group.terminate()?;
    #[cfg(not(any(windows, unix)))]
    {
        let _ = child.kill();
    }
    child.wait()?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn replace_file(from: &Path, to: &Path) -> Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *const c_void,
            reserved: *const c_void,
        ) -> i32;
    }
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let target = wide(to);
    let replacement = wide(from);
    // ReplaceFileW atomically substitutes file data on the same volume.
    let success = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if success == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
