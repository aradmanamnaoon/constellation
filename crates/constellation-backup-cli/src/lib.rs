//! Offline, user-invoked SQLite backup and restore operations.
//!
//! The online SQLite Backup API is mandatory: copying a live .sqlite3 file
//! omits committed WAL data. This crate does not modify the application DB.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use constellation_backup::{BackupCodec, BackupLimits, HARD_MAX_PLAINTEXT};
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags, MAIN_DB,
};
use tempfile::{Builder, NamedTempFile};
use zeroize::Zeroizing;

const BACKUP_TIMEOUT: Duration = Duration::from_secs(30);
const STEP_PAUSE: Duration = Duration::from_millis(20);

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("source is missing or is not a regular file")]
    InvalidSource,
    #[error("output already exists; refusing to overwrite")]
    OutputExists,
    #[error("SQLite destination has an existing journal, WAL, or shared-memory sidecar")]
    ExistingSidecar,
    #[error("destination directory does not exist")]
    MissingParent,
    #[error("database snapshot or restore timed out")]
    SnapshotTimeout,
    #[error("SQLite integrity_check failed")]
    IntegrityFailure,
    #[error("database image has invalid SQLite header")]
    InvalidDatabase,
    #[error("encrypted package or database exceeds configured size limit")]
    SizeLimit,
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("SQLite {0} error: {1}")]
    SqliteAt(&'static str, #[source] rusqlite::Error),
    #[error("encrypted package error: {0}")]
    Package(#[from] constellation_backup::BackupError),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CliError>;

/// All operations create output files exclusively, never replacing an existing
/// name. A destination can still be removed by external processes after this
/// function returns; persistent backups should be copied to safe storage.
#[derive(Clone, Debug)]
pub struct BackupService {
    codec: BackupCodec,
}

impl BackupService {
    pub fn new(limits: BackupLimits) -> Self {
        Self {
            codec: BackupCodec::new(limits),
        }
    }

    pub fn limits(&self) -> BackupLimits {
        self.codec.limits()
    }

    /// Capture a committed, transactionally consistent SQLite image, including
    /// data still in an active WAL, then write only ciphertext to disk.
    ///
    /// Passphrase bytes are never read from an argument/environment by this
    /// library; the executable obtains them via a no-echo console.
    pub fn backup_database(
        &self,
        source: impl AsRef<Path>,
        new_package: impl AsRef<Path>,
        passphrase: &[u8],
    ) -> Result<()> {
        let source = source.as_ref();
        verify_source(source)?;
        let destination = new_package.as_ref();
        let output = verify_new_destination(destination, false)?;

        let snapshot = self.snapshot_database(source)?;
        let encrypted = self.codec.encrypt(&snapshot, passphrase)?;
        // NamedTempFile uses restrictive, random file names within the target
        // directory. persist_noclobber is atomic against competing creators.
        persist_new_file(&output.parent, &output.target, &encrypted)?;
        Ok(())
    }

    /// Authenticates/decrypts the package, checks the SQLite image in memory,
    /// and creates the restored database at an exclusively NEW path.
    ///
    /// The restore staging file is unencrypted because the *destination* is an
    /// ordinary SQLite database. It is kept private and removed on failures.
    pub fn restore_database(
        &self,
        source_package: impl AsRef<Path>,
        new_database: impl AsRef<Path>,
        passphrase: &[u8],
    ) -> Result<()> {
        let package = source_package.as_ref();
        verify_source(package)?;
        let target = new_database.as_ref();
        let output = verify_new_destination(target, true)?;
        let mut plaintext = self.codec.decrypt_from_file(package, passphrase)?;
        check_sqlite_header(&plaintext)?;
        normalize_snapshot_journal_mode(&mut plaintext)?;

        let mut input = Connection::open_in_memory()?;
        input
            .deserialize_read_exact(MAIN_DB, plaintext.as_slice(), plaintext.len(), true)
            .map_err(|e| CliError::SqliteAt("deserialize", e))?;
        check_integrity(&input).map_err(|e| match e {
            CliError::Sqlite(err) => CliError::SqliteAt("deserialized integrity", err),
            err => err,
        })?;
        // The decrypted buffer is zeroized on drop; SQLite owns its own
        // internal deserialized image until the connection is closed.

        let staged = Builder::new()
            .prefix(".constellation-restore-")
            .suffix(".sqlite3")
            .tempfile_in(&output.parent)?
            .into_temp_path();
        {
            let mut target_db =
                Connection::open_with_flags(&staged, OpenFlags::SQLITE_OPEN_READ_WRITE)
                    .map_err(|e| CliError::SqliteAt("staging open", e))?;
            sqlite_backup(&input, &mut target_db, self.codec.limits().max_plaintext()).map_err(
                |e| match e {
                    CliError::Sqlite(err) => CliError::SqliteAt("restore online backup", err),
                    err => err,
                },
            )?;
            check_integrity(&target_db).map_err(|e| match e {
                CliError::Sqlite(err) => CliError::SqliteAt("staging integrity", err),
                err => err,
            })?;
            target_db
                .close()
                .map_err(|(_, error)| CliError::SqliteAt("restore close", error))?;
        }
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&staged)?
            .sync_all()?;
        // Reject sidecars again immediately before publishing. SQLite could
        // otherwise attach an old -wal/-shm file to the restored database.
        check_restore_sidecars(&output.target)?;
        persist_path_noclobber(staged, &output.target)?;
        sync_parent(&output.parent)?;
        Ok(())
    }

    fn snapshot_database(&self, source: &Path) -> Result<Zeroizing<Vec<u8>>> {
        let source_db = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        source_db.busy_timeout(Duration::from_millis(250))?;
        database_size_guard(&source_db, self.codec.limits().max_plaintext())?;

        let mut snapshot = Connection::open_in_memory()?;
        sqlite_backup(
            &source_db,
            &mut snapshot,
            self.codec.limits().max_plaintext(),
        )?;
        check_integrity(&snapshot)?;
        database_size_guard(&snapshot, self.codec.limits().max_plaintext())?;
        let serialized = snapshot.serialize(MAIN_DB)?;
        if serialized.len() > self.codec.limits().max_plaintext() {
            return Err(CliError::SizeLimit);
        }
        let mut image = Zeroizing::new(serialized.to_vec());
        normalize_snapshot_journal_mode(&mut image)?;
        Ok(image)
    }
}

fn sqlite_backup(source: &Connection, target: &mut Connection, max_bytes: usize) -> Result<()> {
    database_size_guard(source, max_bytes)?;
    let backup = Backup::new(source, target)?;
    let started = Instant::now();
    loop {
        if started.elapsed() > BACKUP_TIMEOUT {
            return Err(CliError::SnapshotTimeout);
        }
        let status = backup.step(64)?;
        let pages = i64::from(backup.progress().pagecount.max(0));
        let page_size: i64 = source.pragma_query_value(None, "page_size", |row| row.get(0))?;
        if page_size < 1 || pages.saturating_mul(page_size) > max_bytes as i64 {
            return Err(CliError::SizeLimit);
        }
        match status {
            StepResult::Done => return Ok(()),
            StepResult::More | StepResult::Busy | StepResult::Locked => thread::sleep(STEP_PAUSE),
            _ => return Err(CliError::SnapshotTimeout),
        }
    }
}

fn database_size_guard(db: &Connection, limit: usize) -> Result<()> {
    let page_size: i64 = db.pragma_query_value(None, "page_size", |row| row.get(0))?;
    let page_count: i64 = db.pragma_query_value(None, "page_count", |row| row.get(0))?;
    if page_size < 1 || page_count < 0 || page_size.saturating_mul(page_count) > limit as i64 {
        return Err(CliError::SizeLimit);
    }
    Ok(())
}

fn check_integrity(db: &Connection) -> Result<()> {
    // Full integrity_check (not quick_check) validates the snapshot before
    // encryption and restored image before publishing a new database file.
    let status: String = db.query_row("PRAGMA main.integrity_check", [], |row| row.get(0))?;
    if status != "ok" {
        return Err(CliError::IntegrityFailure);
    }
    Ok(())
}

fn check_sqlite_header(data: &[u8]) -> Result<()> {
    if data.len() < 100 || !data.starts_with(b"SQLite format 3\0") {
        return Err(CliError::InvalidDatabase);
    }
    Ok(())
}

fn normalize_snapshot_journal_mode(image: &mut [u8]) -> Result<()> {
    // SQLite's online Backup API includes committed WAL frames in the target
    // database, but serialized SQLite image headers can retain the source's
    // WAL read/write version bytes (2,2). Without a companion -wal/-shm,
    // sqlite3_deserialize(..., READONLY) may reject the self-contained image.
    // The SQLite file format defines (1,1) as rollback-journal format.
    check_sqlite_header(image)?;
    match (image[18], image[19]) {
        (1, 1) => Ok(()),
        (2, 2) => {
            image[18] = 1;
            image[19] = 1;
            Ok(())
        }
        _ => Err(CliError::InvalidDatabase),
    }
}

fn verify_source(file: &Path) -> Result<()> {
    // `metadata` follows symlinks. Reject even valid symlinks so the user's
    // explicitly selected source cannot silently resolve to another file.
    let metadata = fs::symlink_metadata(file).map_err(|_| CliError::InvalidSource)?;
    if !metadata.file_type().is_file() {
        return Err(CliError::InvalidSource);
    }
    Ok(())
}

struct NewDestination {
    parent: PathBuf,
    target: PathBuf,
}

fn verify_new_destination(path: &Path, is_database: bool) -> Result<NewDestination> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(CliError::MissingParent);
    }
    // Publish using the same resolved parent used for staging. Otherwise a
    // symlinked parent may be redirected between the checks and publication.
    let parent = fs::canonicalize(parent)?;
    let filename = path.file_name().ok_or(CliError::MissingParent)?;
    let target = parent.join(filename);
    verify_unoccupied(&target)?;
    if is_database {
        check_restore_sidecars(&target)?;
    }
    Ok(NewDestination { parent, target })
}

fn verify_unoccupied(path: &Path) -> Result<()> {
    // symlink_metadata treats dangling symlinks as occupied. Final publication
    // has a separate atomic no-clobber check against competing creators.
    match fs::symlink_metadata(path) {
        Ok(_) => return Err(CliError::OutputExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(())
}

fn check_restore_sidecars(target: &Path) -> Result<()> {
    for extension in ["-wal", "-shm", "-journal"] {
        let mut filename = target.as_os_str().to_os_string();
        filename.push(extension);
        match fs::symlink_metadata(Path::new(&filename)) {
            Ok(_) => return Err(CliError::ExistingSidecar),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Ok(())
}

fn persist_new_file(parent: &Path, target: &Path, data: &[u8]) -> Result<()> {
    let mut temp = Builder::new()
        .prefix(".constellation-encrypted-")
        .tempfile_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    persist_noclobber(temp, target)?;
    sync_parent(parent)?;
    Ok(())
}

fn persist_noclobber(temp: NamedTempFile, target: &Path) -> Result<()> {
    temp.persist_noclobber(target).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            CliError::OutputExists
        } else {
            CliError::Io(error.error)
        }
    })?;
    Ok(())
}

fn persist_path_noclobber(temp: tempfile::TempPath, target: &Path) -> Result<()> {
    temp.persist_noclobber(target).map_err(|error| {
        if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            CliError::OutputExists
        } else {
            CliError::Io(error.error)
        }
    })?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<()> {
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<()> {
    // Directory fsync is not available through portable std on Windows.
    Ok(())
}

pub fn max_limits(mebibytes: u8) -> Result<BackupLimits> {
    let bytes = usize::from(mebibytes) * 1024 * 1024;
    if bytes == 0 || bytes > HARD_MAX_PLAINTEXT {
        return Err(CliError::SizeLimit);
    }
    Ok(BackupLimits::new(bytes)?)
}
