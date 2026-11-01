# Constellation two-database project archive (offline library)

`constellation-project-backup` creates **one passphrase-encrypted file**
containing exactly two separately consistent SQLite databases: Constellation's
primary studio database and its workflows database. The archive names and their
restore filenames are fixed: `studio.sqlite3` and `workflows.sqlite3`.
The caller supplies two existing source paths; they do not need those names.
No caller-controlled or archive-supplied path, filename, directory entry,
attachment or arbitrary extra file is decoded from the archive.

## Library API for native integration

```rust
use constellation_project_backup::ProjectBackupService;

fn save_project(
    studio_db: &std::path::Path,
    workflows_db: &std::path::Path,
    new_backup: &std::path::Path,
    new_restore_dir: &std::path::Path,
    user_passphrase: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let service = ProjectBackupService;
    // Source SQLite DBs may have concurrent writers or uncheckpointed WAL.
    let details = service.create_archive(studio_db, workflows_db, new_backup, user_passphrase)?;
    assert_eq!(details.version, 1);

    // Authenticate, validate both lengths/digests and run full SQLite
    // integrity_check on both images before creating any restore directory.
    let verified = service.verify_archive(new_backup, user_passphrase)?;
    assert_eq!(verified, details);
    let restored = service.restore_archive(new_backup, new_restore_dir, user_passphrase)?;
    assert_eq!(restored.studio, restored.directory.join("studio.sqlite3"));
    assert_eq!(restored.workflows, restored.directory.join("workflows.sqlite3"));
    Ok(())
}
```

- `ProjectBackupService::create_archive(studio, workflows, new_package, passphrase)`:
  create online SQLite snapshots, authenticate their lengths and SHA-256 hashes
  inside a versioned envelope and atomically publish a **new** encrypted file.
- `ProjectBackupService::verify_archive(package, passphrase)`:
  authenticate/decrypt and validate both in-memory database images without
  creating any destination. Returns `ArchiveInfo`.
- `ProjectBackupService::restore_archive(package, new_directory, passphrase)`:
  authenticate and validate everything first, then write the two fixed database
  filenames into a private staging directory beside the destination, sync and
  verify both files, and atomically publish the complete directory using an OS
  no-replace rename. Returns `RestoredPaths`.
- `create_archive_with_pair_validator(..., validator)`,
  `verify_archive_with_pair_validator(..., validator)`, and
  `restore_archive_with_pair_validator(..., validator)`: optionally enforce a
  caller-defined *application invariant across both captured images*. The
  validator receives **query-only SQLite connections to immutable snapshots**.
  A rejection occurs before encrypted archive creation or restore staging.
  Existing APIs retain their original independent-snapshot behavior.
- `create_archive_coordinated(studio, workflows, new_package, passphrase, timeout)`
  and `create_archive_coordinated_with_pair_validator(..., timeout, validator)`:
  **explicitly opt in** to a shared bounded writer-fence capture. The timeout
  is a nonzero `Duration` no longer than 30 seconds. The archive bytes remain
  version 1 and all existing default APIs behave as before.

### Optional bounded writer-fenced capture (thirteenth wave)

The service validates two existing regular files and a new output path, then
sorts the canonical source paths to acquire SQLite `BEGIN IMMEDIATE` write
reservations on **two separate read-write connections** in the same order
regardless of the caller's studio/workflow argument order. Both reservations
are held while source images are captured via the existing bounded online
SQLite Backup API from **different read-only connections**. This blocks
commits by ordinary SQLite writers to either WAL database across the complete
two-image capture. A single shared deadline covers acquiring both locks and
both snapshots. On contention or an expired deadline the operation drops the
connections (rolling back the uncommitted reservations) and **does not
publish a partial archive**. The locks are released before running a caller
validator or CPU-intensive Argon2/encryption.

One opt-in call, using the existing query-only generation validator shown
below, looks like:

```rust
use std::time::Duration;
use constellation_project_backup::ProjectBackupService;

// No source write is committed by the backup service. This holds SQLite
// writer reservations only during the two snapshots.
// service.create_archive_coordinated_with_pair_validator(
//     studio_db, workflows_db, new_backup, user_passphrase,
//     Duration::from_secs(10), &matching_generation,
// )?;
```

**Crucial limitation:** this is a two-file *writer-fenced capture*, not a
cross-database transaction. A crash or uncoordinated application action might
have already committed one DB's logical operation before both locks were
acquired. This can freeze an inconsistent pair. If writers need cross-file
atomicity, the host must use a real application-level transaction protocol,
e.g. consistently maintained generation markers, and an appropriate
`create_archive_coordinated_with_pair_validator` callback must reject
unmatched states. Processes that bypass SQLite to modify files, replace DB
paths or manipulate external state are **not** coordinated. A SQLite lock
wait is subject to its C-level busy-timeout granularity: the deadline is
checked between blocking steps, not a hard real-time preemption guarantee.
The host must tolerate `SnapshotTimeout` and retry later; it must not assume
that obtaining the fence means all application tasks reached a durable global
checkpoint. No native application writer integration is included in this
crate-only change.

Experimental caveat: a local Windows Python stdlib SQLite **3.39.4** WAL
fixture confirmed that `BEGIN IMMEDIATE` held by a separate connection on
each DB permits independent read-only online backup source connections,
blocks independently competing writer connections, and permits writes again
after rollback. Bundled `rusqlite`/SQLite implementation must be validated
separately by the Rust tests; the Python result is not a proof of every
platform or other SQLite version.

A sample host-owned validator can compare a durable generation marker, if both
databases maintain one as part of an application-level consistency protocol:

```rust
use constellation_project_backup::{ProjectBackupError, ProjectBackupService, Result};
use rusqlite::Connection;

fn matching_generation(studio: &Connection, workflows: &Connection) -> Result<()> {
    let first: i64 = studio.query_row(
        "SELECT generation FROM backup_generation", [], |row| row.get(0)
    )?;
    let second: i64 = workflows.query_row(
        "SELECT generation FROM backup_generation", [], |row| row.get(0)
    )?;
    if first != second { return Err(ProjectBackupError::PairMismatch); }
    Ok(())
}

// The host creates/maintains the marker schema and coordinates both writers.
// service.create_archive_with_pair_validator(studio, workflows, archive, passphrase, &matching_generation)?;
// service.restore_archive_with_pair_validator(archive, output_dir, passphrase, &matching_generation)?;
```

All operations are synchronous and should run in a background/native worker,
with explicit user-initiated filesystem paths and a passphrase obtained from a
secure UI. None are integrated with native Constellation code yet. Do not send
passphrases to an AI model or persist them in application logs.

## Archive layout and bounds

The plaintext passed to the existing `constellation-backup` codec is strictly
bounded to **64 MiB across both database images and all framing**, including
the 96-byte header. The codec supplies Argon2id, AES-256-GCM authenticated
encryption, random salt/nonce, a fixed authenticated header, and a separate
authenticated plaintext checksum. `BackupCodec::encrypt_to_new_file` uses an
encrypted same-directory temporary file, `sync_all`, and atomic
`persist_noclobber`. The application never writes plaintext staging files
during backup. The ciphertext has its own codec format version independent of
this project archive version.

| Plaintext offset | Size | Field |
|---|---:|---|
| 0 | 8 | `CSTLPRJ1` fixed magic |
| 8 | 2 | archive version = 1, little endian |
| 10 | 2 | fixed database count = 2 |
| 12 | 4 | reserved zeros |
| 16 | 8 | length of studio SQLite image (`u64`, LE) |
| 24 | 8 | length of workflows SQLite image (`u64`, LE) |
| 32 | 32 | SHA-256 of the studio image |
| 64 | 32 | SHA-256 of the workflows image |
| 96 | variable | studio SQLite image followed by workflows image, without padding |

The exact total length, immutable order, names, count, version, reserved bytes,
checksums, and full SQLite contents are authenticated by the outer codec.
Restore rejects inconsistent or overflowing lengths, wrong keys, altered
ciphertext, invalid SQLite headers/journal versions and any failed full
`PRAGMA integrity_check`. Each SQLite image must also be exactly its header's
declared page-count times page-size, rejecting appended or forged extra payloads
even when the archive's outer authentication and SHA-256 have been recomputed.
Neither archive nor filenames include user-controlled
relative paths. Legacy/unknown archive versions fail closed.

## SQLite consistency and restore safety

For each source DB, SQLite's **online Backup API** captures a consistent
committed view, including transactions not yet checkpointed out of a WAL file.
The code snapshots into **in-memory SQLite**, verifies page budgets during
bounded backup steps (30-second deadline), runs full `integrity_check`, and
serializes in memory. A WAL-mode header is normalized to a standalone
rollback-journal SQLite image after the snapshot. The DBs are independently
revalidated as SQLite images before encryption.

**Cross-database consistency limitation:** the default method takes two
sequential SQLite backups that are each individually transactionally
consistent, without fencing either writer. The new opt-in coordinated method
acquires both SQLite write reservations across the same two sequential
snapshots, but does not turn an already inconsistent pair into a transaction.
No single globally atomic snapshot exists across the two source DBs. If
studio and workflows records must agree at a logical application boundary,
the native host must coordinate *both* writers and in-flight application
operations, and maintain its own cross-database consistency invariant.
The optional paired-image validator can reject inconsistencies that the host
knows how to express as queries against the captured rows. Such a validator
does **not** itself lock both source databases, coordinate writers, or prove
global atomicity. A generation number only has meaning if **every relevant
writer** updates it under a correct application protocol. Validator callbacks
should be deterministic read-only checks and must fail closed on missing
metadata. For recovery, the host should use the checked verify **and** restore
methods with the same invariant; unaudited older archives can still be opened
via the unvalidated API. This crate currently has no native host integration.

Backup refuses symlink paths, path traversal (`..`) and nonregular sources, and
it requires distinct existing sources and a new output filename. Restore also
rejects symlink/reparse-point components and existing output directories,
including incomplete directories from previous crashes. Both database images
are written into a random private staging directory located in the same parent
as the user-selected destination. Files are created with `create_new`, synced,
and SQLite-checked before the staging directory is atomically renamed to the
destination using no-overwrite OS primitives (Linux `renameat2` with
`RENAME_NOREPLACE`, macOS `renamex_np` with `RENAME_EXCL`, and Windows
`MoveFileExW` without `REPLACE_EXISTING`). Unknown Unix platforms fail closed
until an equivalent primitive is implemented. A collision leaves the previously
existing destination untouched. The resulting output **is intentionally
unencrypted SQLite data**. On Unix staging permissions are 0700 and file
permissions 0600; Windows uses inherited ACLs. Therefore native integration
must require a trusted private destination parent; it must not expose plaintext
staging to untrusted accounts on Windows or permit another writer to alter the
parent while this operation runs. No plaintext is staged in the system temp
directory.

On ordinary restore failure the staging directory is cleaned up. A process
crash or power loss while writing can leave an orphan private staging directory
under the chosen parent, but it cannot expose a partially populated **final**
restore directory. After the atomic rename, a power failure may still compromise
durability because Windows std APIs do not offer portable directory fsync;
even on Unix a parent-directory fsync can fail after publication. Native code
must treat success as the point when the two restored DB files may be opened,
and handle any interrupted staging or post-publication failure conservatively.
It must perform its own safe activation/switch-over and project metadata
reconciliation. This library never replaces currently active DBs and does not
cover other project files, attachments, credentials, or synced revision history.

## Offline verification

From repository root in Windows PowerShell (build output on C:):

```powershell
$env:CARGO_TARGET_DIR = 'C:\Users\Public\constellation-project-backup-target'
cargo fmt --manifest-path crates/constellation-project-backup/Cargo.toml --check
cargo test --manifest-path crates/constellation-project-backup/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-project-backup/Cargo.toml --all-targets --offline -- -D warnings
```

The integration tests cover two SQLite databases with live, uncheckpointed
WAL transactions, both restored rows and integrity checks, actual one-file
encrypted output, wrong passwords, ciphertext tampering, authenticated bad
length/digest/count/version metadata, no-clobber behavior and rejection of an
interrupted directory. They also preserve orphan staging directories left by
simulated crashes while allowing a new atomic publish. Unix tests additionally
cover symlinked paths. Paired-image tests also cover mismatch rejection
before archive publication and restore, checked recovery of matching marker
images, query-only validator connections, and authenticated SQLite images
with extra trailing bytes or falsified page counts.
