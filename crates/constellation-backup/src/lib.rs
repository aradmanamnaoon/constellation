//! Portable, passphrase-encrypted local backup packages.
//!
//! The package is an opaque byte string; database snapshot consistency, sync,
//! and ownership of the source plaintext belong to the integrating host.
//! No secrets are written to disk by this crate.

use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use aes_gcm::{
    aead::{rand_core::RngCore, Aead, OsRng, Payload},
    Aes256Gcm, KeyInit,
};
use argon2::{Algorithm, Argon2, Params, Version};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

/// Unique marker, immutable for the lifetime of the version-one format.
pub const MAGIC: [u8; 8] = *b"CSTLBKP1";
pub const FORMAT_VERSION: u16 = 1;
/// Absolute cap for any package processed by this library.
pub const HARD_MAX_PLAINTEXT: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_PLAINTEXT: usize = 16 * 1024 * 1024;
pub const ARGON2_MEMORY_KIB: u32 = 65_536;
pub const ARGON2_PASSES: u32 = 3;
pub const ARGON2_LANES: u32 = 1;

const HEADER_LEN: usize = 60;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const INNER_CHECKSUM_LEN: usize = 32;
const GCM_TAG_LEN: usize = 16;
const OUTER_CHECKSUM_LEN: usize = 32;
const ENVELOPE_OVERHEAD: usize = HEADER_LEN + INNER_CHECKSUM_LEN + GCM_TAG_LEN + OUTER_CHECKSUM_LEN;

/// Caller-selected limit, which cannot exceed the hard 64 MiB cap.
#[derive(Clone, Copy, Debug)]
pub struct BackupLimits {
    max_plaintext: usize,
}

impl Default for BackupLimits {
    fn default() -> Self {
        Self {
            max_plaintext: DEFAULT_MAX_PLAINTEXT,
        }
    }
}

impl BackupLimits {
    pub fn new(max_plaintext: usize) -> Result<Self> {
        if max_plaintext == 0 || max_plaintext > HARD_MAX_PLAINTEXT {
            return Err(BackupError::SizeLimit);
        }
        Ok(Self { max_plaintext })
    }

    pub fn max_plaintext(&self) -> usize {
        self.max_plaintext
    }

    pub fn max_package(&self) -> usize {
        self.max_plaintext + ENVELOPE_OVERHEAD
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("passphrase must contain between 8 and 1024 bytes")]
    InvalidPassphrase,
    #[error("backup exceeds the configured size limit")]
    SizeLimit,
    #[error("backup package is truncated or its declared length is inconsistent")]
    InvalidLength,
    #[error("backup package magic is not recognized")]
    InvalidMagic,
    #[error("backup package version is not supported")]
    UnsupportedVersion,
    #[error("backup package KDF settings or reserved fields are unsupported")]
    UnsupportedParameters,
    #[error("backup package checksum does not match")]
    ChecksumMismatch,
    #[error("destination already exists; refusing to overwrite")]
    OutputExists,
    #[error("file path is not a regular file or has an invalid filename")]
    InvalidFilePath,
    #[error("decryption failed: incorrect passphrase or altered ciphertext")]
    AuthenticationFailed,
    #[error("cryptographic operation failed")]
    CryptoFailure,
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, BackupError>;

/// Validated unencrypted metadata. It does not certify authenticity;
/// authenticity is established only by `decrypt`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageMetadata {
    pub version: u16,
    pub plaintext_len: usize,
    pub package_len: usize,
}

/// Holds no persistent secrets or mutable global cryptographic state.
#[derive(Clone, Debug, Default)]
pub struct BackupCodec {
    limits: BackupLimits,
}

impl BackupCodec {
    pub fn new(limits: BackupLimits) -> Self {
        Self { limits }
    }

    pub fn limits(&self) -> BackupLimits {
        self.limits
    }

    /// Encrypt an opaque byte payload. The caller still owns and must manage
    /// the original input buffer; intermediate plaintext and key are zeroized.
    pub fn encrypt(&self, plaintext: &[u8], passphrase: &[u8]) -> Result<Vec<u8>> {
        validate_passphrase(passphrase)?;
        self.ensure_plaintext_size(plaintext.len())?;

        let mut salt = [0_u8; SALT_LEN];
        let mut nonce = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut nonce);

        let header = Header {
            plaintext_len: plaintext.len(),
            salt,
            nonce,
        }
        .encode();
        let key = derive_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&*key).map_err(|_| BackupError::CryptoFailure)?;

        let mut inner = Zeroizing::new(Vec::with_capacity(INNER_CHECKSUM_LEN + plaintext.len()));
        inner.extend_from_slice(&Sha256::digest(plaintext));
        inner.extend_from_slice(plaintext);
        let ciphertext = cipher
            .encrypt(
                (&nonce).into(),
                Payload {
                    msg: &inner,
                    aad: &header,
                },
            )
            .map_err(|_| BackupError::CryptoFailure)?;

        let mut package = Vec::with_capacity(plaintext.len() + ENVELOPE_OVERHEAD);
        package.extend_from_slice(&header);
        package.extend_from_slice(&ciphertext);
        let checksum = Sha256::digest(&package);
        package.extend_from_slice(&checksum);
        debug_assert_eq!(package.len(), plaintext.len() + ENVELOPE_OVERHEAD);
        Ok(package)
    }

    /// Decrypted bytes are wrapped in a zeroizing buffer. Do not call
    /// `into_inner` or copy plaintext into ordinary long-lived buffers.
    pub fn decrypt(&self, package: &[u8], passphrase: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        validate_passphrase(passphrase)?;
        let (header, ciphertext) = self.parse(package)?;
        let key = derive_key(passphrase, &header.salt)?;
        let cipher = Aes256Gcm::new_from_slice(&*key).map_err(|_| BackupError::CryptoFailure)?;
        let mut inner = Zeroizing::new(
            cipher
                .decrypt(
                    (&header.nonce).into(),
                    Payload {
                        msg: ciphertext,
                        aad: &package[..HEADER_LEN],
                    },
                )
                .map_err(|_| BackupError::AuthenticationFailed)?,
        );
        if inner.len() != header.plaintext_len + INNER_CHECKSUM_LEN {
            return Err(BackupError::AuthenticationFailed);
        }
        let plaintext = Zeroizing::new(inner.split_off(INNER_CHECKSUM_LEN));
        if Sha256::digest(&*plaintext).as_slice() != &inner[..] {
            return Err(BackupError::AuthenticationFailed);
        }
        Ok(plaintext)
    }

    /// Inspects framing and checks the unkeyed corruption checksum without
    /// executing the expensive KDF. For authenticated data, call `decrypt`.
    pub fn inspect(&self, package: &[u8]) -> Result<PackageMetadata> {
        let (header, _) = self.parse(package)?;
        Ok(PackageMetadata {
            version: FORMAT_VERSION,
            plaintext_len: header.plaintext_len,
            package_len: package.len(),
        })
    }

    /// Atomically replace a regular encrypted package using a same-directory
    /// staging file. This method intentionally allows replacing existing
    /// regular files. Prefer `encrypt_to_new_file` for new backup revisions.
    /// Symlink and non-file destinations are refused.
    pub fn encrypt_to_file(
        &self,
        target: impl AsRef<Path>,
        plaintext: &[u8],
        passphrase: &[u8],
    ) -> Result<PackageMetadata> {
        self.save_encrypted_file(target.as_ref(), plaintext, passphrase, true)
    }

    /// Create a new encrypted package without ever overwriting an existing
    /// destination (including a dangling symlink). The publication step uses
    /// an atomic filesystem no-clobber operation.
    pub fn encrypt_to_new_file(
        &self,
        target: impl AsRef<Path>,
        plaintext: &[u8],
        passphrase: &[u8],
    ) -> Result<PackageMetadata> {
        self.save_encrypted_file(target.as_ref(), plaintext, passphrase, false)
    }

    fn save_encrypted_file(
        &self,
        target: &Path,
        plaintext: &[u8],
        passphrase: &[u8],
        replace_existing: bool,
    ) -> Result<PackageMetadata> {
        let package = self.encrypt(plaintext, passphrase)?;
        let parent = target
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !parent.is_dir() {
            return Err(BackupError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "destination directory does not exist",
            )));
        }
        // Both staging and publication resolve to the same canonical directory.
        let parent = fs::canonicalize(parent)?;
        let filename = target.file_name().ok_or(BackupError::InvalidFilePath)?;
        let target = parent.join(filename);
        match fs::symlink_metadata(&target) {
            Ok(_) if !replace_existing => return Err(BackupError::OutputExists),
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(BackupError::InvalidFilePath);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(BackupError::Io(error)),
        }
        let mut staged = NamedTempFile::new_in(&parent)?;
        staged.write_all(&package)?;
        staged.as_file().sync_all()?;
        if replace_existing {
            staged
                .persist(&target)
                .map_err(|error| BackupError::Io(error.error))?;
        } else {
            staged.persist_noclobber(&target).map_err(|error| {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                    BackupError::OutputExists
                } else {
                    BackupError::Io(error.error)
                }
            })?;
        }
        sync_parent(&parent)?;
        Ok(PackageMetadata {
            version: FORMAT_VERSION,
            plaintext_len: plaintext.len(),
            package_len: package.len(),
        })
    }

    /// Reads no more than the configured maximum package size, checking both
    /// file metadata and actual bytes to defend against a growing file.
    pub fn decrypt_from_file(
        &self,
        source: impl AsRef<Path>,
        passphrase: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        // Do not silently follow a symlink to a package somewhere else.
        // Portable std cannot make this check and the open atomic against a
        // malicious process that can concurrently rewrite the directory.
        let source = source.as_ref();
        if !fs::symlink_metadata(source)?.file_type().is_file() {
            return Err(BackupError::InvalidFilePath);
        }
        let file = File::open(source)?;
        if !file.metadata()?.is_file() {
            return Err(BackupError::InvalidFilePath);
        }
        if file.metadata()?.len() > self.limits.max_package() as u64 {
            return Err(BackupError::SizeLimit);
        }
        let mut package = Vec::new();
        file.take(self.limits.max_package() as u64 + 1)
            .read_to_end(&mut package)?;
        if package.len() > self.limits.max_package() {
            return Err(BackupError::SizeLimit);
        }
        self.decrypt(&package, passphrase)
    }

    fn ensure_plaintext_size(&self, len: usize) -> Result<()> {
        if len > self.limits.max_plaintext {
            Err(BackupError::SizeLimit)
        } else {
            Ok(())
        }
    }

    fn parse<'a>(&self, package: &'a [u8]) -> Result<(Header, &'a [u8])> {
        if package.len() > self.limits.max_package() {
            return Err(BackupError::SizeLimit);
        }
        if package.len() < HEADER_LEN + INNER_CHECKSUM_LEN + GCM_TAG_LEN + OUTER_CHECKSUM_LEN {
            return Err(BackupError::InvalidLength);
        }
        let header = Header::decode(&package[..HEADER_LEN])?;
        self.ensure_plaintext_size(header.plaintext_len)?;
        let expected = header
            .plaintext_len
            .checked_add(ENVELOPE_OVERHEAD)
            .ok_or(BackupError::SizeLimit)?;
        if package.len() != expected {
            return Err(BackupError::InvalidLength);
        }
        let checksum_offset = package.len() - OUTER_CHECKSUM_LEN;
        let actual = Sha256::digest(&package[..checksum_offset]);
        if actual.as_slice() != &package[checksum_offset..] {
            return Err(BackupError::ChecksumMismatch);
        }
        Ok((header, &package[HEADER_LEN..checksum_offset]))
    }
}

fn validate_passphrase(passphrase: &[u8]) -> Result<()> {
    if !(8..=1024).contains(&passphrase.len()) {
        return Err(BackupError::InvalidPassphrase);
    }
    Ok(())
}

fn derive_key(passphrase: &[u8], salt: &[u8; SALT_LEN]) -> Result<Zeroizing<[u8; 32]>> {
    // The format pins cost parameters so a modified header cannot force an
    // attacker-chosen memory/time allocation during restore.
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_PASSES, ARGON2_LANES, Some(32))
        .map_err(|_| BackupError::CryptoFailure)?;
    let algorithm = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0_u8; 32]);
    algorithm
        .hash_password_into(passphrase, salt, &mut *key)
        .map_err(|_| BackupError::CryptoFailure)?;
    Ok(key)
}

struct Header {
    plaintext_len: usize,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
}

impl Header {
    fn encode(&self) -> [u8; HEADER_LEN] {
        let mut result = [0_u8; HEADER_LEN];
        result[0..8].copy_from_slice(&MAGIC);
        result[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        // Bytes 10..12 are reserved and fixed zero.
        result[12..16].copy_from_slice(&ARGON2_MEMORY_KIB.to_le_bytes());
        result[16..20].copy_from_slice(&ARGON2_PASSES.to_le_bytes());
        result[20..24].copy_from_slice(&ARGON2_LANES.to_le_bytes());
        result[24..32].copy_from_slice(&(self.plaintext_len as u64).to_le_bytes());
        result[32..48].copy_from_slice(&self.salt);
        result[48..60].copy_from_slice(&self.nonce);
        result
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes[..8] != MAGIC {
            return Err(BackupError::InvalidMagic);
        }
        if u16::from_le_bytes(bytes[8..10].try_into().expect("fixed header")) != FORMAT_VERSION {
            return Err(BackupError::UnsupportedVersion);
        }
        let memory = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed header"));
        let passes = u32::from_le_bytes(bytes[16..20].try_into().expect("fixed header"));
        let lanes = u32::from_le_bytes(bytes[20..24].try_into().expect("fixed header"));
        if bytes[10..12] != [0, 0]
            || memory != ARGON2_MEMORY_KIB
            || passes != ARGON2_PASSES
            || lanes != ARGON2_LANES
        {
            return Err(BackupError::UnsupportedParameters);
        }
        let declared = u64::from_le_bytes(bytes[24..32].try_into().expect("fixed header"));
        let plaintext_len = usize::try_from(declared).map_err(|_| BackupError::SizeLimit)?;
        Ok(Self {
            plaintext_len,
            salt: bytes[32..48].try_into().expect("fixed header"),
            nonce: bytes[48..60].try_into().expect("fixed header"),
        })
    }
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<()> {
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<()> {
    // Directory fsync is unavailable through portable std on Windows.
    Ok(())
}
