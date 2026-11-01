//! Durable **local-only** revision store. Every head update is a SQLite
//! `BEGIN IMMEDIATE` compare-and-swap, not a remote/cloud provider capability.
//! This makes multi-process offline testing meaningful while preserving the
//! distinction between this storage and a verified S3/WebDAV adapter.

use std::{
    fs,
    path::{Component, Path},
    str::FromStr,
    sync::Mutex,
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{
    CasResult, PutResult, RevisionEnvelope, RevisionId, RevisionStore, StoreError,
    MAX_ENVELOPE_BYTES,
};

const MAX_LOCAL_REVISIONS: i64 = 1024;
const SCHEMA_VERSION: i64 = 1;

/// Versioned on-disk revision store for an explicitly selected local path.
/// Multiple processes coordinate through SQLite's transaction locks. No
/// credential, plaintext snapshot or network endpoint is stored here.
pub struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if !path.is_absolute()
            || path.file_name().is_none()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(StoreError::Failed);
        }
        #[cfg(windows)]
        {
            use std::path::Prefix;
            if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
            {
                return Err(StoreError::Failed);
            }
        }
        let parent = path.parent().ok_or(StoreError::Failed)?;
        // Do not create or traverse a missing parent implicitly.
        let parent = fs::canonicalize(parent).map_err(|_| StoreError::Failed)?;
        if !parent.is_dir() {
            return Err(StoreError::Failed);
        }
        let database = parent.join(path.file_name().ok_or(StoreError::Failed)?);
        if let Ok(meta) = fs::symlink_metadata(&database) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(StoreError::Failed);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(StoreError::Failed);
                }
            }
        }
        let mut connection = Connection::open(&database).map_err(|_| StoreError::Failed)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| StoreError::Failed)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON;
                 PRAGMA trusted_schema=OFF;
                 PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=FULL;",
            )
            .map_err(|_| StoreError::Failed)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| StoreError::Failed)?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| StoreError::Failed)?;
        if version != 0 && version != SCHEMA_VERSION {
            return Err(StoreError::Failed);
        }
        if version == 0 {
            let tables: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| StoreError::Failed)?;
            if tables != 0 {
                return Err(StoreError::Failed);
            }
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS sync_revisions (
                id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 64),
                envelope BLOB NOT NULL CHECK(length(envelope) BETWEEN 1 AND 1048974)
            ) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS sync_head (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                revision_id TEXT REFERENCES sync_revisions(id)
            );
            INSERT OR IGNORE INTO sync_head (singleton, revision_id) VALUES (1, NULL);",
        )
        .map_err(|_| StoreError::Failed)?;
        if version == 0 {
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(|_| StoreError::Failed)?;
        }
        tx.commit().map_err(|_| StoreError::Failed)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// Number of immutable encrypted revision envelopes, not plaintext sizes.
    pub fn revisions_len(&self) -> Result<usize, StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let n: i64 = connection
            .query_row("SELECT COUNT(*) FROM sync_revisions", [], |row| row.get(0))
            .map_err(|_| StoreError::Failed)?;
        usize::try_from(n).map_err(|_| StoreError::Failed)
    }
}

fn stored_id(value: Option<String>) -> Result<Option<RevisionId>, StoreError> {
    value
        .map(|id| RevisionId::from_str(&id).map_err(|_| StoreError::Failed))
        .transpose()
}

impl RevisionStore for SqliteStore {
    fn supports_verified_cas(&self) -> bool {
        true // Local SQLite IMMEDIATE transactions; never an S3 CAS assertion.
    }

    fn head(&self) -> Result<Option<RevisionId>, StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let value: Option<String> = connection
            .query_row(
                "SELECT revision_id FROM sync_head WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Failed)?;
        stored_id(value)
    }

    fn get_revision(
        &self,
        id: RevisionId,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let encoded = id.to_hex();
        let length: Option<i64> = connection
            .query_row(
                "SELECT length(envelope) FROM sync_revisions WHERE id=?1",
                [&encoded],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Failed)?;
        let Some(length) = length else {
            return Ok(None);
        };
        let cap = max_bytes.min(MAX_ENVELOPE_BYTES);
        if length <= 0 || usize::try_from(length).map_or(true, |n| n > cap) {
            return Err(StoreError::SizeLimit);
        }
        let bytes: Vec<u8> = connection
            .query_row(
                "SELECT envelope FROM sync_revisions WHERE id=?1",
                [&encoded],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Failed)?;
        if bytes.len() != length as usize
            || RevisionEnvelope::from_bytes(&bytes)
                .map(|revision| revision.id() != id)
                .unwrap_or(true)
        {
            return Err(StoreError::Failed);
        }
        Ok(Some(bytes))
    }

    fn put_if_absent(&self, id: RevisionId, bytes: &[u8]) -> Result<PutResult, StoreError> {
        if bytes.is_empty() || bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(StoreError::SizeLimit);
        }
        if RevisionEnvelope::from_bytes(bytes)
            .map(|revision| revision.id() != id)
            .unwrap_or(true)
        {
            return Err(StoreError::Failed);
        }
        let mut connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| StoreError::Failed)?;
        let key = id.to_hex();
        let present: Option<Vec<u8>> = tx
            .query_row(
                "SELECT envelope FROM sync_revisions WHERE id=?1",
                [&key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Failed)?;
        if let Some(present) = present {
            return if present == bytes {
                Ok(PutResult::AlreadyPresent)
            } else {
                Err(StoreError::ImmutableConflict)
            };
        }
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM sync_revisions", [], |row| row.get(0))
            .map_err(|_| StoreError::Failed)?;
        if count >= MAX_LOCAL_REVISIONS {
            return Err(StoreError::SizeLimit);
        }
        tx.execute(
            "INSERT INTO sync_revisions (id, envelope) VALUES (?1, ?2)",
            params![key, bytes],
        )
        .map_err(|_| StoreError::Failed)?;
        tx.commit().map_err(|_| StoreError::Failed)?;
        Ok(PutResult::Created)
    }

    fn compare_and_swap_head(
        &self,
        expected: Option<RevisionId>,
        next: RevisionId,
    ) -> Result<CasResult, StoreError> {
        let mut connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| StoreError::Failed)?;
        let current: Option<String> = tx
            .query_row(
                "SELECT revision_id FROM sync_head WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Failed)?;
        let current = stored_id(current)?;
        if current != expected {
            return Ok(CasResult::Conflict { current });
        }
        let key = next.to_hex();
        let exists: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sync_revisions WHERE id=?1)",
                [&key],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Failed)?;
        if exists != 1 {
            return Err(StoreError::Failed);
        }
        let affected = tx
            .execute(
                "UPDATE sync_head SET revision_id=?1 WHERE singleton=1",
                [&key],
            )
            .map_err(|_| StoreError::Failed)?;
        if affected != 1 {
            return Err(StoreError::Failed);
        }
        tx.commit().map_err(|_| StoreError::Failed)?;
        Ok(CasResult::Swapped)
    }
}
