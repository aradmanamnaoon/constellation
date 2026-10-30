use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{AgentId, CoreError, Result, RoomId, TaskId};

/// A requested capability. Tool runners must authorize each invocation with its
/// complete, canonical resource identity before performing any side effect.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum ToolAction {
    ReadFile,
    WriteFile,
    DeleteFile,
    ExecuteCommand,
    HttpRequest,
    DesktopControl,
    GitPush,
    InstallPackage,
    Custom(String),
}

/// An exact permission for one agent and task in one room and one resource.
/// There are no wildcards or implicit approvals.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionScope {
    pub agent_id: AgentId,
    pub room_id: RoomId,
    pub task_id: TaskId,
    pub action: ToolAction,
    pub resource: String,
}

/// Volatile explicit approvals. Permissions are never loaded implicitly from
/// disk, and constructing the gate grants nothing.
#[derive(Debug, Default)]
pub struct PermissionGate {
    grants: HashSet<PermissionScope>,
}

impl PermissionGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(&mut self, scope: PermissionScope) -> Result<()> {
        if scope.resource.trim().is_empty() {
            return Err(CoreError::InvalidInput("empty permission resource".into()));
        }
        self.grants.insert(scope);
        Ok(())
    }

    pub fn revoke(&mut self, scope: &PermissionScope) -> bool {
        self.grants.remove(scope)
    }

    pub fn authorize(&self, request: &PermissionScope) -> Result<()> {
        if self.grants.contains(request) {
            Ok(())
        } else {
            Err(CoreError::PermissionDenied)
        }
    }
}
