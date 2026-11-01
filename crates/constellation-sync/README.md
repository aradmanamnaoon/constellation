# Constellation Sync — encrypted revision protocol and gated S3 HTTPS adapter

Standalone Rust primitives for user-controlled encrypted revision sync and a
strictly opt-in **S3-compatible HTTPS adapter**. The adapter is initially
disabled and does not issue any network request until the caller explicitly
invokes a service conditional-write verification probe. There is no configured
cloud service, observed live atomic-CAS proof, running server, daemon, billing,
tool execution, background scheduler, or native UI integration in this repo.

## What exists

`SyncCoordinator<S: RevisionStore>` creates encrypted revision envelopes,
authenticates revisions on retrieval, checks an externally trusted head
checkpoint, and conditionally advances a single remote head. The immutable
revision objects remain addressable even when a competing device wins.

- **Encryption:** reuses the existing `constellation-backup::BackupCodec`
  (Argon2id 64 MiB/3 passes, AES-256-GCM, fresh random salt and nonce, strict
  framing and authenticated header). The sync crate introduces no encryption
  algorithm or key-derivation implementation.
- **Revision envelope v1:** portable, versioned binary; public fields are a
  bounded device ID, optional 32-byte parent revision digest, and a SHA-256
  content ID over the public metadata **plus encrypted backup package**.
  Device attribution and parent are duplicated inside the authenticated
  ciphertext; a modified public header cannot be accepted merely by
  recomputing its unkeyed content ID.
- **Deterministic content address:** for identical serialized encrypted bytes
  and metadata, the revision ID is identical. Encrypting the same plaintext
  twice deliberately produces *different* IDs due to randomized encryption.
  Parent and device metadata remain visible to the storage operator; the
  snapshot is encrypted end to end.
- **Limits:** at most 1 MiB of snapshot bytes, 64 bytes for device ID, one
  32-byte parent digest and 1,048,974 bytes of envelope data. Invalid magic,
  protocol/KDF version, reserved flags, length, device/parent binding,
  ciphertext authentication, or content digest fails closed.
- **History:** each immutable revision has zero or one parent. A losing CAS
  candidate stays addressable as a branch; the application must explicitly
  recover, inspect and merge diverging snapshots. No silent last-writer-wins.

## Host-facing API

```rust
use constellation_sync::{
    DeviceId, MemoryStore, PublishOutcome, SyncCoordinator,
};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let storage = MemoryStore::new(); // offline test double
let sync = SyncCoordinator::new(storage.clone());
let user_passphrase = b"a high entropy user managed recovery passphrase";
let device = DeviceId::new("my-laptop")?;
let revision = sync.prepare(&device, None, b"encrypted research snapshot", user_passphrase)?;
let uploaded = sync.publish(&revision, None, user_passphrase)?;
assert_eq!(uploaded, PublishOutcome::Committed { revision_id: revision.id() });
let content = sync.fetch_revision(revision.id(), user_passphrase)?;
assert_eq!(content.snapshot.as_slice(), b"encrypted research snapshot");
let verified = sync.verified_head(Some(revision.id()), user_passphrase)?;
assert_eq!(verified.expect("known checkpoint").id, revision.id());
# Ok(())
# }
```

`RevisionEnvelope::to_bytes/from_bytes` use strict framed binary encoding.
`RevisionId` supports exact 64-character lowercase hexadecimal conversion,
`DeviceId::new` validates safe attribution identifiers, and
`VerifiedRevision.snapshot` is zeroized when dropped.

### Bounded recovery from a trusted checkpoint (tenth wave)

`DeviceRoster` is an explicitly populated, bounded (128 labels) set of locally
permitted device labels; `authorize` and `revoke` change host policy only.
With a **durably trusted checkpoint maintained outside the remote store**:

```rust
use constellation_sync::{DeviceId, DeviceRoster, SyncCoordinator, MAX_RECOVERY_DEPTH};

# fn example(sync: &SyncCoordinator<constellation_sync::MemoryStore>,
#            checkpoint: constellation_sync::RevisionId,
#            passphrase: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
let mut trusted = DeviceRoster::new(); // empty means deny all new revisions
trusted.authorize(DeviceId::new("my-laptop")?)?;
let evidence = sync.recover_after_checkpoint(checkpoint, &trusted, passphrase, 16)?;
// evidence.revisions_since_checkpoint lists the authenticated path in order.
// evidence.current.snapshot is the decrypted current head.
// Only after reviewing and durably applying it should the host persist the
// returned evidence.current.id as its *new* trusted checkpoint.
# let _ = (evidence, MAX_RECOVERY_DEPTH);
# Ok(())
# }
```

The checker follows encrypted, authenticated parent links from the current
head to the trusted checkpoint (maximum **64** new revisions), rejects a
missing/alternate/rolled-back chain, enforces local label permission for each
new revision, authenticates the checkpoint itself and rereads the head to detect
some concurrent changes. It does not auto-merge or update trusted local state.
It returns `RecoveryPath { current, revisions_since_checkpoint }`; oversized
or zero-depth requests fail. A changed head during traversal fails and requires
an explicit retry. If the remote head is exactly the trusted checkpoint, the
path is empty.

### Host-pinned, non-mutating restore preview (nineteenth wave)

`RestoreGuard::from_trusted_snapshot(checkpoint, local_bytes)` or
`RestoreGuard::from_pinned_digest(checkpoint, trusted_sha256)` binds a
host-maintained checkpoint to the last committed local snapshot. The new
`preview_restore_if_unchanged(guard, observed_local_bytes, roster, passphrase,
max_depth)` checks that local content still matches the guard **before**
performing remote reads. It then applies the existing bounded authenticated
ancestry, trusted-device and current-head stability verification. The result
is a `RecoveryPath` preview: it cannot overwrite a local file, advance a
checkpoint or merge divergent edits. Dirty local snapshots raise
`SyncError::LocalChanges`. Rolled-back remote heads and revoked device
labels continue to fail closed. The host must keep the digest/checkpoint in
independently protected local persistence and **recheck both under a local
write transaction** before applying a reviewed preview, to prevent changes
between preview and commit.

`publish_from_checkpoint(candidate, checkpoint, roster, passphrase)` checks
the local device label and requires the remote head to match the trusted
checkpoint **before** proceeding through the existing immutable-write and CAS
protocol. A competing writer may still win the CAS; the resulting encrypted
branch remains recoverable. When remote head differs from the local anchor,
the checked publish fails before writing a new branch. Bootstrap without a
trusted checkpoint still needs explicit host-side trust initialization.

**Revocation limitations:** Device IDs are authenticated strings, **not
device-specific digital signatures**. Anyone with the shared encryption key can
construct an envelope claiming another permitted label. These checks cannot
cryptographically prevent a previously authorized machine from uploading if
it retains S3 credentials or the shared key. Strong revocation requires rotating
credentials, removing remote write permissions, replacing shared keys and
eventually using cryptographic per-device identity with a trusted signed roster.
All applications and devices must choose the guarded APIs to enforce their
local policy. Historical revisions authored before revocation remain available
for independent recovery and audit; a revoked label after the trusted checkpoint
blocks checked recovery. The existing permissive APIs remain for compatibility.

`RemoteS3Store::revoke_session()` permanently disables that adapter instance,
including future contract probes, reads, and writes. It affects this process
only: it cannot cancel an HTTP request that started earlier, invalidate
provider-side access keys or revoke other clients' sessions.

The `RevisionStore` trait specifies these host-implemented methods:

| Method | Required semantics |
| --- | --- |
| `supports_verified_cas()` | Defaults to `false`; true only for adapters verified against genuine server-side CAS |
| `head()` | Return the current authoritative head digest |
| `get_revision(id, max_bytes)` | Fetch immutable content and enforce the byte limit **before allocation** |
| `put_if_absent(id, bytes)` | Atomic create-if-absent; never replace existing bytes |
| `compare_and_swap_head(expected, next)` | Atomic conditional head write; return `Swapped` or the actual conflicting head |

`SyncCoordinator::publish(envelope, expected_parent, passphrase)` first rejects
unsupported CAS, mismatched parent, and unauthenticated envelope. It verifies
the parent object (if any), attempts immutable create, then CASes the head.
Following a reported success it rereads the head **and full immutable object**
before returning `Committed`. Following a conflict it validates the reported
head and authenticates its revision before returning
`Conflict { current_head, branch_id }`. Repeated publication of the same
revision is rejected as `Replay`.

`MemoryStore` is a bounded, shared, thread-safe in-memory fake that implements
strict atomic semantics and can be cloned to simulate independent devices.
`MemoryStore::without_cas()` models a provider that does not support
conditional writes: publication fails without storing any revision.
It is **not durable or a remote storage provider**.

`SqliteStore::open(absolute_local_path)` is a **durable local-only storage
adapter**. It creates or reopens a versioned SQLite WAL/FULL database under an
existing local directory, retaining at most 1,024 immutable encrypted
envelopes. The head is advanced under a `BEGIN IMMEDIATE` transaction; two
processes using the same local database cannot both win a stale head CAS.
Every read checks limits before allocating and validates the envelope digest.
The database stores ciphertext, head IDs and public device/parent metadata,
not plaintext or passphrases. A local file database is **not** cross-device
cloud sync or a verified remote conditional-write implementation; do not
expose it as a shared-network-folder sync service without separate locking,
filesystem reliability and threat-model verification.

### Opt-in S3 HTTPS adapter (new; unverified against real service)

`S3Endpoint::new(base, pinned_public_ip, bucket, prefix, region)` accepts an
explicit public-domain **HTTPS on port 443** URL with no path, query, fragment,
credentials, encoded redirects or ambiguous host, plus a separately supplied
**public fixed IP address**. The host must independently determine and verify
the provider's intended DNS address before pinning it. The HTTP client never
performs DNS resolution, uses the original hostname for TLS certificate and SNI
verification, forbids ambient proxies and redirects, and applies connect and
request deadlines. Private, loopback, link-local, reserved/test-network,
IPv4-mapped and non-global IPv6 destinations are rejected before any request.

`S3Credentials::new(access_key, secret_key, session_token)` holds session-only
AWS Signature V4 credentials in zeroizing memory. Debug formatting redacts
credentials and endpoint information; no request URI, response body, secret or
authorization token is logged by this crate. The integration host is
responsible for supplying credentials securely, limiting IAM permissions to
the selected bucket/prefix, and clearing its own copies.

`HttpsS3Transport::new(endpoint, credentials)` builds the bounded transport.
`RemoteS3Store::new_disabled(transport)` starts with
`supports_verified_cas() == false`, and all `RevisionStore` operations fail
with `UnsupportedCas` until `verify_service_contract()` passes. **This
explicit probe makes real conditional PUTs against the selected service** and
leaves its small, uniquely named immutable probe objects in the selected
prefix. It verifies create-if-absent (`If-None-Match: *`) does not overwrite,
GET preserves the original bytes, a head is created conditionally, two
concurrent `If-Match: <strong ETag>` writes against the same ETag yield exactly
one winner, and a stale ETag cannot overwrite the winner. An unsupported,
misbehaving, unreachable or malformed service stays disabled. A failing probe
may leave objects and may incur provider charges; never automatically retry it.

Service-contract probes are now serialized on the same store instance to
avoid conflicting enable/disable decisions. After a newly acknowledged
create-only immutable revision PUT, the adapter immediately performs a fresh
bounded GET and compares the **entire encrypted envelope** to what was sent;
a false success or content substitution is rejected before the head CAS.
This adds a detection barrier; it does not guarantee immutable behavior if
an untrusted service changes objects later. The coordinator still authenticates
envelope contents, verifies the pinned parent and rereads a committed object.

Actual local TCP HTTP/1.1 S3 conditional-object fixture tests now exercise
create-only semantics, ETag `If-Match` races with two concurrent writers,
encrypted competing branches, failed conditional contract probes, false
immutable PUT acknowledgments, and a committed head write whose response
never reached the client. These tests exercise **locally simulated S3
conditional HTTP behavior**, with an intentionally test-only plaintext
loopback client/server. They do **not** exercise the production HTTPS SigV4
transport, certify provider semantics or establish cloud credentials.

For a host that has already obtained explicit user consent, scoped session
credentials and a separately confirmed public destination IP:

```rust
use constellation_sync::{HttpsS3Transport, RemoteS3Store, RevisionStore, S3Credentials, S3Endpoint};
use std::net::IpAddr;

fn prepare_remote(
    pinned_ip: IpAddr, access_key: String, secret_key: String,
) -> Result<RemoteS3Store<HttpsS3Transport>, Box<dyn std::error::Error>> {
    let endpoint = S3Endpoint::new(
        "https://s3.your-owned-domain.example", pinned_ip,
        "my-private-bucket", "constellation", "us-east-1",
    )?;
    let transport = HttpsS3Transport::new(endpoint, S3Credentials::new(
        access_key, secret_key, None,
    )?)?;
    let store = RemoteS3Store::new_disabled(transport);
    assert!(!store.supports_verified_cas());
    // Explicit user-triggered action: writes isolated proof objects remotely.
    store.verify_service_contract()?;
    Ok(store)
}
```

The adapter stores encrypted revisions as `revisions/<sha256>` objects using
atomic `PUT If-None-Match: *`, while the current head is `head`, updated by
`PUT If-Match: <observed ETag>` or create-only for the first revision. It
never uses read-then-unconditional-PUT. A stale writer receives a conflict
with the observed current revision, while its immutable branch persists.
Unknown/ambiguous network failures are errors; the adapter does **not** retry
writes, infer success or overwrite data. HTTP responses are streamed with
maximum allocation bounds; unexpected statuses, redirects, missing/malformed
ETags, bad digest, oversized bodies and dishonest acks fail closed. The remote
transport uses path-style S3 and requires the endpoint to implement **atomic
conditional PutObject for both If-None-Match and If-Match**. Some S3-compatible
providers reject, emulate or omit one of these features: they cannot be used.

The local fixture tests do **not** establish a remote service guarantee.
Passing a one-time live probe is a bounded observation and cannot protect
against a malicious service that later violates its contract. Production
enablement also requires provider-specific documented conditional-PUT
semantics, repeatable service tests including two independent physical devices,
a persisted trusted checkpoint outside the cloud service, user-consented
credential/account setup and native integration. **No Liara, Arvan or other
S3-compatible provider has passed such a real test here.** No live remote sync
is claimed.

## Concurrent revision example

Two devices read the same trusted head `H` and create encrypted candidates
`A(parent=H)` and `B(parent=H)`. Both candidates can be uploaded immutably.
The store's atomic CAS permits exactly one head update (say `A`).
The other receives `Conflict { current_head: A, branch_id: B }`. Both
`fetch_revision(A, ...)` and `fetch_revision(B, ...)` work. The host must
collect an explicit resolution, merge content, and prepare a *new encrypted
revision* with `parent=A` before attempting another CAS.

The host must keep an **independent trusted checkpoint** of the last accepted
head revision ID. `verified_head(checkpoint, passphrase)` rejects any different
head, including an old authentic revision returned by a malicious server.
This detects rollback relative to the supplied anchor; a first device with
no trusted checkpoint cannot know the true latest head. No automatic initial
trust decision is made by this crate.

## What is missing for a verified live provider

A real user-owned storage adapter must supply conditional writes that are
*actually atomic at the storage service*, such as a proven create-only object
operation and an authoritative head-update precondition. A separate HEAD/GET
followed by unconditional PUT is **not CAS** and must fail closed. The
`supports_verified_cas` flag is an adapter contract, not cryptographic proof
that a malicious remote provider will honor it; returned readbacks detect
some broken implementations but cannot provide linearizability on their own.
The opt-in S3 HTTPS adapter and deterministic local fixture are implemented,
but no provider-specific service contract, authorized cloud credentials,
real endpoint verification, trusted remote configuration or two-device transfer
have been observed. `verify_service_contract` has never been invoked against
a real service as part of this repository's acceptance tests.

The native application still needs credential/keyring handling, safe
passphrase unlock, pairing/trusted-checkpoint storage, device enrollment and
revocation, encrypted consistent project snapshot collection, filesystem or
network adapters, interrupted upload reconciliation, retry limits,
confirmation UI and meaningful conflict resolution/merge policies. Durable
recording of the accepted checkpoint must be coordinated with project-state
commit. As with any single remote head, offline branches cannot be merged
without a trusted decision about which parent and combined content to use.
No plaintext, passphrase or provider secret is intentionally logged.

## Offline verification

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp\constellation-cargo-target'
$env:CARGO_INCREMENTAL = '0'
cargo fmt --manifest-path crates/constellation-sync/Cargo.toml --check
cargo test --manifest-path crates/constellation-sync/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-sync/Cargo.toml --all-targets --offline -- -D warnings
```

The regression tests exercise encrypted roundtrips, random ciphertext
addresses, malicious outer/inner schema, recomputed content IDs with altered
metadata, bounds and truncated frames, replay, wrong parent, rollback,
unsupported/lying CAS stores, and real racing writes from two local threads
with both branches recoverable. SQLite integration tests additionally exercise
fresh-process style reopening, separate SQLite connections racing a head CAS,
both encrypted branches surviving restart, bounded reads, corruption rejection,
unknown schema refusal and conditional updates.
The S3 adapter additionally has deterministic negative fixtures for omitted
conditional enforcement, redirects, malformed/oversized HTTP responses,
corrupted GETs, reused strong ETags, real two-writer thread contention,
ambiguous after-write network failure, replay, rollback, private/reserved IPs,
unsafe URL encodings and SigV4 conditional-header signing. These fixtures
perform **zero HTTP requests**, use no cloud credentials and cannot prove
actual provider behavior.
New negative fixtures also exercise trusted-ancestor recovery after a lost
remote CAS acknowledgement, process-local S3 session revocation, local rollback
and alternate-root attacks, capped chain traversal, and a restarted SQLite
two-writer conflict followed by local roster revocation. The host has not yet
implemented persistent device policy or trusted-checkpoint transactions.
