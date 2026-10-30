use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

use crate::{
    Agent, AgentId, CoreError, Event, EventKind, Message, MessageId, MessageSender, OperationId,
    ProviderProfile, Result, Room, RoomId, StudioSnapshot, Task, TaskId, TaskState,
};

/// SQLite-backed studio state. Every mutation and its audit event are committed
/// atomically. The mutex also makes this store safe to share across UI threads.
pub struct StudioStore {
    connection: Mutex<Connection>,
}

impl StudioStore {
    /// Creates the parent directory if needed and opens a durable SQLite file.
    /// WAL and FULL synchronous mode protect committed events across crashes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS rooms (
                 id TEXT PRIMARY KEY NOT NULL, data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS agents (
                 id TEXT PRIMARY KEY NOT NULL, data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT PRIMARY KEY NOT NULL, room_id TEXT NOT NULL,
                 data TEXT NOT NULL,
                 FOREIGN KEY(room_id) REFERENCES rooms(id)
             );
             CREATE TABLE IF NOT EXISTS tasks (
                 id TEXT PRIMARY KEY NOT NULL,
                 room_id TEXT NOT NULL, agent_id TEXT NOT NULL,
                 data TEXT NOT NULL,
                 FOREIGN KEY(room_id) REFERENCES rooms(id),
                 FOREIGN KEY(agent_id) REFERENCES agents(id)
             );
             CREATE TABLE IF NOT EXISTS providers (
                 id TEXT PRIMARY KEY NOT NULL, data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS events (
                 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                 operation_id TEXT UNIQUE NOT NULL,
                 fingerprint TEXT NOT NULL,
                 occurred_at_ms INTEGER NOT NULL,
                 kind TEXT NOT NULL,
                 result TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS messages_room_index ON messages(room_id);",
        )?;
        Ok(Self {
            connection: Mutex::new(conn),
        })
    }

    pub fn snapshot(&self) -> Result<StudioSnapshot> {
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let snapshot = StudioSnapshot {
            rooms: read_table(&tx, "rooms")?,
            agents: read_table(&tx, "agents")?,
            messages: read_table(&tx, "messages")?,
            tasks: read_table(&tx, "tasks")?,
            providers: read_table(&tx, "providers")?,
        };
        tx.commit()?;
        Ok(snapshot)
    }

    pub fn events(&self) -> Result<Vec<Event>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT sequence, operation_id, occurred_at_ms, kind FROM events ORDER BY sequence",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (sequence, operation_id, occurred_at_ms, kind) = row?;
            Ok(Event {
                sequence,
                operation_id: operation_id.parse().map_err(|_| {
                    CoreError::InvalidInput("invalid stored event operation ID".into())
                })?,
                occurred_at_ms,
                kind: serde_json::from_str(&kind)?,
            })
        })
        .collect()
    }

    pub fn create_room(&self, title: impl Into<String>) -> Result<Room> {
        self.create_room_with_operation(RoomId::new(), title, OperationId::new())
    }

    pub fn create_room_with_operation(
        &self,
        room_id: RoomId,
        title: impl Into<String>,
        operation_id: OperationId,
    ) -> Result<Room> {
        let title = title.into();
        validate_text(&title, "room title")?;
        let fingerprint = json!({"op": "create_room", "room_id": room_id, "title": title});
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = previous_result::<Room>(&tx, operation_id, &fingerprint)? {
            return Ok(existing);
        }
        let room = Room {
            id: room_id,
            title,
            created_at_ms: now_ms(),
        };
        if id_exists(&tx, "rooms", &room_id.to_string())? {
            return Err(CoreError::AlreadyExists("room"));
        }
        tx.execute(
            "INSERT INTO rooms (id, data) VALUES (?1, ?2)",
            params![room.id.to_string(), serde_json::to_string(&room)?],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::RoomCreated { room_id: room.id },
            &room,
        )?;
        tx.commit()?;
        Ok(room)
    }

    pub fn add_agent(&self, role: impl Into<String>, name: impl Into<String>) -> Result<Agent> {
        self.add_agent_with_operation(AgentId::new(), role, name, OperationId::new())
    }

    pub fn add_agent_with_operation(
        &self,
        agent_id: AgentId,
        role: impl Into<String>,
        name: impl Into<String>,
        operation_id: OperationId,
    ) -> Result<Agent> {
        let role = role.into();
        let name = name.into();
        validate_text(&role, "agent role")?;
        validate_text(&name, "agent name")?;
        let fingerprint = json!({
            "op": "add_agent",
            "agent_id": agent_id,
            "role": role,
            "name": name,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = previous_result::<Agent>(&tx, operation_id, &fingerprint)? {
            return Ok(existing);
        }
        let agent = Agent {
            id: agent_id,
            role,
            name,
            created_at_ms: now_ms(),
        };
        if id_exists(&tx, "agents", &agent_id.to_string())? {
            return Err(CoreError::AlreadyExists("agent"));
        }
        tx.execute(
            "INSERT INTO agents (id, data) VALUES (?1, ?2)",
            params![agent.id.to_string(), serde_json::to_string(&agent)?],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::AgentAdded { agent_id: agent.id },
            &agent,
        )?;
        tx.commit()?;
        Ok(agent)
    }

    pub fn append_message(
        &self,
        room_id: RoomId,
        sender: MessageSender,
        content: impl Into<String>,
    ) -> Result<Message> {
        self.append_message_with_operation(
            MessageId::new(),
            room_id,
            sender,
            content,
            OperationId::new(),
        )
    }

    pub fn append_message_with_operation(
        &self,
        message_id: MessageId,
        room_id: RoomId,
        sender: MessageSender,
        content: impl Into<String>,
        operation_id: OperationId,
    ) -> Result<Message> {
        let content = content.into();
        validate_text(&content, "message content")?;
        let fingerprint = json!({
            "op": "append_message",
            "message_id": message_id,
            "room_id": room_id,
            "sender": sender,
            "content": content,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = previous_result::<Message>(&tx, operation_id, &fingerprint)? {
            return Ok(existing);
        }
        ensure_id(&tx, "rooms", &room_id.to_string(), "room")?;
        if let MessageSender::Agent(id) = sender {
            ensure_id(&tx, "agents", &id.to_string(), "agent")?;
        }
        let message = Message {
            id: message_id,
            room_id,
            sender,
            content,
            created_at_ms: now_ms(),
        };
        if id_exists(&tx, "messages", &message_id.to_string())? {
            return Err(CoreError::AlreadyExists("message"));
        }
        tx.execute(
            "INSERT INTO messages (id, room_id, data) VALUES (?1, ?2, ?3)",
            params![
                message.id.to_string(),
                room_id.to_string(),
                serde_json::to_string(&message)?
            ],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::MessageAppended {
                message_id: message.id,
                room_id,
            },
            &message,
        )?;
        tx.commit()?;
        Ok(message)
    }

    pub fn save_provider_profile(&self, profile: ProviderProfile) -> Result<()> {
        self.save_provider_profile_with_operation(profile, OperationId::new())
    }

    pub fn save_provider_profile_with_operation(
        &self,
        profile: ProviderProfile,
        operation_id: OperationId,
    ) -> Result<()> {
        profile.validate()?;
        let fingerprint = json!({"op": "save_provider_profile", "profile": profile});
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if previous_result::<ProviderProfile>(&tx, operation_id, &fingerprint)?.is_some() {
            return Ok(());
        }
        tx.execute(
            "INSERT INTO providers (id, data) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![profile.id.to_string(), serde_json::to_string(&profile)?],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::ProviderSaved {
                provider_id: profile.id,
            },
            &profile,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn create_task(
        &self,
        room_id: RoomId,
        agent_id: AgentId,
        instructions: impl Into<String>,
    ) -> Result<Task> {
        self.create_task_with_operation(
            TaskId::new(),
            room_id,
            agent_id,
            instructions,
            OperationId::new(),
        )
    }

    /// Caller-supplied IDs allow a retried external command to be idempotent.
    /// Reusing an operation with different input returns OperationConflict.
    pub fn create_task_with_operation(
        &self,
        task_id: TaskId,
        room_id: RoomId,
        agent_id: AgentId,
        instructions: impl Into<String>,
        operation_id: OperationId,
    ) -> Result<Task> {
        let instructions = instructions.into();
        validate_text(&instructions, "task instructions")?;
        let fingerprint = json!({
            "op": "create_task",
            "task_id": task_id,
            "room_id": room_id,
            "agent_id": agent_id,
            "instructions": instructions,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = previous_result::<Task>(&tx, operation_id, &fingerprint)? {
            return Ok(existing);
        }
        ensure_id(&tx, "rooms", &room_id.to_string(), "room")?;
        ensure_id(&tx, "agents", &agent_id.to_string(), "agent")?;
        if id_exists(&tx, "tasks", &task_id.to_string())? {
            return Err(CoreError::AlreadyExists("task"));
        }
        let timestamp = now_ms();
        let task = Task {
            id: task_id,
            room_id,
            agent_id,
            instructions,
            state: TaskState::Pending,
            failure_reason: None,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
        };
        tx.execute(
            "INSERT INTO tasks (id, room_id, agent_id, data) VALUES (?1, ?2, ?3, ?4)",
            params![
                task_id.to_string(),
                room_id.to_string(),
                agent_id.to_string(),
                serde_json::to_string(&task)?
            ],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::TaskCreated { task_id },
            &task,
        )?;
        tx.commit()?;
        Ok(task)
    }

    /// Compare-and-set transition, with exactly-once event semantics for retries.
    /// A cancellation that wins the transaction prevents later completion.
    pub fn transition_task(
        &self,
        task_id: TaskId,
        expected: TaskState,
        next: TaskState,
        operation_id: OperationId,
        failure_reason: Option<String>,
    ) -> Result<Task> {
        if next != TaskState::Failed && failure_reason.is_some() {
            return Err(CoreError::InvalidInput(
                "failure reason is only valid for failed tasks".into(),
            ));
        }
        if let Some(reason) = failure_reason.as_ref() {
            validate_text(reason, "failure reason")?;
        }
        let fingerprint = json!({
            "op": "transition_task",
            "task_id": task_id,
            "expected": expected,
            "next": next,
            "failure_reason": failure_reason,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = previous_result::<Task>(&tx, operation_id, &fingerprint)? {
            return Ok(existing);
        }
        let mut task: Task =
            read_one(&tx, "tasks", &task_id.to_string())?.ok_or(CoreError::NotFound("task"))?;
        if task.state != expected {
            return Err(CoreError::StateConflict {
                expected,
                actual: task.state,
            });
        }
        if !expected.permits(next) {
            return Err(CoreError::InvalidTransition {
                from: expected,
                to: next,
            });
        }
        task.state = next;
        task.failure_reason = failure_reason;
        task.updated_at_ms = now_ms();
        tx.execute(
            "UPDATE tasks SET data = ?2 WHERE id = ?1",
            params![task.id.to_string(), serde_json::to_string(&task)?],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::TaskTransitioned {
                task_id,
                from: expected,
                to: next,
            },
            &task,
        )?;
        tx.commit()?;
        Ok(task)
    }

    /// Commits a verified model result and a task's successful terminal state
    /// in ONE SQLite transaction. A concurrent cancellation wins or loses
    /// atomically; it cannot leave a fabricated success or orphaned reply.
    /// One event represents the coupled state/message mutation.
    pub fn complete_task_with_message(
        &self,
        task_id: TaskId,
        message_id: MessageId,
        operation_id: OperationId,
        content: impl Into<String>,
    ) -> Result<(Task, Message)> {
        let content = content.into();
        validate_text(&content, "task result")?;
        let fingerprint = json!({
            "op": "complete_task_with_message",
            "task_id": task_id,
            "message_id": message_id,
            "content": content,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(previous) = previous_result::<(Task, Message)>(&tx, operation_id, &fingerprint)?
        {
            return Ok(previous);
        }
        let mut task: Task =
            read_one(&tx, "tasks", &task_id.to_string())?.ok_or(CoreError::NotFound("task"))?;
        if task.state != TaskState::Running {
            return Err(CoreError::StateConflict {
                expected: TaskState::Running,
                actual: task.state,
            });
        }
        if id_exists(&tx, "messages", &message_id.to_string())? {
            return Err(CoreError::AlreadyExists("message"));
        }
        let message = Message {
            id: message_id,
            room_id: task.room_id,
            sender: MessageSender::Agent(task.agent_id),
            content,
            created_at_ms: now_ms(),
        };
        tx.execute(
            "INSERT INTO messages (id, room_id, data) VALUES (?1, ?2, ?3)",
            params![
                message.id.to_string(),
                message.room_id.to_string(),
                serde_json::to_string(&message)?,
            ],
        )?;
        task.state = TaskState::Succeeded;
        task.failure_reason = None;
        task.updated_at_ms = now_ms();
        tx.execute(
            "UPDATE tasks SET data = ?2 WHERE id = ?1",
            params![task.id.to_string(), serde_json::to_string(&task)?],
        )?;
        let result = (task, message);
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::TaskCompleted {
                task_id,
                message_id,
                room_id: result.0.room_id,
            },
            &result,
        )?;
        tx.commit()?;
        Ok(result)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| CoreError::Poisoned)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn validate_text(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 1_000_000 {
        return Err(CoreError::InvalidInput(format!("invalid {field}")));
    }
    Ok(())
}

fn id_exists(tx: &Transaction<'_>, table: &'static str, id: &str) -> Result<bool> {
    let sql = match table {
        "rooms" => "SELECT 1 FROM rooms WHERE id = ?1",
        "agents" => "SELECT 1 FROM agents WHERE id = ?1",
        "tasks" => "SELECT 1 FROM tasks WHERE id = ?1",
        "messages" => "SELECT 1 FROM messages WHERE id = ?1",
        _ => return Err(CoreError::InvalidInput("unknown table".into())),
    };
    Ok(tx
        .query_row(sql, [id], |row| row.get::<_, i32>(0))
        .optional()?
        .is_some())
}

fn ensure_id(
    tx: &Transaction<'_>,
    table: &'static str,
    id: &str,
    label: &'static str,
) -> Result<()> {
    if id_exists(tx, table, id)? {
        Ok(())
    } else {
        Err(CoreError::NotFound(label))
    }
}

fn read_one<T: DeserializeOwned>(
    tx: &Transaction<'_>,
    table: &'static str,
    id: &str,
) -> Result<Option<T>> {
    let sql = match table {
        "tasks" => "SELECT data FROM tasks WHERE id = ?1",
        _ => return Err(CoreError::InvalidInput("unknown table".into())),
    };
    let data: Option<String> = tx.query_row(sql, [id], |row| row.get(0)).optional()?;
    data.map(|data| serde_json::from_str(&data).map_err(Into::into))
        .transpose()
}

fn read_table<T: DeserializeOwned>(tx: &Transaction<'_>, table: &'static str) -> Result<Vec<T>> {
    let sql = match table {
        "rooms" => "SELECT data FROM rooms ORDER BY rowid",
        "agents" => "SELECT data FROM agents ORDER BY rowid",
        "messages" => "SELECT data FROM messages ORDER BY rowid",
        "tasks" => "SELECT data FROM tasks ORDER BY rowid",
        "providers" => "SELECT data FROM providers ORDER BY rowid",
        _ => return Err(CoreError::InvalidInput("unknown table".into())),
    };
    let mut stmt = tx.prepare(sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}

fn previous_result<T: DeserializeOwned>(
    tx: &Transaction<'_>,
    operation_id: OperationId,
    fingerprint: &Value,
) -> Result<Option<T>> {
    let found: Option<(String, String)> = tx
        .query_row(
            "SELECT fingerprint, result FROM events WHERE operation_id = ?1",
            [operation_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match found {
        Some((stored_fingerprint, result)) => {
            if stored_fingerprint != serde_json::to_string(fingerprint)? {
                return Err(CoreError::OperationConflict);
            }
            Ok(Some(serde_json::from_str(&result)?))
        }
        None => Ok(None),
    }
}

fn record_event<T: Serialize>(
    tx: &Transaction<'_>,
    operation_id: OperationId,
    fingerprint: &Value,
    kind: EventKind,
    result: &T,
) -> Result<()> {
    tx.execute(
        "INSERT INTO events (operation_id, fingerprint, occurred_at_ms, kind, result)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            operation_id.to_string(),
            serde_json::to_string(fingerprint)?,
            now_ms(),
            serde_json::to_string(&kind)?,
            serde_json::to_string(result)?
        ],
    )?;
    Ok(())
}
