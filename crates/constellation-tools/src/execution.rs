//! Explicitly approved local effects. Host code owns the approval decision;
//! model output must never call `approve` directly.
//!
//! The permission gate and one-shot approval are independent requirements.
//! Receipts are in memory: an integrating host must persist them in its event
//! store, together with approvals, before advertising crash-safe audit.
//! This crate is not an OS sandbox. Path swaps by another privileged process,
//! child process trees and process network egress require additional OS policy.

use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
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
use sha2::{Digest, Sha256};
use url::Url;

use crate::{ReadReceipt, Result, ToolError, WorkspaceReader};

const MAX_COMMAND_ARGS: usize = 128;
const MAX_ARGUMENT_BYTES: usize = 32_768;
const MAX_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
/// through PATH. Git uses fixed argv with hooks and external diff disabled.
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
    },
    Write {
        file: PathBuf,
        bytes: Vec<u8>,
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
            Resolved::Read { file } => OperationDetails::Read {
                canonical_file: file.clone(),
            },
            Resolved::Write { file, bytes, mode } => OperationDetails::Write {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    OutputLimit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionReceipt {
    pub operation_id: OperationId,
    pub digest: String,
    pub scope: PermissionScope,
    pub outcome: OperationOutcome,
    pub exit_code: Option<i32>,
    pub output_sha256: Option<String>,
    pub failure_reason: Option<String>,
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
        })
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
                (ToolAction::ReadFile, file.clone(), Resolved::Read { file })
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
                (
                    ToolAction::WriteFile,
                    file.clone(),
                    Resolved::Write { file, bytes, mode },
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
                validate_args(&args, timeout)?;
                (
                    ToolAction::ExecuteCommand,
                    executable.clone(),
                    Resolved::Command {
                        executable,
                        executable_sha256,
                        cwd,
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
                if !cwd.join(".git").exists() {
                    return Err(ToolError::InvalidCommand);
                }
                if matches!(action, GitAction::Push { .. }) {
                    validate_git_push_config(&cwd)?;
                }
                let (permission, argv) = git_args(&action)?;
                validate_args(&argv, timeout)?;
                (
                    permission,
                    executable.clone(),
                    Resolved::Command {
                        executable,
                        executable_sha256,
                        cwd,
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
        // Push grants are deliberately narrower than general execute grants:
        // the exact repository, remote and ref are part of the permission
        // resource in addition to the one-use approval digest.
        let resource = if action == ToolAction::GitPush {
            let Resolved::Command { cwd, argv, .. } = &resolved else {
                return Err(ToolError::InvalidCommand);
            };
            let remote = argv
                .get(argv.len().saturating_sub(2))
                .ok_or(ToolError::InvalidCommand)?;
            let reference = argv.last().ok_or(ToolError::InvalidCommand)?;
            format!("git-push executable={resource:?} repository={cwd:?} remote={remote:?} ref={reference:?}")
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

    /// Uses approval exactly once, even when execution fails or is cancelled.
    /// Returns an attempt receipt plus optional output on all dispatched paths.
    pub fn execute(
        &self,
        gate: &PermissionGate,
        approval: &OperationApproval,
        operation: PreparedOperation,
        cancellation: &CancellationToken,
    ) -> Result<(ExecutionReceipt, Option<ExecutionOutput>)> {
        ensure_permission(gate, &operation.scope)?;
        if approval.digest != operation.digest {
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
            record.consumed = true;
        }

        let outcome = if cancellation.is_cancelled() {
            Ok((OperationOutcome::Cancelled, None, None))
        } else {
            match operation.resolved {
                Resolved::Read { file } => self
                    .reader
                    .read(
                        gate,
                        operation.scope.agent_id,
                        operation.scope.room_id,
                        operation.scope.task_id,
                        file.strip_prefix(self.reader.root())
                            .map_err(|_| ToolError::ScopeEscape)?,
                    )
                    .map(|data| {
                        (
                            OperationOutcome::Succeeded,
                            Some(ExecutionOutput::Read(data)),
                            None,
                        )
                    }),
                Resolved::Write { file, bytes, mode } => {
                    self.write(&file, &bytes, &mode, cancellation).map(|data| {
                        (
                            OperationOutcome::Succeeded,
                            Some(ExecutionOutput::Write(data)),
                            None,
                        )
                    })
                }
                Resolved::Command {
                    executable,
                    executable_sha256,
                    cwd,
                    argv,
                    timeout,
                    git,
                } => {
                    let validated = if operation.scope.action == ToolAction::GitPush {
                        validate_git_push_config(&cwd)
                    } else {
                        Ok(())
                    };
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
                            },
                        )
                    })
                }
            }
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
        cancel: &CancellationToken,
    ) -> Result<WriteReceipt> {
        let relative = file
            .strip_prefix(self.reader.root())
            .map_err(|_| ToolError::ScopeEscape)?;
        if self.write_target(relative)? != file {
            return Err(ToolError::ScopeEscape);
        }
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        match mode {
            WriteMode::CreateNew => {
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(file)
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::AlreadyExists {
                            ToolError::FileAlreadyExists
                        } else {
                            ToolError::Io(e)
                        }
                    })?;
                if let Err(error) = output.write_all(bytes).and_then(|()| output.sync_all()) {
                    drop(output);
                    let _ = fs::remove_file(file);
                    return Err(error.into());
                }
            }
            WriteMode::ReplaceIfSha256(expected) => {
                if !file.exists() {
                    return Err(ToolError::ContentConflict);
                }
                let original = read_bounded(file, self.reader_limit())?;
                if !format!("{:x}", Sha256::digest(original)).eq_ignore_ascii_case(expected) {
                    return Err(ToolError::ContentConflict);
                }
                if cancel.is_cancelled() {
                    return Err(ToolError::Cancelled);
                }
                let tmp =
                    file.with_extension(format!("constellation-{}.tmp", uuid::Uuid::new_v4()));
                let staged = (|| -> Result<()> {
                    let mut output = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
                    output.write_all(bytes)?;
                    output.sync_all()?;
                    drop(output);
                    // Re-check before replacement. A malicious concurrent local
                    // process can still race this check; OS handles are needed.
                    if self.write_target(relative)? != file
                        || !format!(
                            "{:x}",
                            Sha256::digest(read_bounded(file, self.reader_limit())?)
                        )
                        .eq_ignore_ascii_case(expected)
                    {
                        return Err(ToolError::ContentConflict);
                    }
                    if cancel.is_cancelled() {
                        return Err(ToolError::Cancelled);
                    }
                    replace_file(&tmp, file)?;
                    Ok(())
                })();
                if staged.is_err() {
                    let _ = fs::remove_file(&tmp);
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

fn validate_git_push_config(repo: &Path) -> Result<()> {
    // Reject local URL rewrite and include directives before an outbound push.
    // User-approved URL is passed directly to Git (not an alias). A malicious
    // concurrent process can still race .git/config; callers must isolate
    // untrusted worktrees before permitting an irreversible push.
    let git_dir = repo.join(".git");
    let config = git_dir.join("config");
    if !git_dir.is_dir()
        || is_reparse(&fs::symlink_metadata(&git_dir)?)
        || is_reparse(&fs::symlink_metadata(&config)?)
    {
        return Err(ToolError::InvalidCommand);
    }
    let content = String::from_utf8(read_bounded(&config, 262_144)?)
        .map_err(|_| ToolError::InvalidCommand)?;
    let lowered = content.to_ascii_lowercase();
    if lowered.contains("insteadof")
        || lowered.contains("[url ")
        || lowered.contains("[include")
        || lowered.contains("worktreeconfig")
    {
        return Err(ToolError::InvalidCommand);
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
        Resolved::Read { file } => {
            feed(&mut hasher, b"read");
            feed(&mut hasher, file.to_str().unwrap_or("").as_bytes());
        }
        Resolved::Write { file, bytes, mode } => {
            feed(&mut hasher, b"write");
            feed(&mut hasher, file.to_str().unwrap_or("").as_bytes());
            feed(&mut hasher, format!("{mode:?}").as_bytes());
            feed(&mut hasher, bytes);
        }
        Resolved::Command {
            executable,
            executable_sha256,
            cwd,
            argv,
            timeout,
            git,
        } => {
            feed(&mut hasher, if *git { b"git" } else { b"process" });
            feed(&mut hasher, executable.to_str().unwrap_or("").as_bytes());
            feed(&mut hasher, executable_sha256.as_bytes());
            feed(&mut hasher, cwd.to_str().unwrap_or("").as_bytes());
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
    let ProcessRunControl { max_output, cancel } = control;
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    if executable_file(&executable)? != executable
        || hash_executable(&executable)? != executable_sha256
    {
        return Err(ToolError::InvalidCommand);
    }
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    let mut command = Command::new(executable);
    command
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    if let Some(path) = std::env::var_os("PATH") {
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
            .env("GIT_ALLOW_PROTOCOL", "https");
    }
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
            let _ = child.kill();
            let _ = child.wait();
            break (OperationOutcome::OutputLimit, None);
        }
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break (OperationOutcome::Cancelled, None);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break (OperationOutcome::TimedOut, None);
        }
        if let Some(status) = child.try_wait()? {
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

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<()> {
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
