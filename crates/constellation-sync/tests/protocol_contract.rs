use std::{
    sync::{Arc, Barrier},
    thread,
};

use constellation_backup::{BackupCodec, BackupLimits};
use constellation_sync::{
    CasResult, DeviceId, DeviceRoster, MemoryStore, PublishOutcome, PutResult, RestoreGuard,
    RevisionEnvelope, RevisionId, RevisionStore, StoreError, SyncCoordinator, SyncError,
    MAX_ENVELOPE_BYTES, MAX_RECOVERY_DEPTH, MAX_SNAPSHOT_BYTES,
};
use sha2::{Digest, Sha256};

const PASSPHRASE: &[u8] = b"high entropy offline recovery passphrase";

fn device(name: &str) -> DeviceId {
    DeviceId::new(name).unwrap()
}

#[test]
fn restore_preview_rejects_dirty_local_edits_and_remote_rollback_without_writing_local_state() {
    let storage = MemoryStore::new();
    let sync = SyncCoordinator::new(storage.clone());
    let local = b"locally-persisted-snapshot";
    let root = sync
        .prepare(&device("owner"), None, local, PASSPHRASE)
        .unwrap();
    sync.publish(&root, None, PASSPHRASE).unwrap();
    let next = sync
        .prepare(
            &device("peer"),
            Some(root.id()),
            b"peer encrypted edit",
            PASSPHRASE,
        )
        .unwrap();
    sync.publish(&next, Some(root.id()), PASSPHRASE).unwrap();
    let mut roster = DeviceRoster::new();
    roster.authorize(device("peer")).unwrap();
    let guard = RestoreGuard::from_trusted_snapshot(root.id(), local).unwrap();
    let pinned =
        RestoreGuard::from_pinned_digest(guard.trusted_checkpoint, guard.local_snapshot_sha256);
    assert_eq!(guard, pinned);
    assert!(matches!(
        sync.preview_restore_if_unchanged(&guard, b"local unsaved edit", &roster, PASSPHRASE, 2),
        Err(SyncError::LocalChanges)
    ));
    let preview = sync
        .preview_restore_if_unchanged(&guard, local, &roster, PASSPHRASE, 2)
        .unwrap();
    assert_eq!(preview.current.id, next.id());
    assert_eq!(preview.current.snapshot.as_slice(), b"peer encrypted edit");
    assert_eq!(preview.revisions_since_checkpoint, vec![next.id()]);
    // The preview cannot mutate the local buffer or any host checkpoint.
    assert_eq!(local, b"locally-persisted-snapshot");
    assert_eq!(storage.head().unwrap(), Some(next.id()));

    // Reverting cloud head to an earlier real revision cannot be mistaken for
    // a safe restore, even though the encrypted object still authenticates.
    storage
        .compare_and_swap_head(Some(next.id()), root.id())
        .unwrap();
    let stale = sync.preview_restore_if_unchanged(
        &RestoreGuard::from_pinned_digest(next.id(), guard.local_snapshot_sha256),
        local,
        &roster,
        PASSPHRASE,
        2,
    );
    assert!(matches!(stale, Err(SyncError::RollbackDetected)));
}

#[test]
fn restore_preview_rejects_unapproved_remote_branch_even_if_ciphertext_is_valid() {
    let storage = MemoryStore::new();
    let sync = SyncCoordinator::new(storage);
    let root = sync
        .prepare(&device("owner"), None, b"anchor", PASSPHRASE)
        .unwrap();
    sync.publish(&root, None, PASSPHRASE).unwrap();
    let hostile = sync
        .prepare(
            &device("revoked"),
            Some(root.id()),
            b"unauthorized restore",
            PASSPHRASE,
        )
        .unwrap();
    sync.publish(&hostile, Some(root.id()), PASSPHRASE).unwrap();
    let mut roster = DeviceRoster::new();
    roster.authorize(device("revoked")).unwrap();
    assert!(roster.revoke(&device("revoked")));
    let guard = RestoreGuard::from_trusted_snapshot(root.id(), b"anchor").unwrap();
    assert!(matches!(
        sync.preview_restore_if_unchanged(&guard, b"anchor", &roster, PASSPHRASE, 2),
        Err(SyncError::UntrustedDevice)
    ));
}

#[test]
fn guarded_checkpoint_recovery_enforces_ancestry_and_local_revocation() {
    let store = MemoryStore::new();
    let sync = SyncCoordinator::new(store.clone());
    let root = sync
        .prepare(&device("owner"), None, b"root", PASSPHRASE)
        .unwrap();
    sync.publish(&root, None, PASSPHRASE).unwrap();
    let first = sync
        .prepare(&device("alice"), Some(root.id()), b"edited", PASSPHRASE)
        .unwrap();
    let second = sync
        .prepare(
            &device("bob"),
            Some(first.id()),
            b"edited-again",
            PASSPHRASE,
        )
        .unwrap();
    let mut roster = DeviceRoster::new();
    roster.authorize(device("alice")).unwrap();
    roster.authorize(device("bob")).unwrap();
    assert!(matches!(
        sync.publish_from_checkpoint(&first, root.id(), &roster, PASSPHRASE),
        Ok(PublishOutcome::Committed { .. })
    ));
    sync.publish_from_checkpoint(&second, first.id(), &roster, PASSPHRASE)
        .unwrap();
    let path = sync
        .recover_after_checkpoint(root.id(), &roster, PASSPHRASE, 2)
        .unwrap();
    assert_eq!(
        path.revisions_since_checkpoint,
        vec![first.id(), second.id()]
    );
    assert_eq!(path.current.id, second.id());
    assert_eq!(path.current.snapshot.as_slice(), b"edited-again");
    assert!(matches!(
        sync.recover_after_checkpoint(root.id(), &roster, PASSPHRASE, 1),
        Err(SyncError::RecoveryLimit)
    ));
    assert!(matches!(
        sync.recover_after_checkpoint(root.id(), &roster, PASSPHRASE, MAX_RECOVERY_DEPTH + 1),
        Err(SyncError::RecoveryLimit)
    ));

    assert!(roster.revoke(&device("bob")));
    assert!(!roster.permits(&device("bob")));
    assert!(matches!(
        sync.recover_after_checkpoint(root.id(), &roster, PASSPHRASE, 2),
        Err(SyncError::UntrustedDevice)
    ));
    let future = sync
        .prepare(
            &device("bob"),
            Some(second.id()),
            b"revoked edit",
            PASSPHRASE,
        )
        .unwrap();
    let previous_object_count = store.revisions_len().unwrap();
    assert!(matches!(
        sync.publish_from_checkpoint(&future, second.id(), &roster, PASSPHRASE),
        Err(SyncError::UntrustedDevice)
    ));
    assert_eq!(store.revisions_len().unwrap(), previous_object_count);
    assert_eq!(store.head().unwrap(), Some(second.id()));
}

#[test]
fn hostile_rollback_or_unrelated_head_cannot_be_recovered_or_published_over() {
    let store = MemoryStore::new();
    let sync = SyncCoordinator::new(store.clone());
    let anchor = sync
        .prepare(&device("owner"), None, b"anchor", PASSPHRASE)
        .unwrap();
    sync.publish(&anchor, None, PASSPHRASE).unwrap();
    let next = sync
        .prepare(&device("alice"), Some(anchor.id()), b"next", PASSPHRASE)
        .unwrap();
    sync.publish(&next, Some(anchor.id()), PASSPHRASE).unwrap();
    let mut roster = DeviceRoster::new();
    roster.authorize(device("alice")).unwrap();
    assert_eq!(
        sync.recover_after_checkpoint(next.id(), &roster, PASSPHRASE, 1)
            .unwrap()
            .revisions_since_checkpoint,
        vec![]
    );
    // A malicious operator can replay a real earlier immutable revision as
    // head. The local checkpoint must independently forbid this.
    assert_eq!(
        store
            .compare_and_swap_head(Some(next.id()), anchor.id())
            .unwrap(),
        CasResult::Swapped
    );
    let prepared = sync
        .prepare(
            &device("alice"),
            Some(next.id()),
            b"must not publish",
            PASSPHRASE,
        )
        .unwrap();
    assert!(matches!(
        sync.publish_from_checkpoint(&prepared, next.id(), &roster, PASSPHRASE),
        Err(SyncError::RollbackDetected)
    ));
    assert!(matches!(
        sync.recover_after_checkpoint(next.id(), &roster, PASSPHRASE, 1),
        Err(SyncError::RollbackDetected)
    ));
    assert_eq!(store.revisions_len().unwrap(), 2);
    assert_eq!(store.head().unwrap(), Some(anchor.id()));

    let separate = sync
        .prepare(&device("alice"), None, b"unrelated root", PASSPHRASE)
        .unwrap();
    assert_eq!(
        store
            .put_if_absent(separate.id(), &separate.to_bytes())
            .unwrap(),
        PutResult::Created
    );
    store
        .compare_and_swap_head(Some(anchor.id()), separate.id())
        .unwrap();
    assert!(matches!(
        sync.recover_after_checkpoint(next.id(), &roster, PASSPHRASE, 2),
        Err(SyncError::RollbackDetected)
    ));
}

fn cid_hex(bytes: &[u8]) -> RevisionId {
    let mut changed = bytes.to_vec();
    // Rehash public metadata while deliberately leaving encrypted inner
    // metadata unchanged. The encrypted metadata check must catch the forgery.
    let device_len = changed[12] as usize;
    let package = &changed[81 + device_len..];
    let mut hasher = Sha256::new();
    hasher.update(b"constellation-sync-content-address-v1");
    hasher.update(1u16.to_le_bytes());
    hasher.update([changed[10], changed[12]]);
    hasher.update(&changed[81..81 + device_len]);
    hasher.update(&changed[49..81]);
    hasher.update((package.len() as u32).to_le_bytes());
    hasher.update(package);
    let hash: [u8; 32] = hasher.finalize().into();
    changed[17..49].copy_from_slice(&hash);
    RevisionEnvelope::from_bytes(&changed).unwrap().id()
}

#[test]
fn envelope_roundtrip_e2ee_and_deterministic_ciphertext_content_address() {
    let sync = SyncCoordinator::new(MemoryStore::new());
    let fixture = b"sensitive research notes\0\x80\xff";
    let first = sync
        .prepare(&device("computer-a"), None, fixture, PASSPHRASE)
        .unwrap();
    let raw = first.to_bytes();
    assert_eq!(raw.len(), first.encrypted_len() + 81 + "computer-a".len());
    assert!(!raw.windows(fixture.len()).any(|slice| slice == fixture));
    let recovered = RevisionEnvelope::from_bytes(&raw).unwrap();
    assert_eq!(recovered.id(), first.id());
    assert_eq!(recovered.parent(), None);
    assert_eq!(recovered.device().as_str(), "computer-a");
    assert_eq!(recovered.to_bytes(), raw);
    assert_eq!(
        sync.decrypt(&recovered, PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        fixture
    );
    assert!(matches!(
        sync.decrypt(&recovered, b"definitely wrong private passphrase"),
        Err(SyncError::Crypto(_))
    ));
    assert_eq!(
        recovered.id().to_hex().parse::<RevisionId>().unwrap(),
        first.id()
    );
    assert!(first
        .id()
        .to_hex()
        .to_uppercase()
        .parse::<RevisionId>()
        .is_err());
    assert!("0".repeat(63).parse::<RevisionId>().is_err());
    // Same *ciphertext* gives same CID; new encryption uses a random salt and
    // nonce, so identical plaintext intentionally yields a different CID.
    let second = sync
        .prepare(&device("computer-a"), None, fixture, PASSPHRASE)
        .unwrap();
    assert_ne!(first.id(), second.id());
}

#[test]
fn version_flags_size_and_metadata_forgery_fail_before_publish() {
    let store = MemoryStore::new();
    let sync = SyncCoordinator::new(store.clone());
    let original = sync
        .prepare(&device("workstation"), None, b"private", PASSPHRASE)
        .unwrap();
    let encoded = original.to_bytes();
    for cut in [0, 1, 8, 17, 49, 80, 81, encoded.len() - 1] {
        assert!(RevisionEnvelope::from_bytes(&encoded[..cut]).is_err());
    }
    let mut bad = encoded.clone();
    bad[8] = 99;
    assert!(matches!(
        RevisionEnvelope::from_bytes(&bad),
        Err(SyncError::UnsupportedSchema)
    ));
    let mut bad = encoded.clone();
    bad[10] = 0x10;
    assert!(matches!(
        RevisionEnvelope::from_bytes(&bad),
        Err(SyncError::UnsupportedSchema)
    ));
    let mut bad = encoded.clone();
    bad[13..17].copy_from_slice(&(u32::MAX).to_le_bytes());
    assert!(RevisionEnvelope::from_bytes(&bad).is_err());
    let mut bad = encoded.clone();
    bad[17] ^= 0x01;
    assert!(matches!(
        RevisionEnvelope::from_bytes(&bad),
        Err(SyncError::DigestMismatch)
    ));
    // Tampering with public device and recomputing the unkeyed CID cannot
    // forge the original encrypted device attribution.
    let mut bad = encoded.clone();
    bad[81] = b'x';
    let forged = cid_hex(&bad);
    let mut forged_bytes = bad;
    forged_bytes[17..49].copy_from_slice(forged.as_bytes());
    let forged_envelope = RevisionEnvelope::from_bytes(&forged_bytes).unwrap();
    assert!(matches!(
        sync.decrypt(&forged_envelope, PASSPHRASE),
        Err(SyncError::MetadataMismatch)
    ));
    assert!(matches!(
        sync.publish(&forged_envelope, None, PASSPHRASE),
        Err(SyncError::MetadataMismatch)
    ));
    assert_eq!(store.revisions_len().unwrap(), 0);

    assert!(matches!(
        sync.prepare(
            &device("a"),
            None,
            &vec![0x12; MAX_SNAPSHOT_BYTES + 1],
            PASSPHRASE
        ),
        Err(SyncError::SizeLimit)
    ));
    let huge = vec![0u8; MAX_ENVELOPE_BYTES + 1];
    assert!(matches!(
        RevisionEnvelope::from_bytes(&huge),
        Err(SyncError::SizeLimit)
    ));
    for bad_id in ["", "../evil", "trailing ", "a\0b", "ä", "x/y"] {
        assert!(DeviceId::new(bad_id).is_err());
    }
    assert!(DeviceId::new(&"a".repeat(65)).is_err());
}

#[test]
fn malicious_authenticated_inner_schema_rejected_without_side_effects() {
    let store = MemoryStore::new();
    let sync = SyncCoordinator::new(store.clone());
    let good = sync
        .prepare(&device("alice"), None, b"snapshot", PASSPHRASE)
        .unwrap();
    let mut raw = good.to_bytes();
    // An attacker who knows the passphrase could create an encrypted payload
    // with unsupported inner schema. Header authentication alone is not enough.
    let mut invalid_inner = Vec::new();
    invalid_inner.extend_from_slice(b"CSTLPAY1");
    invalid_inner.extend_from_slice(&999u16.to_le_bytes());
    invalid_inner.extend_from_slice(&0u16.to_le_bytes());
    invalid_inner.push(5);
    invalid_inner.extend_from_slice(&1u32.to_le_bytes());
    invalid_inner.extend_from_slice(&[0; 32]);
    invalid_inner.extend_from_slice(b"alice");
    invalid_inner.push(42);
    let bad_package = BackupCodec::new(BackupLimits::new(2 * 1024 * 1024).unwrap())
        .encrypt(&invalid_inner, PASSPHRASE)
        .unwrap();
    raw[13..17].copy_from_slice(&(bad_package.len() as u32).to_le_bytes());
    raw.truncate(81 + 5);
    raw.extend_from_slice(&bad_package);
    let new_id = cid_hex(&raw);
    raw[17..49].copy_from_slice(new_id.as_bytes());
    let malformed = RevisionEnvelope::from_bytes(&raw).unwrap();
    assert!(matches!(
        sync.decrypt(&malformed, PASSPHRASE),
        Err(SyncError::UnsupportedSchema)
    ));
    assert!(matches!(
        sync.publish(&malformed, None, PASSPHRASE),
        Err(SyncError::UnsupportedSchema)
    ));
    assert_eq!(store.revisions_len().unwrap(), 0);
}

#[test]
fn two_devices_race_atomic_cas_preserves_losing_branch_and_manual_merge() {
    let shared = MemoryStore::new();
    let coordinator = SyncCoordinator::new(shared.clone());
    let genesis = coordinator
        .prepare(&device("laptop"), None, b"base", PASSPHRASE)
        .unwrap();
    assert_eq!(
        coordinator.publish(&genesis, None, PASSPHRASE).unwrap(),
        PublishOutcome::Committed {
            revision_id: genesis.id()
        }
    );
    let base_id = genesis.id();
    assert_eq!(
        coordinator
            .verified_head(Some(base_id), PASSPHRASE)
            .unwrap()
            .unwrap()
            .snapshot
            .as_slice(),
        b"base"
    );
    let a = coordinator
        .prepare(&device("alice"), Some(base_id), b"branch-A", PASSPHRASE)
        .unwrap();
    let b = coordinator
        .prepare(&device("bob"), Some(base_id), b"branch-B", PASSPHRASE)
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let workers = vec![a.clone(), b.clone()]
        .into_iter()
        .map(|prepared| {
            let storage = shared.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let sync = SyncCoordinator::new(storage);
                barrier.wait();
                (
                    prepared.id(),
                    sync.publish(&prepared, Some(base_id), PASSPHRASE).unwrap(),
                )
            })
        })
        .collect::<Vec<_>>();
    let published: Vec<_> = workers.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        published
            .iter()
            .filter(|(_, outcome)| matches!(outcome, PublishOutcome::Committed { .. }))
            .count(),
        1
    );
    assert_eq!(
        published
            .iter()
            .filter(|(_, outcome)| matches!(outcome, PublishOutcome::Conflict { .. }))
            .count(),
        1
    );
    let head = shared.head().unwrap().unwrap();
    let losing = published
        .iter()
        .find_map(|(_, result)| match result {
            PublishOutcome::Conflict {
                current_head,
                branch_id,
            } => {
                assert_eq!(*current_head, Some(head));
                Some(*branch_id)
            }
            _ => None,
        })
        .unwrap();
    assert_ne!(losing, head);
    assert_eq!(shared.revisions_len().unwrap(), 3);
    let winner_data = coordinator.fetch_revision(head, PASSPHRASE).unwrap();
    let loser_data = coordinator.fetch_revision(losing, PASSPHRASE).unwrap();
    assert_eq!(winner_data.parent, Some(base_id));
    assert_eq!(loser_data.parent, Some(base_id));
    assert_ne!(*winner_data.snapshot, *loser_data.snapshot);
    assert!(matches!(
        coordinator.verified_head(Some(base_id), PASSPHRASE),
        Err(SyncError::RollbackDetected)
    ));
    // No automatic content merge: trusted host or user explicitly constructs
    // a merged snapshot, using the winning head as its new parent.
    let merged = coordinator
        .prepare(
            &device("reviewer"),
            Some(head),
            b"merged: branch-A + branch-B",
            PASSPHRASE,
        )
        .unwrap();
    assert_eq!(
        coordinator
            .publish(&merged, Some(head), PASSPHRASE)
            .unwrap(),
        PublishOutcome::Committed {
            revision_id: merged.id()
        }
    );
    assert_eq!(
        coordinator
            .fetch_revision(losing, PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        loser_data.snapshot.as_slice()
    );
    assert_eq!(
        coordinator
            .fetch_revision(head, PASSPHRASE)
            .unwrap()
            .snapshot
            .as_slice(),
        winner_data.snapshot.as_slice()
    );
    assert_eq!(shared.revisions_len().unwrap(), 4);
}

#[test]
fn replay_parent_mismatch_and_unsupported_cas_never_overwrite() {
    let shared = MemoryStore::new();
    let coordinator = SyncCoordinator::new(shared.clone());
    let root = coordinator
        .prepare(&device("a"), None, b"original", PASSPHRASE)
        .unwrap();
    assert!(matches!(
        coordinator.publish(&root, Some(root.id()), PASSPHRASE),
        Err(SyncError::ParentMismatch)
    ));
    assert_eq!(shared.revisions_len().unwrap(), 0);
    coordinator.publish(&root, None, PASSPHRASE).unwrap();
    assert!(matches!(
        coordinator.publish(&root, None, PASSPHRASE),
        Err(SyncError::Replay)
    ));
    assert_eq!(shared.head().unwrap(), Some(root.id()));
    let unsupported = MemoryStore::without_cas();
    let sync = SyncCoordinator::new(unsupported.clone());
    let candidate = sync
        .prepare(&device("b"), None, b"never-sent", PASSPHRASE)
        .unwrap();
    assert!(matches!(
        sync.publish(&candidate, None, PASSPHRASE),
        Err(SyncError::ConditionalWriteUnsupported)
    ));
    assert_eq!(unsupported.revisions_len().unwrap(), 0);
    assert_eq!(unsupported.head().unwrap(), None);
}

#[derive(Clone)]
struct LyingStore {
    inner: MemoryStore,
}
impl RevisionStore for LyingStore {
    fn supports_verified_cas(&self) -> bool {
        true
    }
    fn head(&self) -> std::result::Result<Option<RevisionId>, StoreError> {
        self.inner.head()
    }
    fn get_revision(
        &self,
        id: RevisionId,
        limit: usize,
    ) -> std::result::Result<Option<Vec<u8>>, StoreError> {
        self.inner.get_revision(id, limit)
    }
    fn put_if_absent(
        &self,
        id: RevisionId,
        bytes: &[u8],
    ) -> std::result::Result<PutResult, StoreError> {
        self.inner.put_if_absent(id, bytes)
    }
    fn compare_and_swap_head(
        &self,
        _expected: Option<RevisionId>,
        _next: RevisionId,
    ) -> std::result::Result<CasResult, StoreError> {
        Ok(CasResult::Swapped) // fake CAS success without changing head
    }
}

#[test]
fn lying_provider_conditional_write_acknowledgment_fails_closed() {
    let memory = MemoryStore::new();
    let sync = SyncCoordinator::new(LyingStore {
        inner: memory.clone(),
    });
    let candidate = sync
        .prepare(&device("alice"), None, b"safe", PASSPHRASE)
        .unwrap();
    assert!(matches!(
        sync.publish(&candidate, None, PASSPHRASE),
        Err(SyncError::UnverifiedStorage)
    ));
    assert_eq!(memory.head().unwrap(), None);
    // Immutable candidate is preserved, but no false success is reported.
    assert_eq!(memory.revisions_len().unwrap(), 1);
}
