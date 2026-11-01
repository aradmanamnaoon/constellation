//! Host-only durable correlation between a model proposal and a native review.
//! This is evidence of origin, never permission or evidence of execution.
//! A missing or rolled-back row refuses recovery instead of reviving consent.
use std::{path::Path, time::Duration};

use constellation_agent_loop::ProposedAction;
use constellation_core::{OperationId, ProviderId, ProviderProfile, StudioStore, TaskState};
use constellation_tools::OperationContext;
use ring::hmac;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

const SCHEMA: &str = "CREATE TABLE model_review_bindings (
    operation_id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    room_id TEXT NOT NULL,
    checkpoint_revision INTEGER NOT NULL CHECK (checkpoint_revision > 0 AND checkpoint_revision <= 64),
    prepared_digest TEXT NOT NULL,
    mac BLOB NOT NULL
);";
const MAX_BOUND_MODEL_STEPS_PER_TASK: usize = 8;
// The original model-review binding has no provider or action field; do not
// retrofit invented provenance into signed legacy rows. Record the actual
// host-selected provider and independently reviewed operation action BEFORE
// native consent in a distinct, strictly validated credential-keyed ledger.
#[allow(dead_code)]
const OBSERVATION_SOURCE_SCHEMA: &str = "CREATE TABLE observation_sources (
    operation_id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    room_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    provider_profile_sha256 TEXT NOT NULL CHECK(length(provider_profile_sha256) = 64),
    checkpoint_revision INTEGER NOT NULL CHECK (checkpoint_revision > 0 AND checkpoint_revision <= 64),
    prepared_digest TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('read_file','create_file')),
    mac BLOB NOT NULL
);";

/// The only trusted provider/action origin for a subsequent *separate* ToolHost
/// effect. No values are inferred retrospectively from a model reply, task
/// affinity or global provider attempt counter.
#[allow(dead_code)]
pub(crate) struct NativeObservationSourceStore<'a> {
    path: &'a Path,
    key: &'a [u8; 32],
}

#[allow(dead_code)]
impl<'a> NativeObservationSourceStore<'a> {
    /// A strict routing commitment, *not* the credential value. Provider
    /// profiles only persist credential references, never API key material.
    fn provider_fingerprint(profile: &ProviderProfile) -> Result<String, String> {
        profile.validate().map_err(|_| denied())?;
        let encoded = serde_json::to_vec(&(
            "constellation.native-tool-provider-route.v1",
            profile.id,
            profile.kind,
            &profile.model_id,
            &profile.base_url,
            &profile.credentials,
        ))
        .map_err(|_| denied())?;
        Ok(format!("{:x}", Sha256::digest(encoded)))
    }
    pub(crate) fn new(path: &'a Path, key: &'a [u8; 32]) -> Result<Self, String> {
        if !path.is_absolute()
            || !path.parent().is_some_and(Path::is_dir)
            || key.iter().all(|byte| *byte == 0)
            || std::fs::symlink_metadata(path)
                .is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(denied());
        }
        Ok(Self { path, key })
    }

    fn action_name(action: ProposedAction) -> &'static str {
        match action {
            ProposedAction::ReadFile => "read_file",
            ProposedAction::CreateFile => "create_file",
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "source MAC binds full native operation and immutable provider route"
    )]
    fn tag(
        &self,
        operation: OperationId,
        context: OperationContext,
        provider: ProviderId,
        provider_profile_sha256: &str,
        revision: u64,
        digest: &str,
        action: ProposedAction,
    ) -> Vec<u8> {
        let mut bytes = b"constellation.native-tool-observation-source.v1\0".to_vec();
        for field in [
            operation.to_string(),
            context.task_id.to_string(),
            context.room_id.to_string(),
            context.agent_id.to_string(),
            provider.to_string(),
            provider_profile_sha256.to_owned(),
            revision.to_string(),
            digest.to_owned(),
            Self::action_name(action).into(),
        ] {
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, self.key), &bytes)
            .as_ref()
            .to_vec()
    }

    fn check_schema(conn: &Connection) -> Result<bool, String> {
        let mut statement = conn.prepare(
            "SELECT name,type,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name"
        ).map_err(|_| denied())?;
        let rows: Vec<(String, String, String)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|_| denied())?
            .collect::<Result<_, _>>()
            .map_err(|_| denied())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| denied())?;
        if rows.is_empty() && version == 0 {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
                .map_err(|_| denied())?;
            return if total == 0 { Ok(true) } else { Err(denied()) };
        }
        let reference = Connection::open_in_memory().map_err(|_| denied())?;
        reference
            .execute_batch(OBSERVATION_SOURCE_SCHEMA)
            .map_err(|_| denied())?;
        let expected: String = reference
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='observation_sources'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| denied())?;
        if version != 1 || rows != vec![("observation_sources".into(), "table".into(), expected)] {
            return Err(denied());
        }
        Ok(false)
    }

    /// Native-only creation point, before showing the OS consent UI. An
    /// operation is source-bound at most once: provider, action, context,
    /// digest and revision cannot later be edited, adopted or replayed.
    #[expect(
        clippy::too_many_arguments,
        reason = "native source must explicitly bind independent provider, action, task, operation, digest and revision"
    )]
    pub(crate) fn record(
        &self,
        core: &StudioStore,
        operation: OperationId,
        context: OperationContext,
        provider: ProviderId,
        revision: u64,
        digest: &str,
        action: ProposedAction,
    ) -> Result<(), String> {
        // Kept for existing deterministic fixtures only; the zero fingerprint
        // cannot authorize a production follow-up configuration.
        self.record_with_fingerprint(
            core,
            operation,
            context,
            provider,
            &"0".repeat(64),
            revision,
            digest,
            action,
        )
    }

    /// Production native source record, BEFORE independent user consent.
    /// Subsequent same-ID provider rewrites cannot silently change the route.
    #[expect(
        clippy::too_many_arguments,
        reason = "source binding must include all independent native provenance"
    )]
    pub(crate) fn record_with_profile(
        &self,
        core: &StudioStore,
        operation: OperationId,
        context: OperationContext,
        provider: ProviderId,
        revision: u64,
        digest: &str,
        action: ProposedAction,
        profile: &ProviderProfile,
    ) -> Result<(), String> {
        if provider != profile.id {
            return Err(denied());
        }
        let fingerprint = Self::provider_fingerprint(profile)?;
        self.record_with_fingerprint(
            core,
            operation,
            context,
            provider,
            &fingerprint,
            revision,
            digest,
            action,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "source binding must include all independent native provenance"
    )]
    fn record_with_fingerprint(
        &self,
        core: &StudioStore,
        operation: OperationId,
        context: OperationContext,
        provider: ProviderId,
        fingerprint: &str,
        revision: u64,
        digest: &str,
        action: ProposedAction,
    ) -> Result<(), String> {
        NativeModelBindingStore::validate_task(core, context)?;
        if !(1..=64).contains(&revision)
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || fingerprint.len() != 64
            || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(denied());
        }
        let mac = self.tag(
            operation,
            context,
            provider,
            fingerprint,
            revision,
            digest,
            action,
        );
        let mut conn = Connection::open_with_flags(
            self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|_| denied())?;
        conn.busy_timeout(Duration::from_secs(3))
            .map_err(|_| denied())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| denied())?;
        if Self::check_schema(&tx)? {
            tx.execute_batch(OBSERVATION_SOURCE_SCHEMA)
                .map_err(|_| denied())?;
            tx.pragma_update(None, "user_version", 1)
                .map_err(|_| denied())?;
        }
        tx.execute(
            "INSERT INTO observation_sources (operation_id,task_id,agent_id,room_id,provider_id,provider_profile_sha256,checkpoint_revision,prepared_digest,action,mac) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                operation.to_string(), context.task_id.to_string(), context.agent_id.to_string(),
                context.room_id.to_string(), provider.to_string(), fingerprint, revision as i64, digest,
                Self::action_name(action), mac,
            ],
        ).map_err(|_| denied())?;
        tx.commit().map_err(|_| denied())?;
        NativeModelBindingStore::validate_task(core, context)
    }

    /// Verification never creates or modifies the DB. Its only output is the
    /// fixed-vocabulary action from the independently HMAC-bound native row.
    pub(crate) fn verify(
        &self,
        core: &StudioStore,
        operation: OperationId,
        context: OperationContext,
        provider: ProviderId,
        revision: u64,
        digest: &str,
    ) -> Result<ProposedAction, String> {
        NativeModelBindingStore::validate_task(core, context)?;
        if !(1..=64).contains(&revision)
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !self.path.is_file()
        {
            return Err(denied());
        }
        let conn = Connection::open_with_flags(self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| denied())?;
        if Self::check_schema(&conn)? {
            return Err(denied());
        }
        let row: (String, String, String, String, String, i64, String, String, Vec<u8>) =
            conn.query_row(
                "SELECT task_id,agent_id,room_id,provider_id,provider_profile_sha256,checkpoint_revision,prepared_digest,action,mac FROM observation_sources WHERE operation_id=?1",
                [operation.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                         row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
            ).map_err(|_| denied())?;
        if row.0 != context.task_id.to_string()
            || row.1 != context.agent_id.to_string()
            || row.2 != context.room_id.to_string()
            || row.3 != provider.to_string()
            || row.4.len() != 64
            || !row.4.bytes().all(|byte| byte.is_ascii_hexdigit())
            || row.5 != revision as i64
            || row.6 != digest
        {
            return Err(denied());
        }
        let action = match row.7.as_str() {
            "read_file" => ProposedAction::ReadFile,
            "create_file" => ProposedAction::CreateFile,
            _ => return Err(denied()),
        };
        let authenticated = self.tag(
            operation, context, provider, &row.4, revision, digest, action,
        );
        if authenticated != row.8 {
            return Err(denied());
        }
        NativeModelBindingStore::validate_task(core, context)?;
        Ok(action)
    }

    /// Require the original provider's *exact* endpoint/protocol/model and
    /// credential reference to be unchanged before spending a follow-up.
    /// Existing zero-fingerprint test/legacy rows fail closed.
    pub(crate) fn verify_provider_configuration(
        &self,
        core: &StudioStore,
        operation: OperationId,
        context: OperationContext,
        revision: u64,
        digest: &str,
        profile: &ProviderProfile,
    ) -> Result<(), String> {
        let fingerprint = Self::provider_fingerprint(profile)?;
        let conn = Connection::open_with_flags(self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| denied())?;
        if Self::check_schema(&conn)? {
            return Err(denied());
        }
        let pinned: String = conn
            .query_row(
                "SELECT provider_profile_sha256 FROM observation_sources WHERE operation_id=?1",
                [operation.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| denied())?;
        if pinned != fingerprint || pinned == "0".repeat(64) {
            return Err(denied());
        }
        // Re-read and verify the full row/HMAC *after* comparing the proposed
        // route; an attacker editing SQLite between queries cannot promote
        // an unauthenticated post-check provider fingerprint.
        self.verify(core, operation, context, profile.id, revision, digest)?;
        NativeModelBindingStore::validate_task(core, context)
    }

    /// Recover the originally selected model provider from the independent
    /// native provenance row; never infer it from task affinity or the UI.
    /// The raw value is only a candidate until the complete HMAC is verified.
    pub(crate) fn verified_provider(
        &self,
        core: &StudioStore,
        operation: OperationId,
        context: OperationContext,
        revision: u64,
        digest: &str,
    ) -> Result<ProviderId, String> {
        NativeModelBindingStore::validate_task(core, context)?;
        if !self.path.is_file() {
            return Err(denied());
        }
        let conn = Connection::open_with_flags(self.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| denied())?;
        if Self::check_schema(&conn)? {
            return Err(denied());
        }
        let encoded: String = conn
            .query_row(
                "SELECT provider_id FROM observation_sources WHERE operation_id=?1",
                [operation.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| denied())?;
        let provider: ProviderId = encoded.parse().map_err(|_| denied())?;
        self.verify(core, operation, context, provider, revision, digest)?;
        Ok(provider)
    }
}

fn denied() -> String {
    "Native model operation binding cannot be authenticated".into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BoundModelReview {
    pub(crate) checkpoint_revision: u64,
    pub(crate) prepared_digest: String,
}

/// Key is independently obtained from the OS vault by the native ToolHost.
/// Callers must also verify the authenticated model checkpoint and ToolHost
/// result before considering another provider step. This ledger never contains
/// approval tokens, source bytes, or a renderer-controlled permission field.
pub(crate) struct NativeModelBindingStore<'a> {
    path: &'a Path,
    key: &'a [u8; 32],
}

impl<'a> NativeModelBindingStore<'a> {
    pub(crate) fn new(path: &'a Path, key: &'a [u8; 32]) -> Result<Self, String> {
        if !path.is_absolute()
            || !path.parent().is_some_and(Path::is_dir)
            || key.iter().all(|value| *value == 0)
        {
            return Err(denied());
        }
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && (!meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(denied());
        }
        Ok(Self { path, key })
    }

    fn open(&self, create: bool) -> Result<Connection, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | if create {
                OpenFlags::SQLITE_OPEN_CREATE
            } else {
                OpenFlags::empty()
            };
        let conn = Connection::open_with_flags(self.path, flags).map_err(|_| denied())?;
        conn.busy_timeout(Duration::from_secs(3))
            .map_err(|_| denied())?;
        Ok(conn)
    }

    fn check_schema(conn: &Connection) -> Result<bool, String> {
        let mut stmt = conn
            .prepare("SELECT name,type,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|_| denied())?;
        let observed: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|_| denied())?
            .collect::<Result<_, _>>()
            .map_err(|_| denied())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| denied())?;
        if observed.is_empty() && version == 0 {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| row.get(0))
                .map_err(|_| denied())?;
            return if count == 0 { Ok(true) } else { Err(denied()) };
        }
        let expected = Connection::open_in_memory().map_err(|_| denied())?;
        expected.execute_batch(SCHEMA).map_err(|_| denied())?;
        let sql: String = expected
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='model_review_bindings'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| denied())?;
        if version != 1 || observed != vec![("model_review_bindings".into(), "table".into(), sql)] {
            return Err(denied());
        }
        Ok(false)
    }

    fn tag(
        &self,
        id: OperationId,
        context: OperationContext,
        revision: u64,
        digest: &str,
    ) -> Vec<u8> {
        let mut payload = b"constellation.model-native-review-binding.v1\0".to_vec();
        for item in [
            id.to_string(),
            context.task_id.to_string(),
            context.agent_id.to_string(),
            context.room_id.to_string(),
            revision.to_string(),
            digest.to_owned(),
        ] {
            payload.extend_from_slice(item.as_bytes());
            payload.push(0);
        }
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.key);
        hmac::sign(&key, &payload).as_ref().to_vec()
    }

    fn validate_task(core: &StudioStore, context: OperationContext) -> Result<(), String> {
        let snapshot = core.snapshot().map_err(|_| denied())?;
        if snapshot.tasks.iter().any(|task| {
            task.id == context.task_id
                && task.agent_id == context.agent_id
                && task.room_id == context.room_id
                && task.state == TaskState::Pending
        }) {
            Ok(())
        } else {
            Err(denied())
        }
    }

    /// Write and commit before presenting any native consent UI. A duplicate
    /// operation ID, including one with the same bytes, is never re-registered.
    pub(crate) fn record(
        &self,
        core: &StudioStore,
        id: OperationId,
        context: OperationContext,
        checkpoint_revision: u64,
        prepared_digest: &str,
    ) -> Result<(), String> {
        Self::validate_task(core, context)?;
        if checkpoint_revision == 0
            || checkpoint_revision > 64
            || prepared_digest.len() != 64
            || !prepared_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(denied());
        }
        let tag = self.tag(id, context, checkpoint_revision, prepared_digest);
        let mut conn = self.open(true)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| denied())?;
        if Self::check_schema(&tx)? {
            tx.execute_batch(SCHEMA).map_err(|_| denied())?;
            tx.pragma_update(None, "user_version", 1)
                .map_err(|_| denied())?;
        }
        tx.execute(
            "INSERT INTO model_review_bindings
             (operation_id,task_id,agent_id,room_id,checkpoint_revision,prepared_digest,mac)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                id.to_string(),
                context.task_id.to_string(),
                context.agent_id.to_string(),
                context.room_id.to_string(),
                checkpoint_revision as i64,
                prepared_digest,
                tag,
            ],
        )
        .map_err(|_| denied())?;
        tx.commit().map_err(|_| denied())?;
        Self::validate_task(core, context)
    }

    /// Reads only a model-owned binding for an exact authoritative task.
    /// Do not interpret a valid binding as user consent or as a tool result.
    pub(crate) fn lookup(
        &self,
        core: &StudioStore,
        id: OperationId,
        context: OperationContext,
    ) -> Result<Option<BoundModelReview>, String> {
        Self::validate_task(core, context)?;
        if !self.path.is_file() {
            return Ok(None);
        }
        let conn = self.open(false)?;
        if Self::check_schema(&conn)? {
            return Ok(None);
        }
        let row: Option<(String, String, String, i64, String, Vec<u8>)> = conn
            .query_row(
                "SELECT task_id,agent_id,room_id,checkpoint_revision,prepared_digest,mac
                 FROM model_review_bindings WHERE operation_id=?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| denied())?;
        let Some((task, agent, room, revision, digest, mac)) = row else {
            return Ok(None);
        };
        if task != context.task_id.to_string()
            || agent != context.agent_id.to_string()
            || room != context.room_id.to_string()
            || !(1..=64).contains(&revision)
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(denied());
        }
        let expected = self.tag(id, context, revision as u64, &digest);
        let key = hmac::Key::new(hmac::HMAC_SHA256, self.key);
        // Exact signed bytes, constant-time comparison via ring::hmac.
        // Verify the tag as a message rather than relying on SQLite row values.
        let mut signed = b"constellation.model-native-review-binding.v1\0".to_vec();
        for item in [
            id.to_string(),
            task,
            agent,
            room,
            revision.to_string(),
            digest.clone(),
        ] {
            signed.extend_from_slice(item.as_bytes());
            signed.push(0);
        }
        hmac::verify(&key, &signed, &mac).map_err(|_| denied())?;
        // Verify domain-derived tag through the same routine used by record.
        if expected != mac {
            return Err(denied());
        }
        Self::validate_task(core, context)?;
        Ok(Some(BoundModelReview {
            checkpoint_revision: revision as u64,
            prepared_digest: digest,
        }))
    }

    /// Enumerate operations from a trusted task identity after a process
    /// restart. Every row must authenticate independently before the native
    /// journal may be queried. Missing rows are not proof no effect occurred;
    /// the caller must also match the separate anchored turn's step count.
    pub(crate) fn list_for_task(
        &self,
        core: &StudioStore,
        context: OperationContext,
    ) -> Result<Vec<(OperationId, BoundModelReview)>, String> {
        Self::validate_task(core, context)?;
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let conn = self.open(false)?;
        if Self::check_schema(&conn)? {
            return Err(denied());
        }
        let mut stmt = conn
            .prepare(
                "SELECT operation_id FROM model_review_bindings
                 WHERE task_id=?1 ORDER BY rowid",
            )
            .map_err(|_| denied())?;
        let ids: Vec<String> = stmt
            .query_map([context.task_id.to_string()], |row| row.get(0))
            .map_err(|_| denied())?
            .collect::<Result<_, _>>()
            .map_err(|_| denied())?;
        if ids.len() > MAX_BOUND_MODEL_STEPS_PER_TASK {
            return Err(denied());
        }
        ids.into_iter()
            .map(|id| {
                let id: OperationId = id.parse().map_err(|_| denied())?;
                let binding = self.lookup(core, id, context)?.ok_or_else(denied)?;
                Ok((id, binding))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        native_consent::NativeConsentProof,
        tool_host::ToolHost,
        turn_checkpoint::{Anchor, TrustedTurnStore},
    };
    use constellation_agent_loop::{AgentTurn, TurnEvent};
    use constellation_core::{AgentId, RoomId, TaskId};
    use constellation_tools::{JournalOperationState, LocalExecutor, OperationOutcome};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct TestAnchor(Arc<Mutex<HashMap<String, String>>>);

    impl Anchor for TestAnchor {
        fn read(&self, task: TaskId) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(&task.to_string()).cloned())
        }

        fn pin(&self, task: TaskId, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(task.to_string(), value.to_owned());
            Ok(())
        }
    }

    #[test]
    fn source_bound_provider_route_refuses_same_id_profile_substitution() {
        use constellation_agent_loop::ProposedAction;
        use constellation_core::{CredentialSource, ProviderKind};
        let folder = tempfile::tempdir().unwrap();
        let core = StudioStore::open(folder.path().join("core.sqlite3")).unwrap();
        let room = core.create_room("Provider route").unwrap();
        let agent = core.add_agent("Agent", "No direct tool use").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Task confidential")
            .unwrap();
        let context = OperationContext {
            task_id: task.id,
            room_id: room.id,
            agent_id: agent.id,
        };
        let operation = OperationId::new();
        let digest = "ab".repeat(32);
        let profile = ProviderProfile {
            id: ProviderId::new(),
            display_name: "Original".into(),
            kind: ProviderKind::OpenaiChat,
            model_id: "original-model".into(),
            base_url: "https://original.example/v1".into(),
            credentials: CredentialSource::OsKeyring {
                service: "fixture".into(),
                account: "original-account".into(),
            },
        };
        let path = folder.path().join("native-observation-sources.sqlite3");
        let key = [0x63; 32];
        let ledger = NativeObservationSourceStore::new(&path, &key).unwrap();
        ledger
            .record_with_profile(
                &core,
                operation,
                context,
                profile.id,
                2,
                &digest,
                ProposedAction::CreateFile,
                &profile,
            )
            .unwrap();
        assert!(
            ledger
                .verify_provider_configuration(&core, operation, context, 2, &digest, &profile,)
                .is_ok()
        );
        let mut renamed = profile.clone();
        renamed.display_name = "Only a label change".into();
        assert!(
            ledger
                .verify_provider_configuration(&core, operation, context, 2, &digest, &renamed,)
                .is_ok(),
            "a display label is not a routing change"
        );
        let mut variants = Vec::new();
        let mut changed = profile.clone();
        changed.base_url = "https://changed.example/v1".into();
        variants.push(changed);
        let mut changed = profile.clone();
        changed.model_id = "other-model".into();
        variants.push(changed);
        let mut changed = profile.clone();
        changed.kind = ProviderKind::AnthropicMessages;
        variants.push(changed);
        let mut changed = profile.clone();
        changed.credentials = CredentialSource::OsKeyring {
            service: "fixture".into(),
            account: "other-account".into(),
        };
        variants.push(changed);
        let mut changed = profile.clone();
        changed.id = ProviderId::new();
        variants.push(changed);
        for changed in variants {
            assert!(
                ledger
                    .verify_provider_configuration(&core, operation, context, 2, &digest, &changed,)
                    .is_err()
            );
        }
        assert!(
            ledger
                .record_with_profile(
                    &core,
                    operation,
                    context,
                    profile.id,
                    2,
                    &digest,
                    ProposedAction::CreateFile,
                    &profile,
                )
                .is_err(),
            "source record is exactly one use"
        );

        let legacy_path = folder.path().join("legacy-source.sqlite3");
        let legacy = NativeObservationSourceStore::new(&legacy_path, &key).unwrap();
        legacy
            .record(
                &core,
                OperationId::new(),
                context,
                profile.id,
                2,
                &digest,
                ProposedAction::CreateFile,
            )
            .unwrap();
        // Every legacy/unbound origin is intentionally non-promotable.
        let old_id: String = rusqlite::Connection::open(&legacy_path)
            .unwrap()
            .query_row("SELECT operation_id FROM observation_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(
            legacy
                .verify_provider_configuration(
                    &core,
                    old_id.parse().unwrap(),
                    context,
                    2,
                    &digest,
                    &profile,
                )
                .is_err()
        );
    }

    #[test]
    fn authenticated_model_origin_and_native_tool_result_reconcile_across_real_restart() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("core.sqlite3");
        let core = StudioStore::open(&db).unwrap();
        let room = core.create_room("Workspace").unwrap();
        let agent = core.add_agent("Agent", "No OS privileges").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Write an approved note")
            .unwrap();
        let ctx = OperationContext {
            agent_id: agent.id,
            room_id: room.id,
            task_id: task.id,
        };
        let workspace = root.path().join("artifacts");
        let audit = root.path().join("audit");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&audit).unwrap();
        let anchor = TestAnchor::default();
        let checkpoint = audit.join("turns.sqlite3");
        let trusted =
            TrustedTurnStore::with_anchor(&checkpoint, [0x11; 32], anchor.clone()).unwrap();
        let mut turn = AgentTurn::new(ctx);
        trusted
            .persist_pending_task(&core, ctx, &turn, None)
            .unwrap();
        let model_journal = audit.join("model-decisions.jsonl");
        let model_executor = LocalExecutor::with_authenticated_journal(
            &workspace,
            16384,
            16384,
            &model_journal,
            [0x22; 32],
        )
        .unwrap();
        let proposal = r#"{"type":"tool","action":"create_file","path":"approved.txt","content":"approved note"}"#;
        let model_op = match turn
            .submit_model_at_revision(0, proposal, &model_executor)
            .unwrap()
        {
            TurnEvent::ApprovalRequired { operation_id, .. } => operation_id,
            TurnEvent::Finished(_) => panic!("expected untrusted tool suggestion"),
        };
        turn.deny(&model_executor, model_op).unwrap();
        assert_eq!(
            (turn.revision(), turn.model_attempts(), turn.tool_steps()),
            (2, 1, 1)
        );
        trusted
            .persist_pending_task(&core, ctx, &turn, Some(0))
            .unwrap();
        drop(model_executor);
        assert!(!workspace.join("approved.txt").exists());

        let binding_path = audit.join("model-bindings.sqlite3");
        let key = [0x33; 32];
        let binding = NativeModelBindingStore::new(&binding_path, &key).unwrap();
        let mut host = ToolHost::open(&workspace, &audit, [0x44; 32]).unwrap();
        let preview = host
            .prepare(ctx, "create_new", "approved.txt", Some("approved note"))
            .unwrap();
        let id: OperationId = preview["operation_id"].as_str().unwrap().parse().unwrap();
        let ticket = preview["digest"].as_str().unwrap();
        let digest = host.pending_prepared_digest(id, ctx).unwrap();
        binding.record(&core, id, ctx, 2, &digest).unwrap();
        assert!(matches!(
            host.recovery_state(id, ctx, &digest).unwrap(),
            JournalOperationState::Unknown
        ));
        let native = host.pending_native_approval(id, ticket).unwrap();
        assert_eq!(host.pending_prepared_digest(id, ctx).unwrap(), digest);
        let result = host
            .approve_and_execute(
                id,
                ticket,
                NativeConsentProof::fixture_for_test(&id.to_string(), &native.digest),
            )
            .unwrap();
        assert_eq!(result["status"], "succeeded");
        drop(host);

        // Native OS consent cannot survive a restart; the separately signed
        // evidence of its result can. This never creates another permission.
        let restored_turn = TrustedTurnStore::with_anchor(&checkpoint, [0x11; 32], anchor)
            .unwrap()
            .restore_pending_task(&core, ctx)
            .unwrap()
            .unwrap();
        assert_eq!(
            (restored_turn.revision(), restored_turn.tool_steps()),
            (2, 1)
        );
        let rows = binding.list_for_task(&core, ctx).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, id);
        assert_eq!(rows[0].1.prepared_digest, digest);
        let reopened_host = ToolHost::open(&workspace, &audit, [0x44; 32]).unwrap();
        assert!(matches!(
            reopened_host.recovery_state(id, ctx, &digest).unwrap(),
            JournalOperationState::Completed {
                outcome: OperationOutcome::Succeeded,
                ..
            }
        ));
        assert!(reopened_host.pending_task(id).is_none());
        assert_eq!(
            std::fs::read_to_string(workspace.join("approved.txt")).unwrap(),
            "approved note"
        );
        assert!(
            reopened_host
                .recovery_state(
                    id,
                    OperationContext {
                        room_id: RoomId::new(),
                        ..ctx
                    },
                    &digest
                )
                .is_err()
        );
        core.transition_task(
            task.id,
            TaskState::Pending,
            TaskState::Cancelled,
            OperationId::new(),
            None,
        )
        .unwrap();
        assert!(binding.list_for_task(&core, ctx).is_err());
    }

    #[test]
    fn persisted_model_binding_is_exact_authenticated_and_never_an_approval() {
        let root = tempfile::tempdir().unwrap();
        let core = StudioStore::open(root.path().join("core.db")).unwrap();
        let room = core.create_room("Room").unwrap();
        let agent = core.add_agent("Agent", "No tool authority").unwrap();
        let task = core
            .create_task(room.id, agent.id, "Review proposal")
            .unwrap();
        let ctx = OperationContext {
            agent_id: agent.id,
            room_id: room.id,
            task_id: task.id,
        };
        let path = root.path().join("model-review.sqlite3");
        let id = OperationId::new();
        let digest = "a".repeat(64);
        let key = [0x24; 32];
        let binding = NativeModelBindingStore::new(&path, &key).unwrap();
        binding.record(&core, id, ctx, 2, &digest).unwrap();
        assert_eq!(
            binding.lookup(&core, id, ctx).unwrap(),
            Some(BoundModelReview {
                checkpoint_revision: 2,
                prepared_digest: digest.clone()
            })
        );
        let history = binding.list_for_task(&core, ctx).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, id);
        assert_eq!(history[0].1.checkpoint_revision, 2);
        assert!(binding.record(&core, id, ctx, 2, &digest).is_err());
        assert!(
            binding
                .lookup(
                    &core,
                    id,
                    OperationContext {
                        agent_id: AgentId::new(),
                        ..ctx
                    }
                )
                .is_err()
        );
        assert!(
            binding
                .lookup(
                    &core,
                    id,
                    OperationContext {
                        room_id: RoomId::new(),
                        ..ctx
                    }
                )
                .is_err()
        );
        assert!(
            NativeModelBindingStore::new(&path, &[0x33; 32])
                .unwrap()
                .lookup(&core, id, ctx)
                .is_err()
        );
        assert!(
            binding
                .lookup(&core, OperationId::new(), ctx)
                .unwrap()
                .is_none()
        );
        let db = Connection::open(&path).unwrap();
        db.execute(
            "UPDATE model_review_bindings SET prepared_digest=?1",
            ["b".repeat(64)],
        )
        .unwrap();
        assert!(binding.lookup(&core, id, ctx).is_err());
        assert!(
            binding
                .record(&core, OperationId::new(), ctx, 0, &digest)
                .is_err()
        );
        core.transition_task(
            task.id,
            TaskState::Pending,
            TaskState::Cancelled,
            OperationId::new(),
            None,
        )
        .unwrap();
        assert!(binding.lookup(&core, id, ctx).is_err());
    }

    #[test]
    fn foreign_or_corrupt_database_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let core = StudioStore::open(root.path().join("core.db")).unwrap();
        let room = core.create_room("Room").unwrap();
        let agent = core.add_agent("Agent", "No tools").unwrap();
        let task = core.create_task(room.id, agent.id, "Review").unwrap();
        let ctx = OperationContext {
            agent_id: agent.id,
            room_id: room.id,
            task_id: task.id,
        };
        let path = root.path().join("tampered.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE intruder (secret TEXT);")
            .unwrap();
        drop(conn);
        let key = [0x19; 32];
        let binding = NativeModelBindingStore::new(&path, &key).unwrap();
        assert!(
            binding
                .record(&core, OperationId::new(), ctx, 2, &"c".repeat(64))
                .is_err()
        );
        assert!(binding.lookup(&core, OperationId::new(), ctx).is_err());
    }
}
