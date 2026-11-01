//! E2EE revision protocol and a gated opt-in S3 HTTPS conditional transport.
//! No preconfigured providers, automatic merges, background work, or tool
//! permissions. Remote CAS is disabled until an explicit service probe passes.
//!
//! Revision IDs are content addresses of *encrypted* packages and authenticated
//! public metadata. The same bytes always have the same ID, but fresh secure
//! backup encryption intentionally gives distinct IDs for identical plaintext.

use std::{collections::BTreeSet, fmt, str::FromStr};

use constellation_backup::{BackupCodec, BackupError, BackupLimits};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

mod memory;
pub use memory::MemoryStore;
mod sqlite;
pub use sqlite::SqliteStore;
mod s3;
pub use s3::{HttpsS3Transport, RemoteS3Store, S3Credentials, S3Endpoint};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
pub const MAX_DEVICE_ID_BYTES: usize = 64;
/// Cap history traversal and Argon2 decrypts during an explicit recovery.
pub const MAX_RECOVERY_DEPTH: usize = 64;
pub const MAX_ROSTER_DEVICES: usize = 128;
const INNER_FIXED: usize = 8 + 2 + 2 + 1 + 4 + 32;
const OUTER_FIXED: usize = 8 + 2 + 2 + 1 + 4 + 32 + 32;
const BACKUP_OVERHEAD: usize = 140;
const MAX_INNER: usize = MAX_SNAPSHOT_BYTES + INNER_FIXED + MAX_DEVICE_ID_BYTES;
const MAX_PACKAGE: usize = MAX_INNER + BACKUP_OVERHEAD;
pub const MAX_ENVELOPE_BYTES: usize = OUTER_FIXED + MAX_DEVICE_ID_BYTES + MAX_PACKAGE;
const OUTER_MAGIC: &[u8; 8] = b"CSTLSYN1";
const INNER_MAGIC: &[u8; 8] = b"CSTLPAY1";
const REVISION_DOMAIN: &[u8] = b"constellation-sync-content-address-v1";

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("unsupported sync schema or reserved fields")]
    UnsupportedSchema,
    #[error("invalid device or revision identifier")]
    InvalidId,
    #[error("revision envelope or snapshot exceeds the byte limit")]
    SizeLimit,
    #[error("malformed revision envelope")]
    Malformed,
    #[error("revision content digest does not match")]
    DigestMismatch,
    #[error("authenticated encrypted metadata does not match public envelope")]
    MetadataMismatch,
    #[error("revision parent differs from the expected base")]
    ParentMismatch,
    #[error("previously stored revision cannot be published again")]
    Replay,
    #[error("storage does not guarantee atomic conditional create/CAS")]
    ConditionalWriteUnsupported,
    #[error("head differs from the trusted local checkpoint")]
    RollbackDetected,
    #[error("revision device is not in the caller's trusted local roster")]
    UntrustedDevice,
    #[error("trusted recovery ancestry exceeds its explicit bound")]
    RecoveryLimit,
    #[error("referenced immutable revision does not exist")]
    MissingRevision,
    #[error("storage data or conditional-write acknowledgement cannot be verified")]
    UnverifiedStorage,
    #[error("local snapshot no longer matches the independently trusted restore guard")]
    LocalChanges,
    #[error(transparent)]
    Crypto(#[from] BackupError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub type Result<T> = std::result::Result<T, SyncError>;

/// Public, opaque, bounded device identifier. It is authenticated *inside*
/// the ciphertext too, so a server cannot change its attribution.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(id: &str) -> Result<Self> {
        if id.is_empty()
            || id.len() > MAX_DEVICE_ID_BYTES
            || !id.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric())
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b':' | b'_' | b'-'))
        {
            return Err(SyncError::InvalidId);
        }
        Ok(Self(id.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Locally trusted device labels for *application-level* acceptance checks.
/// Device IDs are encrypted/authenticated but are NOT signed identities: an
/// actor retaining the shared encryption key can impersonate another label.
/// A host must separately revoke credentials and rotate shared keys.
#[derive(Clone, Default, Debug)]
pub struct DeviceRoster {
    allowed: BTreeSet<DeviceId>,
}

impl DeviceRoster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn authorize(&mut self, device: DeviceId) -> Result<()> {
        if !self.allowed.contains(&device) && self.allowed.len() >= MAX_ROSTER_DEVICES {
            return Err(SyncError::SizeLimit);
        }
        self.allowed.insert(device);
        Ok(())
    }

    /// Removing a label refuses future *checked* acceptance of that label.
    /// This does not revoke preexisting S3 credentials or shared key material.
    pub fn revoke(&mut self, device: &DeviceId) -> bool {
        self.allowed.remove(device)
    }

    pub fn permits(&self, device: &DeviceId) -> bool {
        self.allowed.contains(device)
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RevisionId([u8; 32]);

impl RevisionId {
    pub fn to_hex(self) -> String {
        format!("{:x}", HexDigest(self.0))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RevisionId({})", self.to_hex())
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for RevisionId {
    type Err = SyncError;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() != 64 {
            return Err(SyncError::InvalidId);
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            bytes[index] = (digit(chunk[0]).ok_or(SyncError::InvalidId)? << 4)
                | digit(chunk[1]).ok_or(SyncError::InvalidId)?;
        }
        Ok(Self(bytes))
    }
}

struct HexDigest([u8; 32]);
impl fmt::LowerHex for HexDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Version 1 binary envelope. Public parent/device metadata is duplicated
/// inside the authenticated encrypted payload and verified on every read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionEnvelope {
    id: RevisionId,
    device: DeviceId,
    parent: Option<RevisionId>,
    package: Vec<u8>,
}

impl RevisionEnvelope {
    pub fn id(&self) -> RevisionId {
        self.id
    }
    pub fn device(&self) -> &DeviceId {
        &self.device
    }
    pub fn parent(&self) -> Option<RevisionId> {
        self.parent
    }
    pub fn encrypted_len(&self) -> usize {
        self.package.len()
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(OUTER_FIXED + self.device.0.len() + self.package.len());
        bytes.extend_from_slice(OUTER_MAGIC);
        bytes.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(u16::from(self.parent.is_some())).to_le_bytes());
        bytes.push(self.device.0.len() as u8);
        bytes.extend_from_slice(&(self.package.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.id.0);
        bytes.extend_from_slice(&self.parent.map_or([0; 32], |parent| parent.0));
        bytes.extend_from_slice(self.device.0.as_bytes());
        bytes.extend_from_slice(&self.package);
        bytes
    }

    /// Strict bounded parsing. The ID binds device, parent and encrypted
    /// package bytes; only decrypting authenticates the sender's metadata.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(SyncError::SizeLimit);
        }
        if bytes.len() < OUTER_FIXED {
            return Err(SyncError::Malformed);
        }
        if &bytes[..8] != OUTER_MAGIC || u16_at(bytes, 8)? != PROTOCOL_VERSION {
            return Err(SyncError::UnsupportedSchema);
        }
        let flags = u16_at(bytes, 10)?;
        if flags > 1 {
            return Err(SyncError::UnsupportedSchema);
        }
        let device_len = usize::from(bytes[12]);
        let encrypted_len = u32_at(bytes, 13)? as usize;
        if device_len == 0 || device_len > MAX_DEVICE_ID_BYTES || encrypted_len > MAX_PACKAGE {
            return Err(SyncError::SizeLimit);
        }
        if bytes.len() != OUTER_FIXED + device_len + encrypted_len {
            return Err(SyncError::Malformed);
        }
        let id = digest_id(&bytes[17..49])?;
        let mut parent_bytes = [0; 32];
        parent_bytes.copy_from_slice(&bytes[49..81]);
        let parent = if flags == 1 {
            Some(RevisionId(parent_bytes))
        } else {
            if parent_bytes != [0; 32] {
                return Err(SyncError::Malformed);
            }
            None
        };
        let device_str =
            std::str::from_utf8(&bytes[81..81 + device_len]).map_err(|_| SyncError::InvalidId)?;
        let device = DeviceId::new(device_str)?;
        let package = bytes[81 + device_len..].to_vec();
        let expected = revision_digest(&device, parent, &package);
        if id != expected {
            return Err(SyncError::DigestMismatch);
        }
        Ok(Self {
            id,
            device,
            parent,
            package,
        })
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or(SyncError::Malformed)?
            .try_into()
            .map_err(|_| SyncError::Malformed)?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or(SyncError::Malformed)?
            .try_into()
            .map_err(|_| SyncError::Malformed)?,
    ))
}

fn digest_id(bytes: &[u8]) -> Result<RevisionId> {
    Ok(RevisionId(
        bytes.try_into().map_err(|_| SyncError::Malformed)?,
    ))
}

fn revision_digest(device: &DeviceId, parent: Option<RevisionId>, package: &[u8]) -> RevisionId {
    let mut hasher = Sha256::new();
    hasher.update(REVISION_DOMAIN);
    hasher.update(PROTOCOL_VERSION.to_le_bytes());
    hasher.update([u8::from(parent.is_some()), device.0.len() as u8]);
    hasher.update(device.0.as_bytes());
    hasher.update(parent.map_or([0; 32], |id| id.0));
    hasher.update((package.len() as u32).to_le_bytes());
    hasher.update(package);
    RevisionId(hasher.finalize().into())
}

/// Decrypted revision contents are zeroized on drop; avoid long-lived copies.
pub struct VerifiedRevision {
    pub id: RevisionId,
    pub device: DeviceId,
    pub parent: Option<RevisionId>,
    pub snapshot: Zeroizing<Vec<u8>>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum StoreError {
    #[error("storage does not support verifiable atomic conditional writes")]
    UnsupportedCas,
    #[error("immutable storage object differs from the supplied revision")]
    ImmutableConflict,
    #[error("storage resource limit exceeded")]
    SizeLimit,
    #[error("storage mutex poisoned")]
    Poisoned,
    #[error("storage operation failed")]
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PutResult {
    Created,
    AlreadyPresent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CasResult {
    Swapped,
    Conflict { current: Option<RevisionId> },
}

/// Adapter contract for user-controlled storage. Both conditional operations
/// MUST be backed by *atomic server-side conditional semantics*, never a
/// read/then-unconditional-write approximation. Default CAS is unsupported.
/// The max_bytes argument MUST limit data before allocating remote responses.
pub trait RevisionStore {
    fn supports_verified_cas(&self) -> bool {
        false
    }
    fn head(&self) -> std::result::Result<Option<RevisionId>, StoreError>;
    fn get_revision(
        &self,
        id: RevisionId,
        max_bytes: usize,
    ) -> std::result::Result<Option<Vec<u8>>, StoreError>;
    fn put_if_absent(
        &self,
        id: RevisionId,
        bytes: &[u8],
    ) -> std::result::Result<PutResult, StoreError>;
    fn compare_and_swap_head(
        &self,
        _expected: Option<RevisionId>,
        _next: RevisionId,
    ) -> std::result::Result<CasResult, StoreError> {
        Err(StoreError::UnsupportedCas)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishOutcome {
    Committed {
        revision_id: RevisionId,
    },
    Conflict {
        current_head: Option<RevisionId>,
        branch_id: RevisionId,
    },
}

/// Recovery evidence relative to a trusted checkpoint stored outside the
/// revision store. IDs are chronological; the current snapshot is decrypted.
/// The host must persist an accepted new checkpoint itself.
pub struct RecoveryPath {
    pub current: VerifiedRevision,
    pub revisions_since_checkpoint: Vec<RevisionId>,
}

/// Host-pinned local restore precondition. Persist the trusted revision ID and
/// digest independently of S3. The remote server never gets to select either.
/// This is an optimistic, read-only check: the caller must atomically recheck
/// both values against live local state before applying a preview.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestoreGuard {
    pub trusted_checkpoint: RevisionId,
    pub local_snapshot_sha256: [u8; 32],
}

impl RestoreGuard {
    pub fn from_trusted_snapshot(trusted_checkpoint: RevisionId, bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(SyncError::SizeLimit);
        }
        Ok(Self {
            trusted_checkpoint,
            local_snapshot_sha256: Sha256::digest(bytes).into(),
        })
    }

    /// Reconstruct only from independently trusted local persistence, never
    /// from a remote response or an unreviewed backup.
    pub fn from_pinned_digest(
        trusted_checkpoint: RevisionId,
        local_snapshot_sha256: [u8; 32],
    ) -> Self {
        Self {
            trusted_checkpoint,
            local_snapshot_sha256,
        }
    }
}

/// Pure sync coordinator with only explicit host calls. It keeps no
/// credentials, timers or external connections.
pub struct SyncCoordinator<S> {
    storage: S,
    codec: BackupCodec,
}

impl<S: RevisionStore> SyncCoordinator<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            codec: BackupCodec::new(
                BackupLimits::new(MAX_INNER).expect("compile-time bound within backup limit"),
            ),
        }
    }

    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Produce a *non-mutating* restore preview only when the supplied local
    /// snapshot still matches the host's independently pinned guard. The
    /// recovered branch must descend from that checkpoint and pass all usual
    /// encrypted ancestry and roster verification. Never overwrite local
    /// files here; the host must check the guard again under its own write
    /// transaction immediately before committing any restored bytes.
    pub fn preview_restore_if_unchanged(
        &self,
        guard: &RestoreGuard,
        local_snapshot: &[u8],
        roster: &DeviceRoster,
        passphrase: &[u8],
        max_depth: usize,
    ) -> Result<RecoveryPath> {
        if local_snapshot.len() > MAX_SNAPSHOT_BYTES {
            return Err(SyncError::SizeLimit);
        }
        if Sha256::digest(local_snapshot).as_slice() != guard.local_snapshot_sha256 {
            return Err(SyncError::LocalChanges);
        }
        self.recover_after_checkpoint(guard.trusted_checkpoint, roster, passphrase, max_depth)
    }

    pub fn prepare(
        &self,
        device: &DeviceId,
        parent: Option<RevisionId>,
        snapshot: &[u8],
        passphrase: &[u8],
    ) -> Result<RevisionEnvelope> {
        if snapshot.len() > MAX_SNAPSHOT_BYTES {
            return Err(SyncError::SizeLimit);
        }
        let mut inner = Zeroizing::new(Vec::with_capacity(
            INNER_FIXED + device.0.len() + snapshot.len(),
        ));
        inner.extend_from_slice(INNER_MAGIC);
        inner.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        inner.extend_from_slice(&(u16::from(parent.is_some())).to_le_bytes());
        inner.push(device.0.len() as u8);
        inner.extend_from_slice(&(snapshot.len() as u32).to_le_bytes());
        inner.extend_from_slice(&parent.map_or([0; 32], |id| id.0));
        inner.extend_from_slice(device.0.as_bytes());
        inner.extend_from_slice(snapshot);
        let package = self.codec.encrypt(&inner, passphrase)?;
        let id = revision_digest(device, parent, &package);
        Ok(RevisionEnvelope {
            id,
            device: device.clone(),
            parent,
            package,
        })
    }

    /// Decrypt and verify that public device/parent match authenticated
    /// private fields. This validates framing before the expensive KDF.
    pub fn decrypt(
        &self,
        envelope: &RevisionEnvelope,
        passphrase: &[u8],
    ) -> Result<VerifiedRevision> {
        if envelope.id != revision_digest(&envelope.device, envelope.parent, &envelope.package) {
            return Err(SyncError::DigestMismatch);
        }
        let mut inner = self.codec.decrypt(&envelope.package, passphrase)?;
        if inner.len() < INNER_FIXED {
            return Err(SyncError::Malformed);
        }
        if &inner[..8] != INNER_MAGIC || u16_at(&inner, 8)? != PROTOCOL_VERSION {
            return Err(SyncError::UnsupportedSchema);
        }
        let flags = u16_at(&inner, 10)?;
        if flags > 1 {
            return Err(SyncError::UnsupportedSchema);
        }
        let device_len = inner[12] as usize;
        let snapshot_len = u32_at(&inner, 13)? as usize;
        if device_len == 0 || device_len > MAX_DEVICE_ID_BYTES || snapshot_len > MAX_SNAPSHOT_BYTES
        {
            return Err(SyncError::SizeLimit);
        }
        if inner.len() != INNER_FIXED + device_len + snapshot_len {
            return Err(SyncError::Malformed);
        }
        let mut parent_bytes = [0; 32];
        parent_bytes.copy_from_slice(&inner[17..49]);
        let parsed_parent = if flags == 1 {
            Some(RevisionId(parent_bytes))
        } else {
            if parent_bytes != [0; 32] {
                return Err(SyncError::Malformed);
            }
            None
        };
        let parsed_device = DeviceId::new(
            std::str::from_utf8(&inner[49..49 + device_len]).map_err(|_| SyncError::InvalidId)?,
        )?;
        if parsed_parent != envelope.parent || parsed_device != envelope.device {
            return Err(SyncError::MetadataMismatch);
        }
        let snapshot = Zeroizing::new(inner.split_off(INNER_FIXED + device_len));
        Ok(VerifiedRevision {
            id: envelope.id,
            device: parsed_device,
            parent: parsed_parent,
            snapshot,
        })
    }

    pub fn fetch_revision(&self, id: RevisionId, passphrase: &[u8]) -> Result<VerifiedRevision> {
        let bytes = self
            .storage
            .get_revision(id, MAX_ENVELOPE_BYTES)?
            .ok_or(SyncError::MissingRevision)?;
        let envelope = RevisionEnvelope::from_bytes(&bytes)?;
        if envelope.id != id {
            return Err(SyncError::DigestMismatch);
        }
        self.decrypt(&envelope, passphrase)
    }

    /// The caller MUST supply a trusted checkpoint from outside storage.
    /// Without one, an old authentic head could be replayed undetected.
    pub fn verified_head(
        &self,
        trusted_checkpoint: Option<RevisionId>,
        passphrase: &[u8],
    ) -> Result<Option<VerifiedRevision>> {
        let current = self.storage.head()?;
        if current != trusted_checkpoint {
            return Err(SyncError::RollbackDetected);
        }
        current
            .map(|id| self.fetch_revision(id, passphrase))
            .transpose()
    }

    /// Examine a possibly advanced remote head, authenticating every ancestor
    /// back to a trusted checkpoint and checking device labels on the *new*
    /// path. This never advances local trust or writes remote storage.
    ///
    /// A missing ancestor, rolled-back head, alternate branch, revoked label,
    /// malformed ciphertext or oversized history fails closed. An honest
    /// concurrently advancing head can require the caller to retry explicitly.
    pub fn recover_after_checkpoint(
        &self,
        trusted_checkpoint: RevisionId,
        roster: &DeviceRoster,
        passphrase: &[u8],
        max_depth: usize,
    ) -> Result<RecoveryPath> {
        if max_depth == 0 || max_depth > MAX_RECOVERY_DEPTH {
            return Err(SyncError::RecoveryLimit);
        }
        let head = self.storage.head()?.ok_or(SyncError::RollbackDetected)?;
        let mut seen = BTreeSet::new();
        let mut cursor = head;
        let mut path = Vec::new();
        let mut tip = None;
        while cursor != trusted_checkpoint {
            if path.len() >= max_depth {
                return Err(SyncError::RecoveryLimit);
            }
            if !seen.insert(cursor) {
                return Err(SyncError::RollbackDetected);
            }
            let current = self.fetch_revision(cursor, passphrase)?;
            let parent = current.parent.ok_or(SyncError::RollbackDetected)?;
            if !roster.permits(&current.device) {
                return Err(SyncError::UntrustedDevice);
            }
            if tip.is_none() {
                tip = Some(current);
            }
            path.push(cursor);
            cursor = parent;
        }
        // A remote server could retain a valid head and corrupt or delete the
        // trusted ancestor; anchor readability/authenticity is mandatory.
        let anchor = self.fetch_revision(trusted_checkpoint, passphrase)?;
        if self.storage.head()? != Some(head) {
            return Err(SyncError::UnverifiedStorage);
        }
        path.reverse();
        Ok(RecoveryPath {
            current: tip.unwrap_or(anchor),
            revisions_since_checkpoint: path,
        })
    }

    /// Publish only on the exact externally trusted local checkpoint and only
    /// with a currently allowed device label. If the server has been rolled
    /// back or another writer has advanced it, do not overwrite silently.
    pub fn publish_from_checkpoint(
        &self,
        envelope: &RevisionEnvelope,
        trusted_checkpoint: RevisionId,
        roster: &DeviceRoster,
        passphrase: &[u8],
    ) -> Result<PublishOutcome> {
        if !roster.permits(&envelope.device) {
            return Err(SyncError::UntrustedDevice);
        }
        if envelope.parent != Some(trusted_checkpoint) {
            return Err(SyncError::ParentMismatch);
        }
        self.verified_head(Some(trusted_checkpoint), passphrase)?;
        self.publish(envelope, Some(trusted_checkpoint), passphrase)
    }

    /// Store an immutable candidate before atomically CAS-updating the head.
    /// When two devices race, the losing candidate remains retrievable by
    /// branch_id. No blind overwrite or automatic merge is performed.
    pub fn publish(
        &self,
        envelope: &RevisionEnvelope,
        expected_parent: Option<RevisionId>,
        passphrase: &[u8],
    ) -> Result<PublishOutcome> {
        if !self.storage.supports_verified_cas() {
            return Err(SyncError::ConditionalWriteUnsupported);
        }
        if envelope.parent != expected_parent {
            return Err(SyncError::ParentMismatch);
        }
        self.decrypt(envelope, passphrase)?;
        // A forged/missing parent object must not become a valid new head.
        // Inspect the claimed parent using the same passphrase and CID.
        if let Some(parent) = expected_parent {
            self.fetch_revision(parent, passphrase)?;
        }
        let bytes = envelope.to_bytes();
        match self.storage.put_if_absent(envelope.id, &bytes)? {
            PutResult::Created => {}
            PutResult::AlreadyPresent => {
                let original = self
                    .storage
                    .get_revision(envelope.id, MAX_ENVELOPE_BYTES)?
                    .ok_or(SyncError::UnverifiedStorage)?;
                if original != bytes {
                    return Err(SyncError::UnverifiedStorage);
                }
                return Err(SyncError::Replay);
            }
        }
        match self
            .storage
            .compare_and_swap_head(expected_parent, envelope.id)?
        {
            CasResult::Swapped => {
                // Recheck the new head; an untrusted adapter that acknowledges
                // without storing cannot silently yield success.
                if self.storage.head()? != Some(envelope.id) {
                    return Err(SyncError::UnverifiedStorage);
                }
                let committed = self
                    .storage
                    .get_revision(envelope.id, MAX_ENVELOPE_BYTES)?
                    .ok_or(SyncError::UnverifiedStorage)?;
                if committed != bytes {
                    return Err(SyncError::UnverifiedStorage);
                }
                Ok(PublishOutcome::Committed {
                    revision_id: envelope.id,
                })
            }
            CasResult::Conflict { current } => {
                if self.storage.head()? != current {
                    return Err(SyncError::UnverifiedStorage);
                }
                // Fail closed if the reported winning head cannot be read and
                // authenticated. The losing immutable branch remains stored.
                if let Some(id) = current {
                    self.fetch_revision(id, passphrase)?;
                }
                Ok(PublishOutcome::Conflict {
                    current_head: current,
                    branch_id: envelope.id,
                })
            }
        }
    }
}
