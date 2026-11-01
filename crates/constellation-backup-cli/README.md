# Constellation Offline Backup CLI

A **working standalone command-line tool** for encrypted backups of **SQLite databases**, built around the `constellation-backup` portable Argon2id + AES-256-GCM codec. It runs on the user's machine without an AI service, network request or cloud integration.

**This crate is not yet integrated with all of Constellation's project management, scheduled tasks, GUI, cloud sync or E2EE collaboration.** It backs up one SQLite database's `main` schema, not application sidecar files, attachments, separately attached databases, credential stores or other project resources.

## Run

From the repository root:

```sh
cargo run --offline --manifest-path crates/constellation-backup-cli/Cargo.toml -- --help
cargo run --offline --manifest-path crates/constellation-backup-cli/Cargo.toml -- backup ./studio.sqlite3 ./studio-2026-10-30.cbackup
cargo run --offline --manifest-path crates/constellation-backup-cli/Cargo.toml -- restore ./studio-2026-10-30.cbackup ./studio-restored.sqlite3
```

If your SQLite image exceeds the default **16 MiB**, pass `--max-mib 64` after the output path (absolute cap: 64 MiB, covering the database file, not just user rows). The output parent directory must exist. **The output filename must not exist, including a dangling symlink; this tool never overwrites existing backup packages or databases.** Source symlinks are rejected. For SQLite restore, pre-existing `-wal`, `-shm` or `-journal` files next to the requested output are also rejected: they could corrupt or change a newly restored database. A failed restore or wrong passphrase must not create the requested database.

Both operations prompt for a passphrase using **no-echo interactive console input**. Backup asks for confirmation. There are **no** password flags, password environment-variable support or redirected-stdin input. Run this tool in a genuine interactive terminal. It fails closed if standard input, output or error is redirected. Never paste your passphrase into a shell command, log, screenshot or chat.

The executable does not require a runtime network connection: compile/install it once using Cargo, then run the binary directly. Use `cargo run --offline` after the dependencies are installed. Initial dependency retrieval/build may require internet access; offline execution does not.

## How a consistent SQLite snapshot is produced

An ordinary file copy of an active SQLite `.sqlite3` misses committed changes still in its `-wal` file. Instead:

1. Open the existing source SQLite database **read-only** and validate the file-size budget.
2. Use SQLite's **online Backup API** (`sqlite3_backup_*` via `rusqlite`) to copy a consistent snapshot, including committed WAL transactions, into an in-memory database. Page counts and a 30-second operation deadline are enforced.
3. Run full `PRAGMA integrity_check` on the snapshot, then serialize the SQLite image in memory. Convert WAL-mode header read/write bytes to standalone rollback-journal bytes, if necessary; the snapshot already contains committed WAL contents.
4. Encrypt the serialized bytes using `constellation-backup` and stage **only ciphertext** in a randomly named file in the canonicalized destination directory. Flush and sync it; publish an exclusively new file using `persist_noclobber`.

Restoring authenticates and decrypts the package into a zeroizing in-memory buffer, checks the SQLite header, deserializes into an in-memory read-only SQLite database and runs `PRAGMA integrity_check`. Only then does the tool use SQLite's Backup API to create a database in a private temporary file in the output directory. The staging file is synced, SQLite handles are closed, destination sidecars are checked again, then `persist_noclobber` publishes the new database.

The encrypted package format, Argon2id parameters, authenticated header, salt, nonce, checksum and maximum sizes are documented in `../constellation-backup/README.md`.

## Operational and security boundaries

- Restore creates an **ordinary unencrypted SQLite database**. It is suitable for an explicitly requested restore but should be written only to a trusted filesystem.
- Restore stages a **temporary unencrypted SQLite file** in the output directory. It is private and normally removed after failures. OS crashes, snapshots, swap, filesystem backups, forensic recovery and SQLite temporary artifacts can nevertheless preserve plaintext. Choose an appropriate destination for sensitive data.
- Backup never intentionally writes plaintext to disk: SQLite's online backup is in memory and only the encrypted blob is staged. SQLite/OS allocators may retain copies outside Rust's zeroizing buffers. The source database itself remains unencrypted.
- The codec bounds plaintext to at most 64 MiB, with SQLite page-count checks before and during online backup. Larger databases need streaming encryption and an explicitly versioned format, rather than bypassing limits.
- **SQLite consistency is separate from application consistency.** An online SQLite backup is transactionally valid but does not guarantee that attachments, cache directories, other databases or in-flight application operations represent the same checkpoint. Desktop code must coordinate those resources.
- An authentic older package can be restored: **there is no anti-rollback protection**. There is no password recovery, key escrow, scheduling, cloud sync or E2EE collaboration here.
- No library can protect a compromised OS. The CLI never accepts secrets from arguments or environment variables; privileged processes may still inspect its memory.
- On Unix, the output parent directory is synced after publication. Portable Rust lacks equivalent Windows directory syncing, so power-loss durability depends on OS/filesystem behavior. An error after publication may mean a new file is already visible.
- Final source symlinks and output filename symlinks are rejected. Output staging/publication both use the canonical parent. Symlinked *parent directories* are resolved. A malicious process able to rewrite directory entries concurrently could still race source opening or SQLite sidecar checks. Use an access-controlled destination directory.
- Database locks, concurrent corruption or a busy database may cause an operation to fail. Existing database/package files must be preserved, without a published partial new file.

## Desktop integration contract

Rust desktop code may construct `BackupService::new(BackupLimits::default())` and call `backup_database(source_sqlite_path, new_encrypted_package_path, passphrase_bytes)` or `restore_database(encrypted_package_path, new_sqlite_path, passphrase_bytes)`. Paths and secrets must come from explicit user choices. Both destinations must be **new filenames**. `CliError` distinguishes conflicts, integrity failures, malformed images and nested `Package(BackupError::AuthenticationFailed)` for an incorrect password or modified ciphertext. This synchronous API should execute off the UI thread.

Desktop integration still needs to coordinate a project checkpoint, validate project schema/migration compatibility, handle attachments, apply a verified restore through an explicit user-approved switch, and design an interruption-safe workflow. Scheduling, progress reporting, cancellation during Argon2, cloud sync, anti-rollback revision history and end-to-end project restore are outside this crate.

## Verification

```sh
cargo fmt --manifest-path crates/constellation-backup-cli/Cargo.toml --check
cargo test --manifest-path crates/constellation-backup-cli/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-backup-cli/Cargo.toml --all-targets --offline -- -D warnings
```

Tests exercise real SQLite databases with an active WAL writer, ensure uncheckpointed rows appear in a restored image with `integrity_check=ok`, reject authenticated tampering, wrong passwords, bad images, oversized databases, existing outputs, symlink sources/targets and stale SQLite sidecars, and verify that console password flags or redirected input are rejected.
