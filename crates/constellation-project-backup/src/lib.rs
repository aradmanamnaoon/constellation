//! Portable encrypted archive of exactly two WAL-consistent SQLite snapshots.
//! No network, execution, global state, or disk plaintext staging is involved
//! during backup. A restore atomically publishes a verified private directory.

use std::{
    fs::{self, Metadata, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use constellation_backup::{BackupCodec, BackupLimits, HARD_MAX_PLAINTEXT};
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags, MAIN_DB,
};
use sha2::{Digest, Sha256};
use tempfile::Builder;
use thiserror::Error;
use zeroize::Zeroizing;

pub const ARCHIVE_VERSION: u16 = 1;
pub const ARCHIVE_MAGIC: [u8; 8] = *b"CSTLPRJ1";
pub const STUDIO_NAME: &str = "studio.sqlite3";
pub const WORKFLOWS_NAME: &str = "workflows.sqlite3";
pub const MAX_PLAINTEXT_BYTES: usize = HARD_MAX_PLAINTEXT;

const ARCHIVE_HEADER_LEN: usize = 96;
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(30);
const BACKUP_PAUSE: Duration = Duration::from_millis(20);

#[derive(Debug, Error)]
pub enum ProjectBackupError {
    #[error("invalid source database: expected a regular SQLite file")]
    InvalidSource,
    #[error("source paths must identify two distinct databases")]
    DuplicateSource,
    #[error("unsafe path component, symlink or invalid destination")]
    UnsafePath,
    #[error("destination already exists; refusing to overwrite")]
    OutputExists,
    #[error("encrypted archive or database exceeds the 64 MiB plaintext budget")]
    SizeLimit,
    #[error("SQLite online backup timed out")]
    SnapshotTimeout,
    #[error("SQLite image failed full integrity_check")]
    Integrity,
    #[error("captured SQLite pair failed the caller's consistency invariant")]
    PairMismatch,
    #[error("invalid archive version, metadata, image length or checksum")]
    InvalidArchive,
    #[error("invalid SQLite image header or journal mode")]
    InvalidDatabase,
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("encryption error: {0}")]
    Crypto(#[from] constellation_backup::BackupError),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ProjectBackupError>;
type PairValidator<'a> = dyn Fn(&Connection, &Connection) -> Result<()> + 'a;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveInfo {
    pub version: u16,
    pub studio_bytes: usize,
    pub workflows_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredPaths {
    pub directory: PathBuf,
    pub studio: PathBuf,
    pub workflows: PathBuf,
}

/// This service only returns the two known database filenames; no filenames
/// or filesystem paths can be supplied from encrypted archive metadata.
#[derive(Debug, Clone, Default)]
pub struct ProjectBackupService;

impl ProjectBackupService {
    /// SQLite online snapshots include committed data still in both WALs.
    /// The encrypted archive is atomically published under a NEW filename;
    /// no plaintext database files are written during backup.
    pub fn create_archive(
        &self,
        studio_database: impl AsRef<Path>,
        workflows_database: impl AsRef<Path>,
        new_encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
    ) -> Result<ArchiveInfo> {
        self.create_archive_inner(
            studio_database,
            workflows_database,
            new_encrypted_archive,
            passphrase,
            None,
            None,
        )
    }

    /// Enforce an application-level invariant across the two *captured* images.
    /// The validator receives query-only in-memory SQLite connections, after
    /// the independent online snapshots and before any encrypted output exists.
    ///
    /// A caller can, for example, verify a durable generation marker stored in
    /// both databases, provided its writers maintain that marker under a real
    /// cross-database coordination protocol. Matching markers on their own do
    /// not create an atomic two-database snapshot.
    pub fn create_archive_with_pair_validator(
        &self,
        studio_database: impl AsRef<Path>,
        workflows_database: impl AsRef<Path>,
        new_encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
        validator: &dyn Fn(&Connection, &Connection) -> Result<()>,
    ) -> Result<ArchiveInfo> {
        self.create_archive_inner(
            studio_database,
            workflows_database,
            new_encrypted_archive,
            passphrase,
            Some(validator),
            None,
        )
    }

    /// Opt-in bounded two-source writer fence. A separate read-write SQLite
    /// connection acquires BEGIN IMMEDIATE on each canonical source in a
    /// deterministic path order. Both reservations remain held while the
    /// existing online Backup API reads from *other* read-only connections.
    /// Thus ordinary SQLite writers cannot commit to either DB during either
    /// snapshot. The guards release before encryption or file publication.
    ///
    /// This is NOT an atomic distributed transaction: application operations
    /// spanning two files may have committed only one side *before* the fence.
    /// Writers bypassing SQLite, or non-database side effects, are not fenced.
    /// Timeout must be between 1ns and 30s; failure publishes no archive.
    pub fn create_archive_coordinated(
        &self,
        studio_database: impl AsRef<Path>,
        workflows_database: impl AsRef<Path>,
        new_encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
        timeout: Duration,
    ) -> Result<ArchiveInfo> {
        self.create_archive_inner(
            studio_database,
            workflows_database,
            new_encrypted_archive,
            passphrase,
            None,
            Some(timeout),
        )
    }

    /// The same optional SQLite writer fence, with a query-only
    /// application-specific pair check on the two captured images.
    pub fn create_archive_coordinated_with_pair_validator(
        &self,
        studio_database: impl AsRef<Path>,
        workflows_database: impl AsRef<Path>,
        new_encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
        timeout: Duration,
        validator: &dyn Fn(&Connection, &Connection) -> Result<()>,
    ) -> Result<ArchiveInfo> {
        self.create_archive_inner(
            studio_database,
            workflows_database,
            new_encrypted_archive,
            passphrase,
            Some(validator),
            Some(timeout),
        )
    }

    fn create_archive_inner(
        &self,
        studio_database: impl AsRef<Path>,
        workflows_database: impl AsRef<Path>,
        new_encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
        validator: Option<&PairValidator<'_>>,
        coordination_timeout: Option<Duration>,
    ) -> Result<ArchiveInfo> {
        let studio = studio_database.as_ref();
        let workflows = workflows_database.as_ref();
        ensure_regular_source(studio)?;
        ensure_regular_source(workflows)?;
        if fs::canonicalize(studio)? == fs::canonicalize(workflows)? {
            return Err(ProjectBackupError::DuplicateSource);
        }
        // Check new target first, so user mistakes do not trigger expensive KDF.
        let target = verify_output_archive(new_encrypted_archive.as_ref())?;
        // In coordinated mode, lock both files *before* capturing either.
        // Always open the locked canonical paths to avoid naming a different
        // file through an alias between reservation and snapshot.
        let (guards, deadline, studio_path, workflows_path) =
            if let Some(timeout) = coordination_timeout {
                let studio_path = fs::canonicalize(studio)?;
                let workflows_path = fs::canonicalize(workflows)?;
                let deadline = coordinated_deadline(timeout)?;
                let guards = acquire_writer_fences(&studio_path, &workflows_path, deadline)?;
                (guards, Some(deadline), studio_path, workflows_path)
            } else {
                (
                    Vec::new(),
                    None,
                    studio.to_path_buf(),
                    workflows.to_path_buf(),
                )
            };
        let mut studio_image = snapshot_database(&studio_path, MAX_PLAINTEXT_BYTES, deadline)?;
        let remaining = MAX_PLAINTEXT_BYTES
            .checked_sub(ARCHIVE_HEADER_LEN + studio_image.len())
            .ok_or(ProjectBackupError::SizeLimit)?;
        let mut workflows_image = snapshot_database(&workflows_path, remaining, deadline)?;
        ensure_capture_deadline(deadline)?;
        // Release both reservations even if the validator rejects a pair.
        // Do not hold write locks during potentially expensive KDF/encryption
        // or a host-supplied validator callback.
        drop(guards);
        let plaintext_len = ARCHIVE_HEADER_LEN
            .checked_add(studio_image.len())
            .and_then(|n| n.checked_add(workflows_image.len()))
            .ok_or(ProjectBackupError::SizeLimit)?;
        if plaintext_len > MAX_PLAINTEXT_BYTES {
            return Err(ProjectBackupError::SizeLimit);
        }
        validate_pair_if_requested(&studio_image, &workflows_image, validator)?;
        let info = ArchiveInfo {
            version: ARCHIVE_VERSION,
            studio_bytes: studio_image.len(),
            workflows_bytes: workflows_image.len(),
        };
        let mut plaintext = Zeroizing::new(Vec::with_capacity(plaintext_len));
        plaintext.extend_from_slice(&ARCHIVE_MAGIC);
        plaintext.extend_from_slice(&ARCHIVE_VERSION.to_le_bytes());
        plaintext.extend_from_slice(&2_u16.to_le_bytes());
        plaintext.extend_from_slice(&0_u32.to_le_bytes());
        plaintext.extend_from_slice(&(studio_image.len() as u64).to_le_bytes());
        plaintext.extend_from_slice(&(workflows_image.len() as u64).to_le_bytes());
        plaintext.extend_from_slice(&Sha256::digest(&studio_image));
        plaintext.extend_from_slice(&Sha256::digest(&workflows_image));
        debug_assert_eq!(plaintext.len(), ARCHIVE_HEADER_LEN);
        plaintext.extend_from_slice(&studio_image);
        plaintext.extend_from_slice(&workflows_image);
        // Reclaim source copies before Argon2 cost and cipher output allocation.
        studio_image.fill(0);
        workflows_image.fill(0);
        BackupCodec::new(BackupLimits::new(MAX_PLAINTEXT_BYTES)?)
            .encrypt_to_new_file(&target, &plaintext, passphrase)?;
        Ok(info)
    }

    /// Verify the entire encrypted archive and each SQLite image before
    /// creating the destination directory. An existing directory or symlink
    /// is never overwritten, and both restored filenames are fixed.
    pub fn restore_archive(
        &self,
        encrypted_archive: impl AsRef<Path>,
        new_directory: impl AsRef<Path>,
        passphrase: &[u8],
    ) -> Result<RestoredPaths> {
        self.restore_archive_inner(encrypted_archive, new_directory, passphrase, None)
    }

    /// Check the application's cross-database invariant on authenticated
    /// SQLite images before creating any output directory.
    pub fn restore_archive_with_pair_validator(
        &self,
        encrypted_archive: impl AsRef<Path>,
        new_directory: impl AsRef<Path>,
        passphrase: &[u8],
        validator: &dyn Fn(&Connection, &Connection) -> Result<()>,
    ) -> Result<RestoredPaths> {
        self.restore_archive_inner(
            encrypted_archive,
            new_directory,
            passphrase,
            Some(validator),
        )
    }

    fn restore_archive_inner(
        &self,
        encrypted_archive: impl AsRef<Path>,
        new_directory: impl AsRef<Path>,
        passphrase: &[u8],
        validator: Option<&PairValidator<'_>>,
    ) -> Result<RestoredPaths> {
        let source = encrypted_archive.as_ref();
        ensure_regular_source(source)?;
        let destination = verify_new_directory(new_directory.as_ref())?;
        let codec = BackupCodec::new(BackupLimits::new(MAX_PLAINTEXT_BYTES)?);
        let plaintext = codec.decrypt_from_file(source, passphrase)?;
        let (studio_image, workflows_image, _) = validate_archive(&plaintext)?;
        verify_database_image(studio_image)?;
        verify_database_image(workflows_image)?;
        validate_pair_if_requested(studio_image, workflows_image, validator)?;

        // Populate a private sibling directory on the destination filesystem.
        // No final destination is visible until both SQLite files pass checks.
        let parent = destination
            .parent()
            .ok_or(ProjectBackupError::UnsafePath)?
            .to_path_buf();
        let staged = Builder::new()
            .prefix(".constellation-restore-")
            .tempdir_in(&parent)?;
        restrict_directory_permissions(staged.path())?;
        let result = (|| {
            let studio = staged.path().join(STUDIO_NAME);
            let workflows = staged.path().join(WORKFLOWS_NAME);
            write_new_file(&studio, studio_image)?;
            write_new_file(&workflows, workflows_image)?;
            // Verify published bytes, not only deserialized input images.
            verify_restored_file(&studio)?;
            verify_restored_file(&workflows)?;
            sync_directory(staged.path())?;
            publish_new_directory(staged.path(), &destination)?;
            // `staged` no longer points to a directory after publication.
            // Do not let its Drop handler clean an unrelated later path.
            let _ = staged.keep();
            sync_directory(&parent)?;
            Ok(RestoredPaths {
                studio: destination.join(STUDIO_NAME),
                workflows: destination.join(WORKFLOWS_NAME),
                directory: destination,
            })
        })();
        result
    }

    /// Authenticate and verify all metadata and both DB images in memory,
    /// without creating output paths. Suitable for preflight UI checks.
    pub fn verify_archive(
        &self,
        encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
    ) -> Result<ArchiveInfo> {
        self.verify_archive_inner(encrypted_archive, passphrase, None)
    }

    /// Validate an encrypted archive's two original database images together
    /// against a caller-supplied application invariant without restoring.
    pub fn verify_archive_with_pair_validator(
        &self,
        encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
        validator: &dyn Fn(&Connection, &Connection) -> Result<()>,
    ) -> Result<ArchiveInfo> {
        self.verify_archive_inner(encrypted_archive, passphrase, Some(validator))
    }

    fn verify_archive_inner(
        &self,
        encrypted_archive: impl AsRef<Path>,
        passphrase: &[u8],
        validator: Option<&PairValidator<'_>>,
    ) -> Result<ArchiveInfo> {
        let source = encrypted_archive.as_ref();
        ensure_regular_source(source)?;
        let plaintext = BackupCodec::new(BackupLimits::new(MAX_PLAINTEXT_BYTES)?)
            .decrypt_from_file(source, passphrase)?;
        let (studio, workflows, info) = validate_archive(&plaintext)?;
        verify_database_image(studio)?;
        verify_database_image(workflows)?;
        validate_pair_if_requested(studio, workflows, validator)?;
        Ok(info)
    }
}

fn validate_pair_if_requested(
    studio: &[u8],
    workflows: &[u8],
    validator: Option<&PairValidator<'_>>,
) -> Result<()> {
    let Some(validator) = validator else {
        return Ok(());
    };
    let mut studio_db = Connection::open_in_memory()?;
    let mut workflows_db = Connection::open_in_memory()?;
    studio_db.deserialize_read_exact(MAIN_DB, studio, studio.len(), true)?;
    workflows_db.deserialize_read_exact(MAIN_DB, workflows, workflows.len(), true)?;
    studio_db.pragma_update(None, "query_only", "ON")?;
    workflows_db.pragma_update(None, "query_only", "ON")?;
    validator(&studio_db, &workflows_db)
}

fn snapshot_database(
    path: &Path,
    limit: usize,
    deadline: Option<Instant>,
) -> Result<Zeroizing<Vec<u8>>> {
    ensure_capture_deadline(deadline)?;
    if limit < 100 {
        return Err(ProjectBackupError::SizeLimit);
    }
    let source = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    source.busy_timeout(Duration::from_millis(250))?;
    size_guard(&source, limit)?;
    let mut snapshot = Connection::open_in_memory()?;
    online_backup(&source, &mut snapshot, limit, deadline)?;
    check_integrity(&snapshot)?;
    size_guard(&snapshot, limit)?;
    let serialized = snapshot.serialize(MAIN_DB)?;
    if serialized.len() > limit {
        return Err(ProjectBackupError::SizeLimit);
    }
    let mut image = Zeroizing::new(serialized.to_vec());
    normalize_header(&mut image)?;
    verify_database_image(&image)?;
    ensure_capture_deadline(deadline)?;
    Ok(image)
}

fn online_backup(
    source: &Connection,
    target: &mut Connection,
    limit: usize,
    deadline: Option<Instant>,
) -> Result<()> {
    size_guard(source, limit)?;
    let backup = Backup::new(source, target)?;
    let started = Instant::now();
    let page_size: i64 = source.pragma_query_value(None, "page_size", |row| row.get(0))?;
    loop {
        if started.elapsed() >= SNAPSHOT_TIMEOUT
            || deadline.is_some_and(|until| Instant::now() >= until)
        {
            return Err(ProjectBackupError::SnapshotTimeout);
        }
        let status = backup.step(64)?;
        let pages = i64::from(backup.progress().pagecount.max(0));
        if pages.saturating_mul(page_size) > limit as i64 {
            return Err(ProjectBackupError::SizeLimit);
        }
        match status {
            StepResult::Done => return Ok(()),
            StepResult::More | StepResult::Busy | StepResult::Locked => {
                thread::sleep(BACKUP_PAUSE);
            }
            _ => return Err(ProjectBackupError::SnapshotTimeout),
        }
    }
}

fn coordinated_deadline(timeout: Duration) -> Result<Instant> {
    if timeout.is_zero() || timeout > SNAPSHOT_TIMEOUT {
        return Err(ProjectBackupError::SnapshotTimeout);
    }
    Instant::now()
        .checked_add(timeout)
        .ok_or(ProjectBackupError::SnapshotTimeout)
}

fn ensure_capture_deadline(deadline: Option<Instant>) -> Result<()> {
    if deadline.is_some_and(|until| Instant::now() >= until) {
        return Err(ProjectBackupError::SnapshotTimeout);
    }
    Ok(())
}

fn acquire_writer_fences(
    studio: &Path,
    workflows: &Path,
    deadline: Instant,
) -> Result<Vec<Connection>> {
    // Canonical path order prevents backups competing over reversed source
    // arguments from holding one DB each while waiting on the other.
    let mut ordered = [studio, workflows];
    ordered.sort();
    let mut guards = Vec::with_capacity(2);
    for source in ordered {
        ensure_capture_deadline(Some(deadline))?;
        let connection = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ProjectBackupError::SnapshotTimeout);
        }
        connection.busy_timeout(remaining)?;
        if let Err(error) = connection.execute_batch("BEGIN IMMEDIATE") {
            // SQLite's busy/locked condition is an expected failure to
            // obtain the exclusive writer reservation before this deadline.
            if let rusqlite::Error::SqliteFailure(code, _) = &error {
                if code.extended_code & 0xff == rusqlite::ffi::SQLITE_BUSY
                    || code.extended_code & 0xff == rusqlite::ffi::SQLITE_LOCKED
                {
                    return Err(ProjectBackupError::SnapshotTimeout);
                }
            }
            return Err(ProjectBackupError::Sqlite(error));
        }
        guards.push(connection);
    }
    ensure_capture_deadline(Some(deadline))?;
    Ok(guards)
}

fn size_guard(connection: &Connection, limit: usize) -> Result<()> {
    let page_size: i64 = connection.pragma_query_value(None, "page_size", |row| row.get(0))?;
    let count: i64 = connection.pragma_query_value(None, "page_count", |row| row.get(0))?;
    if page_size < 1 || count < 0 || page_size.saturating_mul(count) > limit as i64 {
        return Err(ProjectBackupError::SizeLimit);
    }
    Ok(())
}

fn check_integrity(db: &Connection) -> Result<()> {
    let status: String = db.query_row("PRAGMA main.integrity_check", [], |row| row.get(0))?;
    if status != "ok" {
        return Err(ProjectBackupError::Integrity);
    }
    Ok(())
}

fn normalize_header(image: &mut [u8]) -> Result<()> {
    if image.len() < 100 || !image.starts_with(b"SQLite format 3\0") {
        return Err(ProjectBackupError::InvalidDatabase);
    }
    match (image[18], image[19]) {
        (1, 1) => Ok(()),
        (2, 2) => {
            image[18] = 1;
            image[19] = 1;
            Ok(())
        }
        _ => Err(ProjectBackupError::InvalidDatabase),
    }
}

fn verify_database_image(image: &[u8]) -> Result<()> {
    if image.len() < 100 || !image.starts_with(b"SQLite format 3\0") {
        return Err(ProjectBackupError::InvalidDatabase);
    }
    if image[18] != 1 || image[19] != 1 {
        return Err(ProjectBackupError::InvalidDatabase);
    }
    // A valid SQLite image must occupy exactly the pages declared in its
    // header. Extra authenticated bytes can otherwise be ignored by SQLite's
    // logical integrity_check (e.g. data appended as a disguised attachment).
    let encoded_page_size = usize::from(u16::from_be_bytes([image[16], image[17]]));
    let page_size = if encoded_page_size == 1 {
        65_536
    } else {
        encoded_page_size
    };
    let page_count = u32::from_be_bytes(image[28..32].try_into().expect("SQLite header"));
    if !(512..=65_536).contains(&page_size)
        || !page_size.is_power_of_two()
        || page_count == 0
        || usize::try_from(page_count)
            .ok()
            .and_then(|pages| pages.checked_mul(page_size))
            != Some(image.len())
    {
        return Err(ProjectBackupError::InvalidDatabase);
    }
    let mut connection = Connection::open_in_memory()?;
    connection.deserialize_read_exact(MAIN_DB, image, image.len(), true)?;
    check_integrity(&connection)
}

fn validate_archive(data: &[u8]) -> Result<(&[u8], &[u8], ArchiveInfo)> {
    if data.len() < ARCHIVE_HEADER_LEN || data[..8] != ARCHIVE_MAGIC {
        return Err(ProjectBackupError::InvalidArchive);
    }
    let version = u16::from_le_bytes(data[8..10].try_into().expect("fixed header"));
    if version != ARCHIVE_VERSION || data[10..12] != 2_u16.to_le_bytes() || data[12..16] != [0; 4] {
        return Err(ProjectBackupError::InvalidArchive);
    }
    let studio_len = u64::from_le_bytes(data[16..24].try_into().expect("fixed header"));
    let workflows_len = u64::from_le_bytes(data[24..32].try_into().expect("fixed header"));
    let studio_len = usize::try_from(studio_len).map_err(|_| ProjectBackupError::InvalidArchive)?;
    let workflows_len =
        usize::try_from(workflows_len).map_err(|_| ProjectBackupError::InvalidArchive)?;
    if studio_len < 100 || workflows_len < 100 || studio_len > MAX_PLAINTEXT_BYTES {
        return Err(ProjectBackupError::InvalidArchive);
    }
    let end = ARCHIVE_HEADER_LEN
        .checked_add(studio_len)
        .and_then(|offset| offset.checked_add(workflows_len))
        .ok_or(ProjectBackupError::InvalidArchive)?;
    if end != data.len() || end > MAX_PLAINTEXT_BYTES {
        return Err(ProjectBackupError::InvalidArchive);
    }
    let studio = &data[ARCHIVE_HEADER_LEN..ARCHIVE_HEADER_LEN + studio_len];
    let workflows = &data[ARCHIVE_HEADER_LEN + studio_len..end];
    if Sha256::digest(studio).as_slice() != &data[32..64]
        || Sha256::digest(workflows).as_slice() != &data[64..96]
    {
        return Err(ProjectBackupError::InvalidArchive);
    }
    Ok((
        studio,
        workflows,
        ArchiveInfo {
            version,
            studio_bytes: studio_len,
            workflows_bytes: workflows_len,
        },
    ))
}

fn ensure_no_symlink_components(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut traversed = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => return Err(ProjectBackupError::UnsafePath),
            Component::CurDir => continue,
            Component::Prefix(_) => {
                // On Windows, canonicalize yields an extended-length path
                // such as `\\?\C:\...`. The prefix without its RootDir is
                // not a complete filesystem path: symlink_metadata on
                // `\\?\C:` fails with ERROR_INVALID_FUNCTION (os error 1).
                // Retain the prefix, then inspect it as soon as RootDir is
                // appended, and keep checking every real path component.
                traversed.push(component.as_os_str());
            }
            Component::RootDir | Component::Normal(_) => {
                traversed.push(component.as_os_str());
                match fs::symlink_metadata(&traversed) {
                    Ok(metadata) if is_link_or_reparse_point(&metadata) => {
                        return Err(ProjectBackupError::UnsafePath)
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(ProjectBackupError::Io(error)),
                }
            }
        }
    }
    Ok(())
}

fn ensure_regular_source(path: &Path) -> Result<()> {
    ensure_no_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| ProjectBackupError::InvalidSource)?;
    if !metadata.file_type().is_file() || is_link_or_reparse_point(&metadata) {
        return Err(ProjectBackupError::InvalidSource);
    }
    Ok(())
}

fn verified_parent_and_target(path: &Path) -> Result<(PathBuf, PathBuf)> {
    ensure_no_symlink_components(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let canonical = fs::canonicalize(parent).map_err(|_| ProjectBackupError::UnsafePath)?;
    let name = path.file_name().ok_or(ProjectBackupError::UnsafePath)?;
    let target = canonical.join(name);
    match fs::symlink_metadata(&target) {
        Ok(_) => return Err(ProjectBackupError::OutputExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ProjectBackupError::Io(error)),
    }
    Ok((canonical, target))
}

fn verify_output_archive(path: &Path) -> Result<PathBuf> {
    let (_, target) = verified_parent_and_target(path)?;
    Ok(target)
}

fn verify_new_directory(path: &Path) -> Result<PathBuf> {
    let (_, target) = verified_parent_and_target(path)?;
    Ok(target)
}

#[cfg(windows)]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // Reject junctions, mount points and other redirection tags, not just
    // the narrower Windows symbolic-link variant.
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse_point(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn restrict_directory_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Publish the complete pair as a single directory entry. This must be an
/// atomic *no replace* rename: ordinary rename may replace an existing empty
/// destination directory on Unix or Windows and is therefore insufficient.
#[cfg(target_os = "linux")]
fn publish_new_directory(source: &Path, destination: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| ProjectBackupError::UnsafePath)?;
    let to = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| ProjectBackupError::UnsafePath)?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    translate_rename_result(result)
}

#[cfg(target_os = "macos")]
fn publish_new_directory(source: &Path, destination: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| ProjectBackupError::UnsafePath)?;
    let to = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| ProjectBackupError::UnsafePath)?;
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    translate_rename_result(result)
}

#[cfg(windows)]
fn publish_new_directory(source: &Path, destination: &Path) -> Result<()> {
    use std::{iter, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let from: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let to: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    // Flags zero means neither REPLACE_EXISTING nor COPY_ALLOWED.
    let result = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        // ERROR_ALREADY_EXISTS / ERROR_FILE_EXISTS
        if matches!(error.raw_os_error(), Some(183 | 80)) {
            return Err(ProjectBackupError::OutputExists);
        }
        return Err(ProjectBackupError::Io(error));
    }
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn publish_new_directory(_source: &Path, _destination: &Path) -> Result<()> {
    // An unsupported platform must fail closed rather than use a replacement
    // rename that can destroy a concurrently created directory.
    Err(ProjectBackupError::UnsafePath)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn translate_rename_result(result: libc::c_int) -> Result<()> {
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        Err(ProjectBackupError::OutputExists)
    } else {
        Err(ProjectBackupError::Io(error))
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn verify_restored_file(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    check_integrity(&conn)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod coordination_tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn two_wal_writer_fences_block_independent_threads_but_allow_read_only_backups() {
        let dir = tempfile::tempdir().unwrap();
        let paths = [
            dir.path().join("a-studio.db"),
            dir.path().join("z-workflows.db"),
        ];
        let mut original_connections = Vec::new();
        for path in &paths {
            let conn = Connection::open(path).unwrap();
            let journal: String = conn
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .unwrap_or_default();
            if journal != "wal" {
                conn.pragma_update(None, "journal_mode", "WAL").unwrap();
            }
            conn.execute_batch(
                "PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE entries(value INTEGER NOT NULL);
                 INSERT INTO entries VALUES(7)",
            )
            .unwrap();
            original_connections.push(conn);
        }
        let deadline = coordinated_deadline(Duration::from_secs(5)).unwrap();
        let fences = acquire_writer_fences(&paths[0], &paths[1], deadline).unwrap();

        // Separate independent writer connections in other threads cannot
        // commit even though source snapshots on third connections succeed.
        let (sender, receiver) = mpsc::channel();
        let mut writers = Vec::new();
        for path in &paths {
            let path = path.to_owned();
            let sender = sender.clone();
            writers.push(thread::spawn(move || {
                let conn = Connection::open(&path).unwrap();
                conn.busy_timeout(Duration::from_millis(70)).unwrap();
                let blocked = conn.execute("INSERT INTO entries VALUES(8)", []).is_err();
                sender.send(blocked).unwrap();
            }));
        }
        drop(sender);
        assert!(receiver.into_iter().all(|blocked| blocked));
        for writer in writers {
            writer.join().unwrap();
        }

        let studio_image =
            snapshot_database(&paths[0], MAX_PLAINTEXT_BYTES, Some(deadline)).unwrap();
        let workflows_image =
            snapshot_database(&paths[1], MAX_PLAINTEXT_BYTES, Some(deadline)).unwrap();
        validate_pair_if_requested(
            &studio_image,
            &workflows_image,
            Some(&|studio, workflows| {
                for db in [studio, workflows] {
                    let only_value: i64 =
                        db.query_row("SELECT value FROM entries", [], |row| row.get(0))?;
                    assert_eq!(only_value, 7);
                }
                Ok(())
            }),
        )
        .unwrap();

        drop(fences);
        for conn in original_connections {
            conn.execute("INSERT INTO entries VALUES(9)", []).unwrap();
        }
    }
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    // Portable std has no directory fsync on Windows. Files are synced.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_publication_refuses_occupied_destination_even_after_preflight() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("private-stage");
        let target = root.path().join("target");
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("new.txt"), b"new data").unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("old.txt"), b"existing user data").unwrap();

        // Exercise the OS publication primitive after preflight: another
        // writer could create the destination while we are preparing data.
        assert!(publish_new_directory(&staged, &target).is_err());
        assert_eq!(
            fs::read(target.join("old.txt")).unwrap(),
            b"existing user data"
        );
        assert_eq!(fs::read(staged.join("new.txt")).unwrap(), b"new data");

        let fresh = root.path().join("fresh");
        publish_new_directory(&staged, &fresh).unwrap();
        assert_eq!(fs::read(fresh.join("new.txt")).unwrap(), b"new data");
        assert!(!staged.exists());
    }
}
