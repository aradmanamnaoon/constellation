//! Local-first data and safety boundaries for the Constellation desktop application.
//!
//! All state changes are committed with a durable SQLite event in one transaction.
//! No method in this crate executes a tool action or accesses a model provider.

mod error;
mod export;
mod models;
mod permissions;
mod provider;
mod store;

pub use error::{CoreError, Result};
pub use export::export_room_markdown;
pub use models::{
    Agent, AgentId, Event, EventKind, Message, MessageId, MessageSender, OperationId, ProviderId,
    Room, RoomId, StudioSnapshot, Task, TaskId, TaskState,
};
pub use permissions::{PermissionGate, PermissionScope, ToolAction};
pub use provider::{
    ChatCompletionRequest, ChatEntry, ChatRole, CredentialSource, OpenAiCompatibleAdapter,
    PreparedRequest, ProviderAdapter, ProviderKind, ProviderProfile,
};
pub use store::{
    verify_initialized_schema, StudioStore, EVIDENCE_LIBRARY_DDL, PROPOSAL_BUDGET_DDL,
};
