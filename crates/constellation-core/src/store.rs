use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    Agent, AgentId, CoreError, CredentialSource, Event, EventKind, Message, MessageId,
    MessageSender, OperationId, ProviderId, ProviderProfile, Result, Room, RoomId, StudioSnapshot,
    Task, TaskId, TaskState,
};

/// SQLite-backed studio state. Every mutation and its audit event are committed
/// atomically. The mutex also makes this store safe to share across UI threads.
pub struct StudioStore {
    connection: Mutex<Connection>,
}

/// Unsent text is local project data. Revisions prevent a stale editor/window
/// from overwriting a more recent draft without explicit user intervention.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomDraft {
    pub room_id: RoomId,
    pub content: String,
    pub revision: i64,
}

/// The first explicitly versioned layout is byte-for-byte compatible with
/// the formerly shipped, unversioned six-table schema. Never infer a foreign
/// database's identity just from coincidentally named tables.
const STORE_SCHEMA_VERSION: i64 = 1;
const STORE_SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS rooms (
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
         CREATE INDEX IF NOT EXISTS messages_room_index ON messages(room_id);";

/// Exact six-table DDL shipped by the original unversioned desktop build
/// (b229c5f). SQLite retains indentation in sqlite_master.sql, so comparing
/// only the newer DDL falsely rejects an otherwise valid existing profile.
/// Do not replace this explicit allowlist with permissive SQL normalization:
/// unknown tables, changed constraints and triggers must still fail closed.
const STORE_LEGACY_SCHEMA_DDL: &str = "PRAGMA foreign_keys = ON;
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
             CREATE INDEX IF NOT EXISTS messages_room_index ON messages(room_id);";

/// The native host has historically reserved billable model-proposal attempts
/// in the *same* StudioStore SQLite file. Those two tables are a known,
/// optional extension, not a foreign database. Share the exact DDL with the
/// host so a restart after a successful reservation cannot brick the store.
/// Both tables must be present with exactly this layout, or neither may be.
pub const PROPOSAL_BUDGET_DDL: &str = "CREATE TABLE IF NOT EXISTS file_proposal_task_budget (
           task_id TEXT PRIMARY KEY NOT NULL,
           attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 16)
         );
         CREATE TABLE IF NOT EXISTS file_proposal_provider_budget (
           provider_id TEXT PRIMARY KEY NOT NULL,
           attempts INTEGER NOT NULL CHECK (attempts >= 1 AND attempts <= 128)
         );";

/// Saved evidence shares the project SQLite file and its existing encrypted
/// archive boundary. Pasted-only searches never create this optional table.
pub const EVIDENCE_LIBRARY_DDL: &str = "CREATE TABLE IF NOT EXISTS evidence_sources (
           source_id TEXT PRIMARY KEY NOT NULL,
           title TEXT NOT NULL,
           source_format TEXT NOT NULL CHECK (source_format IN ('markdown','plain_text')),
           source_text TEXT NOT NULL,
           source_sha256 TEXT NOT NULL,
           source_bytes INTEGER NOT NULL CHECK (source_bytes > 0 AND source_bytes <= 131072)
         );";

/// Optional, strictly recognized extension to the project database. Keeping
/// this in the existing file means encrypted project archives include drafts.
pub const ROOM_DRAFT_DDL: &str = "CREATE TABLE IF NOT EXISTS room_drafts (
           room_id TEXT PRIMARY KEY NOT NULL REFERENCES rooms(id),
           content TEXT NOT NULL CHECK (length(CAST(content AS BLOB)) <= 131072),
           revision INTEGER NOT NULL CHECK (revision >= 1)
         );";

type SchemaObjects = Vec<(String, String, String)>;

fn schema_objects(conn: &Connection) -> Result<SchemaObjects> {
    let mut stmt = conn.prepare(
        "SELECT type, name, sql FROM sqlite_master
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// SQLite stores the original CREATE SQL in sqlite_master. Compare the entire
/// known schema (tables, indexes, triggers and views), not only table names.
/// This intentionally rejects unrecognized "compatible-looking" layouts
/// rather than guessing a migration for a foreign or previously modified DB.
fn known_schema() -> Result<SchemaObjects> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(STORE_SCHEMA_DDL)?;
    schema_objects(&reference)
}

fn known_schema_with_extensions(
    ddl: &str,
    budgets: bool,
    evidence: bool,
    drafts: bool,
) -> Result<SchemaObjects> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(ddl)?;
    if budgets {
        reference.execute_batch(PROPOSAL_BUDGET_DDL)?;
    }
    if evidence {
        reference.execute_batch(EVIDENCE_LIBRARY_DDL)?;
    }
    if drafts {
        reference.execute_batch(ROOM_DRAFT_DDL)?;
    }
    schema_objects(&reference)
}

fn schema_state(conn: &Connection) -> Result<bool> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 0 && version != STORE_SCHEMA_VERSION {
        return Err(CoreError::UnsupportedSchemaVersion(version));
    }
    let present = schema_objects(conn)?;
    if present.is_empty() && version == 0 {
        // A truly new SQLite database has no sqlite_master entries. An
        // unrelated AUTOINCREMENT table can leave sqlite_sequence behind
        // after DROP TABLE: treating that as fresh would adopt a foreign DB.
        let internal_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))?;
        if internal_count != 0 {
            return Err(CoreError::UnrecognizedSchema);
        }
        return Ok(true); // Fresh database; create known schema atomically.
    }
    if present == known_schema()? {
        return Ok(version == 0); // Exact unversioned layout: version-only migration.
    }
    for ddl in [STORE_SCHEMA_DDL, STORE_LEGACY_SCHEMA_DDL] {
        for budgets in [false, true] {
            for evidence in [false, true] {
                for drafts in [false, true] {
                    if present == known_schema_with_extensions(ddl, budgets, evidence, drafts)? {
                        return Ok(version == 0);
                    }
                }
            }
        }
    }
    Err(CoreError::UnrecognizedSchema)
}

/// Validate the exact committed Constellation layout on a caller-owned SQLite
/// connection or transaction, without migrating or writing anything. The
/// native host must use this *inside* its budget transaction so a replaced
/// table, weakened CHECK constraint, or foreign DB cannot authorize a model
/// request between desktop startup and the subsequent billable dispatch.
/// A version-zero database needs migration through `StudioStore::open` first.
pub fn verify_initialized_schema(conn: &Connection) -> Result<()> {
    if schema_state(conn)? {
        return Err(CoreError::UnrecognizedSchema);
    }
    Ok(())
}

fn initialize_schema(conn: &mut Connection) -> Result<()> {
    // Keep *all* preflight reads in one SQLite snapshot. Two independent
    // first-openers may race: without a read transaction, user_version and
    // the user tables could be observed before another writer commits while
    // the subsequent sqlite_master internal-count query observes its newly
    // committed sqlite_sequence. That inconsistent cross-statement view
    // falsely classifies a genuine Constellation database as foreign.
    // A deferred read transaction must end BEFORE activating WAL; it never
    // migrates or changes the journal mode of an unrecognized database.
    {
        let preflight = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        schema_state(&preflight)?;
        preflight.commit()?;
    }
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    // Foreign keys must be enabled before BEGIN; SQLite ignores changes
    // to that pragma inside a transaction. A failed schema upgrade must
    // roll back *all* DDL, including tables created before a later error.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let schema = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Re-check after acquiring the writer lock: another process might have
    // initialized or upgraded the database after our read-only preflight.
    let needs_migration = schema_state(&schema)?;
    if needs_migration {
        if schema_objects(&schema)?.is_empty() {
            schema.execute_batch(STORE_SCHEMA_DDL)?;
        }
        // DDL and the version update commit together. Errors (disk full,
        // injected SQL failure or process interruption) leave user_version=0.
        schema.pragma_update(None, "user_version", STORE_SCHEMA_VERSION)?;
    }
    schema.commit()?;
    Ok(())
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
        let mut conn = Connection::open(path)?;
        // A single SQLite busy wait must not consume the entire retry
        // budget. Use a short busy timeout during schema admission only,
        // then restore a generous timeout for ordinary task/event writes.
        conn.busy_timeout(Duration::from_millis(25))?;
        // Initial WAL activation can return SQLITE_BUSY immediately while
        // another process is opening the same brand-new database. Bounded
        // retries are safe because schema DDL is atomic below.
        for attempt in 0..=40 {
            match initialize_schema(&mut conn) {
                Ok(()) => {
                    conn.busy_timeout(Duration::from_secs(5))?;
                    return Ok(Self {
                        connection: Mutex::new(conn),
                    });
                }
                Err(CoreError::Database(rusqlite::Error::SqliteFailure(err, _)))
                    if matches!(
                        err.code,
                        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
                    ) && attempt < 40 =>
                {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("initialization either succeeds or returns the final error")
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

    /// Read persisted room drafts, including empty tombstones that retain CAS
    /// revisions. Never infer a schema extension from table presence alone.
    pub fn room_drafts(&self) -> Result<Vec<RoomDraft>> {
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        verify_initialized_schema(&tx)?;
        let has_table: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='room_drafts')",
            [],
            |row| row.get(0),
        )?;
        if !has_table {
            return Ok(Vec::new());
        }
        let drafts = {
            let mut stmt =
                tx.prepare("SELECT room_id, content, revision FROM room_drafts ORDER BY room_id")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?;
            rows.map(|row| {
                let (room_id, content, revision) = row?;
                Ok(RoomDraft {
                    room_id: room_id.parse().map_err(|_| {
                        CoreError::InvalidInput("invalid saved draft room ID".into())
                    })?,
                    content,
                    revision,
                })
            })
            .collect::<Result<Vec<_>>>()?
        };
        tx.commit()?;
        Ok(drafts)
    }

    /// Save a draft using optimistic concurrency. The initial expected revision
    /// is zero. A blank draft is persisted as a tombstone to avoid ABA races.
    pub fn save_room_draft(
        &self,
        room_id: RoomId,
        content: String,
        expected_revision: i64,
    ) -> Result<RoomDraft> {
        if content.len() > 131_072 || !(0..i64::MAX).contains(&expected_revision) {
            return Err(CoreError::InvalidInput(
                "draft size or revision is invalid".into(),
            ));
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_initialized_schema(&tx)?;
        ensure_id(&tx, "rooms", &room_id.to_string(), "room")?;
        let has_table: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='room_drafts')",
            [],
            |row| row.get(0),
        )?;
        let current: Option<i64> = if has_table {
            tx.query_row(
                "SELECT revision FROM room_drafts WHERE room_id=?1",
                [room_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
        } else {
            None
        };
        if current.unwrap_or(0) != expected_revision {
            return Err(CoreError::InvalidInput(
                "draft changed in another window; review before overwriting".into(),
            ));
        }
        if !has_table {
            tx.execute_batch(ROOM_DRAFT_DDL)?;
        }
        let revision = expected_revision + 1;
        tx.execute(
            "INSERT INTO room_drafts (room_id, content, revision) VALUES (?1, ?2, ?3)
             ON CONFLICT(room_id) DO UPDATE SET content=excluded.content, revision=excluded.revision",
            params![room_id.to_string(), content, revision],
        )?;
        tx.commit()?;
        Ok(RoomDraft {
            room_id,
            content,
            revision,
        })
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
            member_ids: None,
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

    /// Persist room-level agent routing with optimistic stale-write detection.
    /// The selection does not grant model, file or permission capabilities.
    /// A legacy room has None; an explicitly empty selection is Some([]).
    pub fn set_room_members(
        &self,
        room_id: RoomId,
        member_ids: Vec<AgentId>,
        expected_member_ids: Option<Vec<AgentId>>,
        operation_id: OperationId,
    ) -> Result<Room> {
        if member_ids.len() > 8 {
            return Err(CoreError::InvalidInput(
                "room team cannot exceed eight agents".into(),
            ));
        }
        let mut unique = std::collections::HashSet::new();
        if !member_ids.iter().all(|id| unique.insert(*id)) {
            return Err(CoreError::InvalidInput("duplicate room team agent".into()));
        }
        let fingerprint = json!({
            "op":"set_room_members", "room_id":room_id,
            "member_ids":member_ids, "expected_member_ids":expected_member_ids,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(result) = previous_result::<Room>(&tx, operation_id, &fingerprint)? {
            return Ok(result);
        }
        let mut room: Room =
            read_one(&tx, "rooms", &room_id.to_string())?.ok_or(CoreError::NotFound("room"))?;
        if room.member_ids != expected_member_ids {
            return Err(CoreError::OperationConflict);
        }
        for agent_id in &member_ids {
            ensure_id(&tx, "agents", &agent_id.to_string(), "agent")?;
        }
        room.member_ids = Some(member_ids.clone());
        tx.execute(
            "UPDATE rooms SET data=?2 WHERE id=?1",
            params![room_id.to_string(), serde_json::to_string(&room)?],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::RoomMembersChanged {
                room_id,
                member_ids,
            },
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
            provider_id: None,
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

    /// Remove a never-used worker from the local roster without destroying
    /// historical attribution. A saved room team, message or task referencing
    /// the worker prevents removal. The native caller must independently
    /// exclude saved workflows and active operations before entering here.
    ///
    /// The full expected agent is a compare-and-swap token: another window's
    /// provider assignment or roster update cannot silently be discarded.
    pub fn remove_unused_agent(&self, expected: Agent, operation_id: OperationId) -> Result<()> {
        let fingerprint = json!({"op":"remove_unused_agent","expected":expected});
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if previous_result::<Agent>(&tx, operation_id, &fingerprint)?.is_some() {
            return Ok(());
        }
        let current: Agent = read_one(&tx, "agents", &expected.id.to_string())?
            .ok_or(CoreError::NotFound("agent"))?;
        if current != expected {
            return Err(CoreError::OperationConflict);
        }
        if read_table::<Room>(&tx, "rooms")?.iter().any(|room| {
            room.member_ids
                .as_ref()
                .is_some_and(|ids| ids.contains(&expected.id))
        }) {
            return Err(CoreError::InvalidInput(
                "remove this worker from all saved room teams first".into(),
            ));
        }
        if read_table::<Message>(&tx, "messages")?
            .iter()
            .any(|message| message.sender == MessageSender::Agent(expected.id))
        {
            return Err(CoreError::InvalidInput(
                "this worker authored messages; retain its identity for provenance".into(),
            ));
        }
        if read_table::<Task>(&tx, "tasks")?
            .iter()
            .any(|task| task.agent_id == expected.id)
        {
            return Err(CoreError::InvalidInput(
                "this worker owns saved tasks; retain its identity for provenance".into(),
            ));
        }
        tx.execute("DELETE FROM agents WHERE id=?1", [expected.id.to_string()])?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::AgentRemoved {
                agent_id: expected.id,
            },
            &expected,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Changes the preferred provider for one agent. There is no implicit
    /// fallback to a different provider, and preference alone never dispatches
    /// a network request. The change is atomically audited in the event log.
    pub fn set_agent_provider(
        &self,
        agent_id: AgentId,
        provider_id: Option<ProviderId>,
        operation_id: OperationId,
    ) -> Result<Agent> {
        let fingerprint = json!({
            "op": "set_agent_provider",
            "agent_id": agent_id,
            "provider_id": provider_id,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(previous) = previous_result::<Agent>(&tx, operation_id, &fingerprint)? {
            return Ok(previous);
        }
        let mut agent: Agent =
            read_one(&tx, "agents", &agent_id.to_string())?.ok_or(CoreError::NotFound("agent"))?;
        if let Some(id) = provider_id {
            let present: Option<String> = tx
                .query_row(
                    "SELECT id FROM providers WHERE id = ?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            if present.is_none() {
                return Err(CoreError::NotFound("provider"));
            }
        }
        agent.provider_id = provider_id;
        tx.execute(
            "UPDATE agents SET data = ?2 WHERE id = ?1",
            params![agent_id.to_string(), serde_json::to_string(&agent)?],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::AgentProviderChanged {
                agent_id,
                provider_id,
            },
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

    /// Save a human message and clear only its exact, revision-bound unsent
    /// draft in ONE SQLite commit. Prevents the restart window where an
    /// already-saved message reappears in the composer and can be resent.
    /// The original operation ID makes uncertain IPC retries idempotent.
    pub fn append_user_message_and_clear_draft(
        &self,
        message_id: MessageId,
        room_id: RoomId,
        content: String,
        expected_draft_revision: i64,
        operation_id: OperationId,
    ) -> Result<Message> {
        validate_text(&content, "message content")?;
        if !(1..i64::MAX).contains(&expected_draft_revision) {
            return Err(CoreError::InvalidInput(
                "invalid saved draft revision".into(),
            ));
        }
        let fingerprint = json!({
            "op": "append_user_message_and_clear_draft",
            "message_id": message_id,
            "room_id": room_id,
            "content": content,
            "expected_draft_revision": expected_draft_revision,
        });
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        verify_initialized_schema(&tx)?;
        if let Some(previous) = previous_result::<Message>(&tx, operation_id, &fingerprint)? {
            return Ok(previous);
        }
        ensure_id(&tx, "rooms", &room_id.to_string(), "room")?;
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT content, revision FROM room_drafts WHERE room_id=?1",
                [room_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((draft, revision))
                if revision == expected_draft_revision && draft.trim() == content => {}
            _ => return Err(CoreError::OperationConflict),
        }
        if id_exists(&tx, "messages", &message_id.to_string())? {
            return Err(CoreError::AlreadyExists("message"));
        }
        let message = Message {
            id: message_id,
            room_id,
            sender: MessageSender::User,
            content,
            created_at_ms: now_ms(),
        };
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
        tx.execute(
            "UPDATE room_drafts SET content='', revision=?1 WHERE room_id=?2",
            params![expected_draft_revision + 1, room_id.to_string()],
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

    /// Remove only a credential-free, completely unused local provider. The
    /// expected public profile is checked inside the same write transaction;
    /// no saved agent, message or task may be orphaned. Other application
    /// stores (such as workflows) must be checked by the native caller.
    pub fn remove_unused_provider(
        &self,
        expected: ProviderProfile,
        operation_id: OperationId,
    ) -> Result<()> {
        expected.validate()?;
        if expected.credentials != CredentialSource::None {
            return Err(CoreError::InvalidInput(
                "only profiles without stored credentials may be removed".into(),
            ));
        }
        let fingerprint = json!({"op":"remove_unused_provider", "expected":expected});
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if previous_result::<ProviderProfile>(&tx, operation_id, &fingerprint)?.is_some() {
            return Ok(());
        }
        let current: ProviderProfile = read_one(&tx, "providers", &expected.id.to_string())?
            .ok_or(CoreError::NotFound("provider"))?;
        if current != expected {
            return Err(CoreError::OperationConflict);
        }
        let agents: Vec<Agent> = read_table(&tx, "agents")?;
        if agents
            .iter()
            .any(|agent| agent.provider_id == Some(expected.id))
        {
            return Err(CoreError::InvalidInput(
                "an agent is still assigned to this provider".into(),
            ));
        }
        // A conservative retention boundary: a provider could have been used
        // as an explicit task/message route even without a current agent
        // preference. Require a workspace with no task/message history.
        for table in ["tasks", "messages"] {
            if !read_table::<serde_json::Value>(&tx, table)?.is_empty() {
                return Err(CoreError::InvalidInput(
                    "the workspace contains task or message history; retain this provider for provenance".into(),
                ));
            }
        }
        tx.execute(
            "DELETE FROM providers WHERE id=?1",
            [expected.id.to_string()],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::ProviderRemoved {
                provider_id: expected.id,
            },
            &expected,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Retire only the credential reference, not the provider's historical
    /// identity. The native host separately deletes the OS vault entry first.
    /// CAS prevents a stale settings window from silently modifying a revised
    /// provider, and the existing immutable event ledger records the change.
    pub fn forget_remembered_provider_credential(
        &self,
        expected: ProviderProfile,
        operation_id: OperationId,
    ) -> Result<()> {
        expected.validate()?;
        if !matches!(expected.credentials, CredentialSource::OsKeyring { .. }) {
            return Err(CoreError::InvalidInput(
                "the expected provider must have an OS-stored credential".into(),
            ));
        }
        let mut retired = expected.clone();
        retired.credentials = CredentialSource::None;
        let fingerprint = json!({"op":"forget_remembered_provider_credential","expected":expected});
        let mut conn = self.lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if previous_result::<ProviderProfile>(&tx, operation_id, &fingerprint)?.is_some() {
            return Ok(());
        }
        let current: ProviderProfile = read_one(&tx, "providers", &expected.id.to_string())?
            .ok_or(CoreError::NotFound("provider"))?;
        if current != expected {
            return Err(CoreError::OperationConflict);
        }
        tx.execute(
            "UPDATE providers SET data=?1 WHERE id=?2",
            params![serde_json::to_string(&retired)?, retired.id.to_string()],
        )?;
        record_event(
            &tx,
            operation_id,
            &fingerprint,
            EventKind::ProviderSaved {
                provider_id: retired.id,
            },
            &retired,
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
        "rooms" => "SELECT data FROM rooms WHERE id = ?1",
        "providers" => "SELECT data FROM providers WHERE id = ?1",
        "tasks" => "SELECT data FROM tasks WHERE id = ?1",
        "agents" => "SELECT data FROM agents WHERE id = ?1",
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

#[cfg(test)]
mod historical_schema_startup_tests {
    use super::*;

    #[test]
    fn worker_removal_is_confirmed_idempotent_and_preserves_immutable_audit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workers.sqlite3");
        let store = StudioStore::open(&path).unwrap();
        let agent = store.add_agent("Researcher", "Unused worker").unwrap();
        let operation = OperationId::new();
        let mut stale = agent.clone();
        stale.name = "wrong name".into();
        assert!(matches!(
            store.remove_unused_agent(stale, OperationId::new()),
            Err(CoreError::OperationConflict)
        ));
        assert_eq!(store.snapshot().unwrap().agents.len(), 1);
        store.remove_unused_agent(agent.clone(), operation).unwrap();
        store.remove_unused_agent(agent.clone(), operation).unwrap();
        assert!(store.snapshot().unwrap().agents.is_empty());
        assert!(matches!(
            store.remove_unused_agent(agent.clone(), OperationId::new()),
            Err(CoreError::NotFound("agent"))
        ));
        drop(store);
        let reopened = StudioStore::open(&path).unwrap();
        assert!(reopened.snapshot().unwrap().agents.is_empty());
        let events = reopened.events().unwrap();
        assert!(events.iter().any(|event| matches!(
            event.kind, EventKind::AgentRemoved { agent_id } if agent_id == agent.id
        )));
    }

    #[test]
    fn worker_removal_rejects_saved_membership_history_and_task_owner() {
        // Each restriction is checked in its own profile. A failed deletion
        // must leave the agent and associated evidence unchanged.
        for case in ["room", "message", "task"] {
            let dir = tempfile::tempdir().unwrap();
            let store = StudioStore::open(dir.path().join("worker-safety.sqlite3")).unwrap();
            let room = store.create_room("Research workspace").unwrap();
            let agent = store.add_agent("Reviewer", "Worker with history").unwrap();
            match case {
                "room" => {
                    store
                        .set_room_members(room.id, vec![agent.id], None, OperationId::new())
                        .unwrap();
                }
                "message" => {
                    store
                        .append_message(room.id, MessageSender::Agent(agent.id), "Saved response")
                        .unwrap();
                }
                "task" => {
                    store.create_task(room.id, agent.id, "Saved task").unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    store.remove_unused_agent(agent.clone(), OperationId::new()),
                    Err(CoreError::InvalidInput(_))
                ),
                "unexpected removal for {case}"
            );
            assert_eq!(store.snapshot().unwrap().agents, vec![agent.clone()]);
            // Membership is a routing preference, so removing someone from
            // the room safely permits roster deletion if no other history
            // refers to that worker.
            if case == "room" {
                store
                    .set_room_members(room.id, vec![], Some(vec![agent.id]), OperationId::new())
                    .unwrap();
                store
                    .remove_unused_agent(agent, OperationId::new())
                    .unwrap();
                assert!(store.snapshot().unwrap().agents.is_empty());
            }
        }
    }

    #[test]
    fn shipped_unversioned_profile_migrates_without_rewriting_schema_or_data() {
        for budget in [false, true] {
            for evidence in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("constellation.sqlite3");
                let original = Connection::open(&path).unwrap();
                original.execute_batch(STORE_LEGACY_SCHEMA_DDL).unwrap();
                if budget {
                    original.execute_batch(PROPOSAL_BUDGET_DDL).unwrap();
                }
                if evidence {
                    original.execute_batch(EVIDENCE_LIBRARY_DDL).unwrap();
                }
                original
                    .execute(
                        "INSERT INTO rooms(id, data) VALUES (?1, ?2)",
                        ["existing-room", "preexisting-data-must-survive"],
                    )
                    .unwrap();
                let before = schema_objects(&original).unwrap();
                assert_eq!(
                    original
                        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                        .unwrap(),
                    0
                );
                drop(original);

                let store = StudioStore::open(&path).unwrap();
                drop(store);
                let migrated = Connection::open(&path).unwrap();
                assert_eq!(
                    migrated
                        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert_eq!(schema_objects(&migrated).unwrap(), before);
                let kept: String = migrated
                    .query_row("SELECT data FROM rooms WHERE id='existing-room'", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                assert_eq!(kept, "preexisting-data-must-survive");
                verify_initialized_schema(&migrated).unwrap();
                drop(migrated);
                // Opening the already-versioned *legacy layout* must continue
                // working on every subsequent launch, not just migration.
                StudioStore::open(&path).unwrap();
            }
        }
    }

    #[test]
    fn legacy_profile_with_changed_table_or_extra_trigger_is_still_rejected_without_writes() {
        for sql in [
            "ALTER TABLE rooms ADD COLUMN unauthorized TEXT;",
            "CREATE TRIGGER unauthorized AFTER INSERT ON rooms BEGIN SELECT 1; END;",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("constellation.sqlite3");
            let original = Connection::open(&path).unwrap();
            original.execute_batch(STORE_LEGACY_SCHEMA_DDL).unwrap();
            original.execute_batch(sql).unwrap();
            let before = schema_objects(&original).unwrap();
            drop(original);

            assert!(matches!(
                StudioStore::open(&path),
                Err(CoreError::UnrecognizedSchema)
            ));
            let rejected = Connection::open(&path).unwrap();
            assert_eq!(schema_objects(&rejected).unwrap(), before);
            assert_eq!(
                rejected
                    .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn drafts_survive_reopen_and_reject_stale_window_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.sqlite3");
        let store = StudioStore::open(&path).unwrap();
        let room = store.create_room("Draft test").unwrap();
        assert!(store.room_drafts().unwrap().is_empty());
        let first = store
            .save_room_draft(room.id, "Unsent text 🧪".into(), 0)
            .unwrap();
        assert_eq!(first.revision, 1);
        drop(store);

        let reopened = StudioStore::open(&path).unwrap();
        assert_eq!(reopened.room_drafts().unwrap(), vec![first.clone()]);
        let latest = reopened
            .save_room_draft(room.id, "Updated locally".into(), 1)
            .unwrap();
        assert_eq!(latest.revision, 2);
        assert!(matches!(
            reopened.save_room_draft(room.id, "stale window".into(), 1),
            Err(CoreError::InvalidInput(_))
        ));
        assert_eq!(reopened.room_drafts().unwrap(), vec![latest]);
        let empty = reopened.save_room_draft(room.id, String::new(), 2).unwrap();
        assert_eq!(empty.revision, 3);
        assert!(matches!(
            reopened.save_room_draft(room.id, "resurrected".into(), 0),
            Err(CoreError::InvalidInput(_))
        ));
        drop(reopened);
        assert_eq!(
            StudioStore::open(&path).unwrap().room_drafts().unwrap(),
            vec![empty]
        );
    }

    #[test]
    fn draft_size_or_unknown_room_rejected_and_schema_remains_strict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drafts.sqlite3");
        let store = StudioStore::open(&path).unwrap();
        let room = store.create_room("Safety").unwrap();
        assert!(matches!(
            store.save_room_draft(RoomId::new(), "oops".into(), 0),
            Err(CoreError::NotFound("room"))
        ));
        assert!(
            store
                .save_room_draft(room.id, "x".repeat(131_073), 0)
                .is_err()
        );
        store.save_room_draft(room.id, "safe".into(), 0).unwrap();
        drop(store);
        let conn = Connection::open(&path).unwrap();
        verify_initialized_schema(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER suspicious AFTER INSERT ON room_drafts BEGIN SELECT 1; END;",
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            StudioStore::open(&path),
            Err(CoreError::UnrecognizedSchema)
        ));
    }

    #[test]
    fn forgotten_credential_preserves_provider_identity_and_rejects_stale_profile() {
        let directory = tempfile::tempdir().unwrap();
        let store = StudioStore::open(directory.path().join("provider.sqlite3")).unwrap();
        let provider_id = ProviderId::new();
        let previous = ProviderProfile {
            id: provider_id,
            display_name: "Test provider".into(),
            model_id: "test-model".into(),
            base_url: "https://example.com/v1".into(),
            kind: Default::default(),
            credentials: CredentialSource::OsKeyring {
                service: "test-vault".into(),
                account: provider_id.to_string(),
            },
        };
        store.save_provider_profile(previous.clone()).unwrap();
        let stale = ProviderProfile {
            display_name: "wrong identity".into(),
            ..previous.clone()
        };
        assert!(matches!(
            store.forget_remembered_provider_credential(stale, OperationId::new()),
            Err(CoreError::OperationConflict)
        ));
        let operation = OperationId::new();
        store
            .forget_remembered_provider_credential(previous.clone(), operation)
            .unwrap();
        store
            .forget_remembered_provider_credential(previous.clone(), operation)
            .unwrap();
        let saved = store
            .snapshot()
            .unwrap()
            .providers
            .into_iter()
            .find(|provider| provider.id == provider_id)
            .unwrap();
        assert_eq!(saved.credentials, CredentialSource::None);
        assert_eq!(saved.display_name, previous.display_name);
        assert_eq!(saved.model_id, previous.model_id);
        assert!(matches!(
            store.forget_remembered_provider_credential(previous, OperationId::new()),
            Err(CoreError::OperationConflict)
        ));
    }

    #[test]
    fn local_send_clears_draft_in_same_transaction_and_replay_cannot_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("atomic-send.sqlite3");
        let store = StudioStore::open(&path).unwrap();
        let room = store.create_room("Atomic send").unwrap();
        let draft = store
            .save_room_draft(room.id, "  hello 🧪  ".into(), 0)
            .unwrap();
        let message_id = MessageId::new();
        let operation = OperationId::new();
        assert!(matches!(
            store.append_user_message_and_clear_draft(
                message_id,
                room.id,
                "hello 🧪".into(),
                0,
                operation,
            ),
            Err(CoreError::InvalidInput(_))
        ));
        assert_eq!(store.snapshot().unwrap().messages.len(), 0);
        assert_eq!(store.room_drafts().unwrap(), vec![draft.clone()]);
        let sent = store
            .append_user_message_and_clear_draft(
                message_id,
                room.id,
                "hello 🧪".into(),
                draft.revision,
                operation,
            )
            .unwrap();
        assert_eq!(sent.id, message_id);
        assert_eq!(sent.sender, MessageSender::User);
        assert_eq!(store.snapshot().unwrap().messages, vec![sent.clone()]);
        let cleared = store.room_drafts().unwrap();
        assert_eq!(cleared[0].content, "");
        assert_eq!(cleared[0].revision, draft.revision + 1);
        drop(store);

        let reopened = StudioStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .append_user_message_and_clear_draft(
                    message_id,
                    room.id,
                    "hello 🧪".into(),
                    draft.revision,
                    operation,
                )
                .unwrap(),
            sent
        );
        assert_eq!(reopened.snapshot().unwrap().messages, vec![sent]);
        assert_eq!(reopened.room_drafts().unwrap(), cleared);
        assert!(matches!(
            reopened.append_user_message_and_clear_draft(
                MessageId::new(),
                room.id,
                "hello 🧪".into(),
                draft.revision,
                OperationId::new(),
            ),
            Err(CoreError::OperationConflict)
        ));
        assert!(matches!(
            reopened.save_room_draft(room.id, "resurrect".into(), draft.revision),
            Err(CoreError::InvalidInput(_))
        ));
        assert_eq!(reopened.snapshot().unwrap().messages.len(), 1);
    }
}
