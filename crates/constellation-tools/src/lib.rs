//! Local, read-only filesystem inspection tool with explicit per-file grants.
//!
//! This is a library slice, not a complete sandbox. In particular, it is not
//! wired to the app, does not execute shell commands, and is not race-proof
//! against a malicious local process swapping paths between validation and
//! open. Use OS-native handle-based isolation before enabling unattended file
//! access or files writable by an adversary.

use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

use constellation_core::{AgentId, PermissionGate, PermissionScope, RoomId, TaskId, ToolAction};
use sha2::{Digest, Sha256};

mod execution;
pub use execution::{
    CancellationToken, ExecutionOutput, ExecutionReceipt, GitAction, LocalExecutor,
    OperationApproval, OperationContext, OperationDetails, OperationOutcome, OperationPreview,
    OperationRequest, PreparedOperation, ProcessOutput, WriteMode, WriteReceipt,
};

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("workspace root must be an existing directory")]
    BadWorkspaceRoot,
    #[error("file path must be a normalized path relative to the selected workspace")]
    InvalidRelativePath,
    #[error("path escapes the selected workspace")]
    ScopeEscape,
    #[error("symlink traversal is not allowed")]
    SymlinkDenied,
    #[error("file access was not explicitly approved for this agent and task")]
    PermissionDenied,
    #[error("requested entry is not a regular file")]
    NotRegularFile,
    #[error("file exceeds the configured byte limit")]
    FileTooLarge,
    #[error("file is not valid UTF-8 text")]
    NonTextFile,
    #[error("filesystem failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid executable, working directory or arguments")]
    InvalidCommand,
    #[error("invalid write target or content")]
    InvalidWrite,
    #[error("target file already exists")]
    FileAlreadyExists,
    #[error("existing file changed since write approval")]
    ContentConflict,
    #[error("approval was not found or differs from the requested action")]
    ApprovalMismatch,
    #[error("operation ID was already approved or used")]
    OperationReplay,
    #[error("operation was cancelled")]
    Cancelled,
    #[error("operation exceeded the deadline")]
    TimedOut,
}

pub type Result<T> = std::result::Result<T, ToolError>;

/// A scoped, immutable snapshot of a text file. The receipt can later be
/// attached to an auditable task, but it is not a claim that one was logged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadReceipt {
    pub canonical_file: PathBuf,
    pub bytes_read: usize,
    pub sha256: String,
    pub content: String,
}

pub struct WorkspaceReader {
    root: PathBuf,
    max_bytes: u64,
}

impl WorkspaceReader {
    /// Only an already existing directory may be chosen as the workspace.
    pub fn new(root: impl AsRef<Path>, max_bytes: u64) -> Result<Self> {
        let root = root.as_ref();
        let attributes = fs::symlink_metadata(root).map_err(|_| ToolError::BadWorkspaceRoot)?;
        if !attributes.is_dir() || execution::is_reparse(&attributes) || max_bytes == 0 {
            return Err(ToolError::BadWorkspaceRoot);
        }
        let root = fs::canonicalize(root).map_err(|_| ToolError::BadWorkspaceRoot)?;
        Ok(Self { root, max_bytes })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Calculate the exact resource for a *future explicit* grant. This only
    /// identifies the requested file and confers no permission on its own.
    pub fn canonical_resource(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let relative = relative.as_ref();
        if relative.as_os_str().is_empty()
            || !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(ToolError::InvalidRelativePath);
        }
        let requested = self.root.join(relative);
        let canonical = fs::canonicalize(&requested)?;
        if !canonical.starts_with(&self.root) {
            return Err(ToolError::ScopeEscape);
        }
        // A path pointing through a symlink is denied even when the resolved
        // destination would still be contained in the workspace.
        let mut candidate = self.root.clone();
        for part in relative.components() {
            if let Component::Normal(name) = part {
                candidate.push(name);
                if execution::is_reparse(&fs::symlink_metadata(&candidate)?) {
                    return Err(ToolError::SymlinkDenied);
                }
            }
        }
        if !fs::metadata(&canonical)?.is_file() {
            return Err(ToolError::NotRegularFile);
        }
        Ok(canonical)
    }

    /// Reads UTF-8 text only after matching the full exact-scope permission.
    /// `read` never opens a file on an unmatched or missing grant.
    pub fn read(
        &self,
        gate: &PermissionGate,
        agent_id: AgentId,
        room_id: RoomId,
        task_id: TaskId,
        relative: impl AsRef<Path>,
    ) -> Result<ReadReceipt> {
        let canonical = self.canonical_resource(relative)?;
        let scope = PermissionScope {
            agent_id,
            room_id,
            task_id,
            action: ToolAction::ReadFile,
            resource: canonical.to_string_lossy().into_owned(),
        };
        gate.authorize(&scope)
            .map_err(|_| ToolError::PermissionDenied)?;
        let mut file = File::open(&canonical)?;
        if !file.metadata()?.is_file() {
            return Err(ToolError::NotRegularFile);
        }
        if file.metadata()?.len() > self.max_bytes {
            return Err(ToolError::FileTooLarge);
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(self.max_bytes + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > self.max_bytes {
            return Err(ToolError::FileTooLarge);
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let content = String::from_utf8(bytes).map_err(|_| ToolError::NonTextFile)?;
        Ok(ReadReceipt {
            canonical_file: canonical,
            bytes_read: content.len(),
            sha256: hash,
            content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_deny_then_exact_permission_and_checksum() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("source.rs"), "fn main() {}\n").unwrap();
        let reader = WorkspaceReader::new(dir.path(), 4096).unwrap();
        let (agent_id, room_id, task_id) = (AgentId::new(), RoomId::new(), TaskId::new());
        let mut gate = PermissionGate::new();
        assert!(matches!(
            reader.read(&gate, agent_id, room_id, task_id, "source.rs"),
            Err(ToolError::PermissionDenied)
        ));
        let file = reader.canonical_resource("source.rs").unwrap();
        gate.grant(PermissionScope {
            agent_id,
            room_id,
            task_id,
            action: ToolAction::ReadFile,
            resource: file.to_string_lossy().into_owned(),
        })
        .unwrap();
        let receipt = reader
            .read(&gate, agent_id, room_id, task_id, "source.rs")
            .unwrap();
        assert_eq!(receipt.content, "fn main() {}\n");
        assert_eq!(receipt.bytes_read, 13);
        assert_eq!(receipt.sha256.len(), 64);
        assert!(matches!(
            reader.read(&gate, AgentId::new(), room_id, task_id, "source.rs"),
            Err(ToolError::PermissionDenied)
        ));
        assert!(matches!(
            reader.read(&gate, agent_id, room_id, TaskId::new(), "source.rs"),
            Err(ToolError::PermissionDenied)
        ));
    }

    #[test]
    fn traversal_absolute_path_and_directories_are_denied() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("nested").join("ok.txt"), "ok").unwrap();
        let reader = WorkspaceReader::new(dir.path(), 4096).unwrap();
        assert!(matches!(
            reader.canonical_resource("../outside.txt"),
            Err(ToolError::InvalidRelativePath)
        ));
        assert!(matches!(
            reader.canonical_resource(dir.path()),
            Err(ToolError::InvalidRelativePath)
        ));
        assert!(matches!(
            reader.canonical_resource("nested"),
            Err(ToolError::NotRegularFile)
        ));
        assert!(reader.canonical_resource("nested/ok.txt").is_ok());
    }

    #[test]
    fn oversized_and_nontext_are_denied_even_when_granted() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("binary"), [0xff, 0xfe, 0xfd]).unwrap();
        fs::write(dir.path().join("large"), "1234567890").unwrap();
        let reader = WorkspaceReader::new(dir.path(), 5).unwrap();
        let (agent_id, room_id, task_id) = (AgentId::new(), RoomId::new(), TaskId::new());
        let mut gate = PermissionGate::new();
        for name in ["binary", "large"] {
            let canonical = reader.canonical_resource(name).unwrap();
            gate.grant(PermissionScope {
                agent_id,
                room_id,
                task_id,
                action: ToolAction::ReadFile,
                resource: canonical.to_string_lossy().into_owned(),
            })
            .unwrap();
        }
        assert!(matches!(
            reader.read(&gate, agent_id, room_id, task_id, "binary"),
            Err(ToolError::NonTextFile)
        ));
        assert!(matches!(
            reader.read(&gate, agent_id, room_id, task_id, "large"),
            Err(ToolError::FileTooLarge)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_rejected_even_when_pointing_inside_root() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), "ok").unwrap();
        symlink("real.txt", dir.path().join("alias.txt")).unwrap();
        let reader = WorkspaceReader::new(dir.path(), 4096).unwrap();
        assert!(matches!(
            reader.canonical_resource("alias.txt"),
            Err(ToolError::SymlinkDenied)
        ));
    }
}
