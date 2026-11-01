use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }
    };
}

typed_id!(AgentId);
typed_id!(RoomId);
typed_id!(MessageId);
typed_id!(TaskId);
typed_id!(OperationId);
typed_id!(ProviderId);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: AgentId,
    pub role: String,
    pub name: String,
    /// Optional user-selected provider affinity. Legacy agent rows deserialize
    /// without this key; model routing still requires an explicit UI action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<ProviderId>,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub id: RoomId,
    pub title: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum MessageSender {
    User,
    Agent(AgentId),
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: MessageId,
    pub room_id: RoomId,
    pub sender: MessageSender,
    pub content: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn permits(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Queued | Self::Cancelled)
                | (Self::Queued, Self::Running | Self::Failed | Self::Cancelled)
                | (
                    Self::Running,
                    Self::Succeeded | Self::Failed | Self::Cancelled
                )
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: TaskId,
    pub room_id: RoomId,
    pub agent_id: AgentId,
    pub instructions: String,
    pub state: TaskState,
    pub failure_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventKind {
    RoomCreated {
        room_id: RoomId,
    },
    AgentAdded {
        agent_id: AgentId,
    },
    AgentProviderChanged {
        agent_id: AgentId,
        provider_id: Option<ProviderId>,
    },
    MessageAppended {
        message_id: MessageId,
        room_id: RoomId,
    },
    ProviderSaved {
        provider_id: ProviderId,
    },
    TaskCreated {
        task_id: TaskId,
    },
    TaskTransitioned {
        task_id: TaskId,
        from: TaskState,
        to: TaskState,
    },
    TaskCompleted {
        task_id: TaskId,
        message_id: MessageId,
        room_id: RoomId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub sequence: i64,
    pub operation_id: OperationId,
    pub occurred_at_ms: i64,
    pub kind: EventKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioSnapshot {
    pub rooms: Vec<Room>,
    pub agents: Vec<Agent>,
    pub messages: Vec<Message>,
    pub tasks: Vec<Task>,
    pub providers: Vec<crate::ProviderProfile>,
}
