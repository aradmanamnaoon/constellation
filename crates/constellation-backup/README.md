# Constellation Backup

Portable, password-protected encrypted backup packages implemented as a standalone Rust library. This crate encrypts and decrypts **opaque bytes**. It is not connected to a database, the desktop GUI, a scheduler, a keyring, or a synchronization service.

## Security and package format

Version 1 uses **Argon2id** (RFC 9106-family algorithm, version 0x13, 64 MiB memory, 3 passes, 1 lane) to derive a 32-byte key from a caller-provided passphrase and a fresh random 16-byte salt. It then encrypts with **AES-256-GCM** using a fresh random 12-byte nonce. The complete fixed header is supplied to AES-GCM as authenticated associated data (AAD).

The format is binary, little-endian, architecture independent, and contains no serialized Rust types or OS paths.

| Offset (bytes) | Length | Encoding |
| --- | ---: | --- |
| 0 | 8 | ASCII magic `CSTLBKP1` |
| 8 | 2 | Unsigned format version (currently 1) |
| 10 | 2 | Reserved zero |
| 12 | 4 | Argon2 memory cost in KiB, 65536 |
| 16 | 4 | Argon2 iteration count, 3 |
| 20 | 4 | Argon2 lanes, 1 |
| 24 | 8 | Unsigned plaintext byte count |
| 32 | 16 | Random salt |
| 48 | 12 | Random AES-GCM nonce |
| 60 | variable | Authenticated encrypted body, followed by 16-byte GCM tag |
| Final 32 | 32 | SHA-256 checksum of header and ciphertext including GCM tag |

The **encrypted body** consists of the 32-byte SHA-256 digest of the plaintext followed by the plaintext itself. This inner digest is confidential and authenticated. The outer SHA-256 checksum provides quick corruption detection but is **not an authenticity mechanism**: AES-GCM authentication rejects modifications even when an attacker recomputes the outer checksum.

Length is checked before KDF invocation, data allocation, or package processing. The untrusted header's Argon2 parameters must match exactly; a manipulated memory cost cannot trigger arbitrary Argon2 allocations. Maximum plaintext size defaults to **16 MiB**, and callers may configure a lower limit or increase up to a hard cap of **64 MiB**. The fixed envelope overhead is **140 bytes**. Inputs are checked for truncated and inconsistent framing and unsupported versions.

## API usage

```rust,no_run
use constellation_backup::{BackupCodec, BackupLimits};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backup = BackupCodec::new(BackupLimits::new(8 * 1024 * 1024)?);
    let passphrase = b"long unique user passphrase";
    let plaintext = b"serialized, consistent application snapshot";

    let package = backup.encrypt(plaintext, passphrase)?;
    let restored = backup.decrypt(&package, passphrase)?;
    assert_eq!(restored.as_slice(), plaintext);

    backup.encrypt_to_new_file("my-backup.cbackup", plaintext, passphrase)?;
    let restored_from_disk = backup.decrypt_from_file("my-backup.cbackup", passphrase)?;
    assert_eq!(restored_from_disk.as_slice(), plaintext);
    Ok(())
}
```

Use the functions as follows:

- `encrypt(&[u8], &[u8]) -> Result<Vec<u8>>`: memory-to-memory encryption with randomized envelope.
- `decrypt(&[u8], &[u8]) -> Result<Zeroizing<Vec<u8>>>`: verifies framing, checksum and AES-GCM authentication, returning a buffer that zeroes its memory on drop.
- `inspect(&[u8]) -> Result<PackageMetadata>`: validates format and checksum without a passphrase; **not** an authenticity check.
- `encrypt_to_new_file(path, plaintext, passphrase)`: creates a new encrypted backup exclusively; existing files, directories and even dangling symlinks cannot be overwritten. Same-directory staging is synced before the atomic no-clobber publication. **Prefer this method for backup revisions.**
- `encrypt_to_file(path, plaintext, passphrase)`: explicitly replaces an existing **regular** package (or creates it if absent) after same-directory staging and syncing. Symlinks, including dangling symlinks, and directories are refused. Existing ciphertext survives encryption/staging failures. On Windows a file open elsewhere may prevent replacement.
- `decrypt_from_file(path, passphrase)`: rejects symlink and non-file inputs, bounds input by file metadata and actual bytes, then decrypts.

Passphrases are caller-owned byte slices, accepted at lengths 8–1024 bytes. A short passphrase meeting the minimum is **not necessarily secure**; the host should guide users to high-entropy unique passphrases. The input plaintext belongs to the caller, so the host should zeroize its source buffers after use. Derived keys and intermediate plaintext are automatically zeroized by this library; the decrypted result is wrapped in `Zeroizing<Vec<u8>>`. The file API does not write decrypted data to disk.

## Portability and operational limits

- Version 1 never silently changes cryptographic parameters. An incompatible format version is rejected. To rotate the passphrase, decrypt in memory and create a fresh package with another salt and nonce.
- A correct passphrase is necessary for recovery. There is no recovery key, password escrow or keyring integration here.
- This library only encrypts a provided byte array. The integrating application must first acquire a consistent and complete application snapshot, then define how the restored snapshot is validated and applied transactionally. **No database backup/restore integration or E2EE sync exists in this crate.**
- AES-GCM provides integrity/confidentiality for the package. It does not guarantee that a complete package is the *latest* revision or prevent rollback to an authentic older package. External metadata/signing or an authenticated revision ledger is necessary for anti-rollback.
- Staging and final publication use the same canonicalized parent directory. On Unix, sync of that directory improves crash persistence. On Windows, directory sync is not provided by portable Rust `std`; crash persistence depends on filesystem and OS behavior. Errors after publication (for example, directory sync errors) may mean the package is already visible.
- The library has no filesystem sandbox. The caller selects the destination; paths and parent directories must be authorized by the host. Final-component symlinks are rejected, but a malicious local process capable of changing parent directories or swapping file entries concurrently can still exploit a time-of-check/time-of-use race. The host should select an access-controlled parent directory and never assume these file APIs enforce an OS sandbox.
- No package data or passphrase is intentionally logged. Debuggers, swap, crash dumps, caller-owned buffers, and third-party dependencies may still expose sensitive material under a compromised machine.

## Validation

```sh
cargo fmt --manifest-path crates/constellation-backup/Cargo.toml -- --check
cargo test --manifest-path crates/constellation-backup/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-backup/Cargo.toml --all-targets --offline -- -D warnings
```

The regression suite checks binary and empty roundtrips, randomization, wrong passphrase, tamper with both untouched and recomputed checksum, authenticated salt/nonce tamper, incompatible version/KDF, every truncated prefix, inconsistent and oversized frames, encrypted-file persistence, exclusive creation, prevention of clobber, symlinks, unicode paths, and rejection of invalid destinations.
