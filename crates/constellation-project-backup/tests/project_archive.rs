use std::{fs, path::Path, time::Duration};

use constellation_backup::{BackupCodec, BackupLimits};
use constellation_project_backup::{
    ProjectBackupError, ProjectBackupService, ARCHIVE_VERSION, MAX_PLAINTEXT_BYTES, STUDIO_NAME,
    WORKFLOWS_NAME,
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const PASS: &[u8] = b"strong-integration-test-passphrase";

/// A caller of ProjectBackupService must supply component-safe, canonical
/// paths. On macOS the system temporary directory can be exposed via the
/// /var -> /private/var symlink; passing that alias to the security validator
/// would correctly be rejected before reaching the test's actual assertion.
/// Create test fixtures beneath the canonical OS temp root instead of
/// weakening production symlink containment or skipping negative cases.
fn safe_test_tempdir() -> TempDir {
    #[cfg(target_os = "macos")]
    {
        let canonical_temp_root = fs::canonicalize(std::env::temp_dir())
            .expect("system temporary directory must have a canonical identity");
        tempfile::Builder::new()
            .prefix("constellation-project-archive-")
            .tempdir_in(&canonical_temp_root)
            .expect("create isolated directory below canonical temporary root")
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows extended-length paths returned by fs::canonicalize can
        // normalize away "/sub/../" during PathBuf::join, accidentally
        // invalidating the adversarial lexical-parent-traversal test.
        TempDir::new().expect("create isolated project-archive test directory")
    }
}

fn new_wal_database(path: &Path, table: &str, value: &str) -> Connection {
    let db = Connection::open(path).unwrap();
    let journal: String = db
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    db.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
    db.execute(&format!("CREATE TABLE {table}(value TEXT NOT NULL)"), [])
        .unwrap();
    db.execute(&format!("INSERT INTO {table}(value) VALUES (?1)"), [value])
        .unwrap();
    db
}

fn fixture(dir: &TempDir) -> (Connection, Connection) {
    let studio = new_wal_database(
        &dir.path().join("primary.db"),
        "notes",
        "committed studio WAL",
    );
    let flows = new_wal_database(
        &dir.path().join("execution.db"),
        "workflow_runs",
        "committed workflows WAL",
    );
    assert!(dir.path().join("primary.db-wal").metadata().unwrap().len() > 0);
    assert!(
        dir.path()
            .join("execution.db-wal")
            .metadata()
            .unwrap()
            .len()
            > 0
    );
    (studio, flows)
}

fn save_package(dir: &TempDir) -> (Connection, Connection) {
    let (studio, flows) = fixture(dir);
    let info = ProjectBackupService
        .create_archive(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            dir.path().join("both.cbackup"),
            PASS,
        )
        .unwrap();
    assert_eq!(info.version, ARCHIVE_VERSION);
    assert!(info.studio_bytes >= 100);
    assert!(info.workflows_bytes >= 100);
    (studio, flows)
}

fn query_value(db: &Path, table: &str) -> String {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    conn.query_row(&format!("SELECT value FROM {table}"), [], |row| row.get(0))
        .unwrap()
}

fn matching_generation(
    studio: &Connection,
    workflows: &Connection,
) -> constellation_project_backup::Result<()> {
    let studio_generation: i64 =
        studio.query_row("SELECT generation FROM backup_generation", [], |row| {
            row.get(0)
        })?;
    let workflows_generation: i64 =
        workflows.query_row("SELECT generation FROM backup_generation", [], |row| {
            row.get(0)
        })?;
    if studio_generation != workflows_generation {
        return Err(ProjectBackupError::PairMismatch);
    }
    Ok(())
}

#[test]
fn pair_validator_fails_closed_on_divergent_images_then_checks_verified_restore() {
    let dir = safe_test_tempdir();
    let (studio, workflows) = fixture(&dir);
    studio
        .execute_batch("CREATE TABLE backup_generation(generation INTEGER NOT NULL); INSERT INTO backup_generation VALUES(7)")
        .unwrap();
    workflows
        .execute_batch("CREATE TABLE backup_generation(generation INTEGER NOT NULL); INSERT INTO backup_generation VALUES(6)")
        .unwrap();
    let backup = dir.path().join("checked.cbackup");
    assert!(matches!(
        ProjectBackupService.create_archive_with_pair_validator(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &backup,
            PASS,
            &matching_generation,
        ),
        Err(ProjectBackupError::PairMismatch)
    ));
    assert!(
        !backup.exists(),
        "mismatched snapshots became a published archive"
    );

    // A legacy/unvalidated archive may contain an inconsistent pair: the
    // checked verify and restore paths still refuse it before writing files.
    let unchecked = dir.path().join("unchecked.cbackup");
    ProjectBackupService
        .create_archive(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &unchecked,
            PASS,
        )
        .unwrap();
    assert!(matches!(
        ProjectBackupService.verify_archive_with_pair_validator(
            &unchecked,
            PASS,
            &matching_generation,
        ),
        Err(ProjectBackupError::PairMismatch)
    ));
    let rejected = dir.path().join("inconsistent-restore");
    assert!(matches!(
        ProjectBackupService.restore_archive_with_pair_validator(
            &unchecked,
            &rejected,
            PASS,
            &matching_generation,
        ),
        Err(ProjectBackupError::PairMismatch)
    ));
    assert!(!rejected.exists());

    workflows
        .execute("UPDATE backup_generation SET generation = 7", [])
        .unwrap();
    let info = ProjectBackupService
        .create_archive_with_pair_validator(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &backup,
            PASS,
            &matching_generation,
        )
        .unwrap();
    assert_eq!(
        ProjectBackupService
            .verify_archive_with_pair_validator(&backup, PASS, &matching_generation)
            .unwrap(),
        info
    );
    // Later live writes cannot change the immutable archived pair.
    studio
        .execute("UPDATE backup_generation SET generation = 99", [])
        .unwrap();
    let restored = ProjectBackupService
        .restore_archive_with_pair_validator(
            &backup,
            dir.path().join("consistent-restore"),
            PASS,
            &matching_generation,
        )
        .unwrap();
    assert_eq!(fs::read_dir(&restored.directory).unwrap().count(), 2);
    assert_eq!(
        query_value(&restored.studio, "notes"),
        "committed studio WAL"
    );
    assert_eq!(
        query_value(&restored.workflows, "workflow_runs"),
        "committed workflows WAL"
    );
}

#[test]
fn pair_validator_sees_query_only_database_images() {
    let dir = safe_test_tempdir();
    let (_studio, _workflows) = fixture(&dir);
    let archive = dir.path().join("read-only.cbackup");
    ProjectBackupService
        .create_archive_with_pair_validator(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &archive,
            PASS,
            &|studio, workflows| {
                assert!(studio
                    .execute("INSERT INTO notes(value) VALUES ('not allowed')", [])
                    .is_err());
                assert!(workflows
                    .execute(
                        "INSERT INTO workflow_runs(value) VALUES ('not allowed')",
                        []
                    )
                    .is_err());
                Ok(())
            },
        )
        .unwrap();
    assert!(archive.is_file());
}

#[test]
fn coordinated_capture_preserves_matching_live_wal_pair_and_archive_format() {
    let dir = safe_test_tempdir();
    let (studio, workflows) = fixture(&dir);
    studio
        .execute_batch(
            "CREATE TABLE backup_generation(generation INTEGER NOT NULL);
             INSERT INTO backup_generation VALUES(42)",
        )
        .unwrap();
    workflows
        .execute_batch(
            "CREATE TABLE backup_generation(generation INTEGER NOT NULL);
             INSERT INTO backup_generation VALUES(42)",
        )
        .unwrap();
    let package = dir.path().join("coordinated.cbackup");
    let info = ProjectBackupService
        .create_archive_coordinated_with_pair_validator(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &package,
            PASS,
            Duration::from_secs(10),
            &matching_generation,
        )
        .unwrap();
    assert_eq!(info.version, ARCHIVE_VERSION);
    assert_eq!(
        ProjectBackupService
            .verify_archive_with_pair_validator(&package, PASS, &matching_generation)
            .unwrap(),
        info
    );
    let restored = ProjectBackupService
        .restore_archive_with_pair_validator(
            &package,
            dir.path().join("recovered-coordinated"),
            PASS,
            &matching_generation,
        )
        .unwrap();
    assert_eq!(
        query_value(&restored.studio, "notes"),
        "committed studio WAL"
    );
    assert_eq!(
        query_value(&restored.workflows, "workflow_runs"),
        "committed workflows WAL"
    );
    // Fences are short-lived: both independently owned source connections
    // remain writable when the offline encryption has finished.
    studio
        .execute("UPDATE backup_generation SET generation=43", [])
        .unwrap();
    workflows
        .execute("UPDATE backup_generation SET generation=43", [])
        .unwrap();

    // Coordination fences writers during capture but must still run the
    // application's cross-DB check. A previously divergent pair is *not*
    // repaired by holding two locks: fail before publishing any ciphertext.
    workflows
        .execute("UPDATE backup_generation SET generation=44", [])
        .unwrap();
    let mismatch_package = dir.path().join("fenced-mismatch.cbackup");
    assert!(matches!(
        ProjectBackupService.create_archive_coordinated_with_pair_validator(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &mismatch_package,
            PASS,
            Duration::from_secs(10),
            &matching_generation,
        ),
        Err(ProjectBackupError::PairMismatch)
    ));
    assert!(!mismatch_package.exists());
    studio
        .execute("UPDATE backup_generation SET generation=44", [])
        .unwrap();
}

#[test]
fn coordinated_capture_busy_second_writer_fence_times_out_without_publication() {
    let dir = safe_test_tempdir();
    // The canonical lexicographic order is a then z; second must not block
    // indefinitely while the first reservation is held.
    let first = dir.path().join("a-studio.db");
    let second = dir.path().join("z-workflows.db");
    let (studio, workflows) = (
        new_wal_database(&first, "notes", "first WAL"),
        new_wal_database(&second, "workflow_runs", "second WAL"),
    );
    let blocking = Connection::open(&second).unwrap();
    blocking.execute_batch("BEGIN IMMEDIATE").unwrap();
    let output = dir.path().join("should-not-publish.cbackup");
    assert!(matches!(
        ProjectBackupService.create_archive_coordinated(
            &first,
            &second,
            &output,
            PASS,
            Duration::from_millis(120),
        ),
        Err(ProjectBackupError::SnapshotTimeout)
    ));
    assert!(!output.exists(), "failure published a partial archive");
    // Failed acquisition must release the earlier source reservation.
    studio
        .execute("INSERT INTO notes(value) VALUES('after failure')", [])
        .unwrap();
    blocking.execute_batch("ROLLBACK").unwrap();
    workflows
        .execute(
            "INSERT INTO workflow_runs(value) VALUES('after failure')",
            [],
        )
        .unwrap();

    // Invalid deadlines and mismatched pairs also fail before publication.
    for timeout in [Duration::ZERO, Duration::from_secs(31)] {
        assert!(matches!(
            ProjectBackupService
                .create_archive_coordinated(&first, &second, &output, PASS, timeout,),
            Err(ProjectBackupError::SnapshotTimeout)
        ));
        assert!(!output.exists());
    }
}

#[test]
fn authenticated_sqlite_images_reject_extra_bytes_or_false_page_counts() {
    let dir = safe_test_tempdir();
    let (_studio, _workflows) = save_package(&dir);
    let codec = BackupCodec::new(BackupLimits::new(MAX_PLAINTEXT_BYTES).unwrap());
    let package = fs::read(dir.path().join("both.cbackup")).unwrap();
    let valid = codec.decrypt(&package, PASS).unwrap();
    let studio_len = u64::from_le_bytes(valid[16..24].try_into().unwrap()) as usize;
    for (case, change) in ["appended-studio", "appended-workflows", "forged-page-count"]
        .iter()
        .enumerate()
    {
        let mut plaintext = valid.clone();
        match case {
            0 => {
                plaintext.insert(96 + studio_len, 0x5A);
                plaintext[16..24].copy_from_slice(&((studio_len + 1) as u64).to_le_bytes());
                let digest = Sha256::digest(&plaintext[96..96 + studio_len + 1]);
                plaintext[32..64].copy_from_slice(&digest);
            }
            1 => {
                plaintext.push(0x5A);
                let workflows_len = u64::from_le_bytes(plaintext[24..32].try_into().unwrap());
                plaintext[24..32].copy_from_slice(&(workflows_len + 1).to_le_bytes());
                let digest = Sha256::digest(&plaintext[96 + studio_len..]);
                plaintext[64..96].copy_from_slice(&digest);
            }
            _ => {
                plaintext[96 + 28..96 + 32].copy_from_slice(&0_u32.to_be_bytes());
                let digest = Sha256::digest(&plaintext[96..96 + studio_len]);
                plaintext[32..64].copy_from_slice(&digest);
            }
        }
        let path = dir.path().join(format!("{change}.cbackup"));
        fs::write(&path, codec.encrypt(&plaintext, PASS).unwrap()).unwrap();
        assert!(
            matches!(
                ProjectBackupService.verify_archive(&path, PASS),
                Err(ProjectBackupError::InvalidDatabase)
            ),
            "accepted forged SQLite image {change}"
        );
        let restore = dir.path().join(format!("{change}-restore"));
        assert!(matches!(
            ProjectBackupService.restore_archive(&path, &restore, PASS),
            Err(ProjectBackupError::InvalidDatabase)
        ));
        assert!(!restore.exists());
    }
}

#[test]
fn active_wals_in_both_databases_restore_from_one_encrypted_file() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let package = dir.path().join("both.cbackup");
    let raw = fs::read(&package).unwrap();
    assert!(!raw
        .windows(b"committed studio WAL".len())
        .any(|part| part == b"committed studio WAL"));
    assert!(!raw
        .windows(b"committed workflows WAL".len())
        .any(|part| part == b"committed workflows WAL"));
    let info = ProjectBackupService.verify_archive(&package, PASS).unwrap();
    assert_eq!(info.version, 1);
    let recovered = ProjectBackupService
        .restore_archive(&package, dir.path().join("recovered"), PASS)
        .unwrap();
    assert_eq!(recovered.studio, recovered.directory.join(STUDIO_NAME));
    assert_eq!(
        recovered.workflows,
        recovered.directory.join(WORKFLOWS_NAME)
    );
    assert_eq!(
        query_value(&recovered.studio, "notes"),
        "committed studio WAL"
    );
    assert_eq!(
        query_value(&recovered.workflows, "workflow_runs"),
        "committed workflows WAL"
    );
    assert_eq!(fs::read_dir(&recovered.directory).unwrap().count(), 2);
    assert!(!recovered
        .directory
        .join(format!("{STUDIO_NAME}-wal"))
        .exists());
    assert!(!recovered
        .directory
        .join(format!("{WORKFLOWS_NAME}-wal"))
        .exists());
}

#[test]
fn wrong_passphrase_or_tampered_ciphertext_cannot_publish_restore() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let source = dir.path().join("both.cbackup");
    assert!(ProjectBackupService
        .restore_archive(&source, dir.path().join("wrong"), b"wrong-long-passphrase")
        .is_err());
    assert!(!dir.path().join("wrong").exists());
    let mut tampered = fs::read(&source).unwrap();
    let middle = tampered.len() / 2;
    tampered[middle] ^= 0x40;
    let altered = dir.path().join("altered.cbackup");
    fs::write(&altered, tampered).unwrap();
    assert!(ProjectBackupService
        .restore_archive(&altered, dir.path().join("corrupt"), PASS)
        .is_err());
    assert!(!dir.path().join("corrupt").exists());
}

#[test]
fn authenticated_corrupt_lengths_hashes_and_version_fail_before_creating_directory() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let codec = BackupCodec::new(BackupLimits::new(MAX_PLAINTEXT_BYTES).unwrap());
    let package = fs::read(dir.path().join("both.cbackup")).unwrap();
    let valid = codec.decrypt(&package, PASS).unwrap();
    for (name, change) in [
        ("bad-length", 16_usize),
        ("bad-hash", 32),
        ("bad-version", 8),
        ("bad-count", 10),
        ("bad-reserved", 12),
    ] {
        let mut plaintext = valid.clone();
        plaintext[change] ^= 1;
        let signed_invalid = codec.encrypt(&plaintext, PASS).unwrap();
        let file = dir.path().join(format!("{name}.cbackup"));
        fs::write(&file, signed_invalid).unwrap();
        assert!(matches!(
            ProjectBackupService.verify_archive(&file, PASS),
            Err(ProjectBackupError::InvalidArchive)
        ));
        let dest = dir.path().join(format!("restore-{name}"));
        assert!(matches!(
            ProjectBackupService.restore_archive(&file, &dest, PASS),
            Err(ProjectBackupError::InvalidArchive)
        ));
        assert!(!dest.exists());
    }
}

#[test]
fn backup_and_restore_never_clobber_existing_paths_or_partial_crash_directories() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let source = dir.path().join("both.cbackup");
    let original = fs::read(&source).unwrap();
    assert!(matches!(
        ProjectBackupService.create_archive(
            dir.path().join("primary.db"),
            dir.path().join("execution.db"),
            &source,
            PASS
        ),
        Err(ProjectBackupError::OutputExists)
    ));
    assert_eq!(fs::read(&source).unwrap(), original);
    // Simulate a previous interrupted restore directory: subsequent attempts
    // must refuse the occupied path and preserve existing user data.
    let previous = dir.path().join("interrupted-restore");
    fs::create_dir(&previous).unwrap();
    fs::write(previous.join("user-file.txt"), b"untouchable").unwrap();
    assert!(matches!(
        ProjectBackupService.restore_archive(&source, &previous, PASS),
        Err(ProjectBackupError::OutputExists)
    ));
    assert_eq!(
        fs::read(previous.join("user-file.txt")).unwrap(),
        b"untouchable"
    );
    let after = dir.path().join("fresh");
    ProjectBackupService
        .restore_archive(&source, &after, PASS)
        .unwrap();
    assert!(matches!(
        ProjectBackupService.restore_archive(&source, &after, PASS),
        Err(ProjectBackupError::OutputExists)
    ));
}

#[test]
fn distinct_sources_and_parent_traversal_are_mandatory() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = fixture(&dir);
    let same = dir.path().join("primary.db");
    assert!(matches!(
        ProjectBackupService.create_archive(&same, &same, dir.path().join("bad.cbackup"), PASS),
        Err(ProjectBackupError::DuplicateSource)
    ));
    let traversal = dir.path().join("sub").join("..").join("bad.cbackup");
    let result = ProjectBackupService.create_archive(
        &same,
        dir.path().join("execution.db"),
        &traversal,
        PASS,
    );
    assert!(
        matches!(result, Err(ProjectBackupError::UnsafePath)),
        "parent traversal must be rejected before any IO: {result:?}; path {traversal:?}"
    );
    assert!(!dir.path().join("bad.cbackup").exists());
}

#[test]
fn orphan_staging_from_interrupted_process_never_becomes_the_restore() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    // Model a process terminated between writing the first SQLite image and
    // publishing the destination: an orphan staging directory may survive.
    let orphan = dir.path().join(".constellation-restore-interrupted");
    fs::create_dir(&orphan).unwrap();
    fs::write(orphan.join(STUDIO_NAME), b"incomplete private staging").unwrap();
    let target = dir.path().join("recovered-after-interruption");
    let recovered = ProjectBackupService
        .restore_archive(dir.path().join("both.cbackup"), &target, PASS)
        .unwrap();
    assert_eq!(
        query_value(&recovered.studio, "notes"),
        "committed studio WAL"
    );
    assert_eq!(
        query_value(&recovered.workflows, "workflow_runs"),
        "committed workflows WAL"
    );
    assert_eq!(fs::read_dir(&target).unwrap().count(), 2);
    // Never remove the crash residue automatically: it might contain
    // user-owned files or be under separate recovery investigation.
    assert_eq!(
        fs::read(orphan.join(STUDIO_NAME)).unwrap(),
        b"incomplete private staging"
    );
    let temporary_count = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".constellation-restore-")
        })
        .count();
    assert_eq!(
        temporary_count, 1,
        "a successful restore left staging behind"
    );
}

#[test]
fn authenticated_invalid_sqlite_bytes_fail_before_publication() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let codec = BackupCodec::new(BackupLimits::new(MAX_PLAINTEXT_BYTES).unwrap());
    let package = fs::read(dir.path().join("both.cbackup")).unwrap();
    let mut plaintext = codec.decrypt(&package, PASS).unwrap();
    let studio_len = u64::from_le_bytes(plaintext[16..24].try_into().unwrap()) as usize;
    // Recompute both crypto authentication and metadata digest, so validation
    // reaches the SQLite header instead of stopping at the checksum layer.
    plaintext[96 + 18] = 2;
    let digest = Sha256::digest(&plaintext[96..96 + studio_len]);
    plaintext[32..64].copy_from_slice(&digest);
    let bad_file = dir.path().join("bad-sqlite.cbackup");
    fs::write(&bad_file, codec.encrypt(&plaintext, PASS).unwrap()).unwrap();
    assert!(matches!(
        ProjectBackupService.verify_archive(&bad_file, PASS),
        Err(ProjectBackupError::InvalidDatabase)
    ));
    let dest = dir.path().join("not-restored");
    assert!(matches!(
        ProjectBackupService.restore_archive(&bad_file, &dest, PASS),
        Err(ProjectBackupError::InvalidDatabase)
    ));
    assert!(!dest.exists());
}

#[cfg(unix)]
#[test]
fn symlinked_source_and_destinations_are_denied() {
    use std::os::unix::fs::symlink;
    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let link = dir.path().join("linked.db");
    symlink(dir.path().join("primary.db"), &link).unwrap();
    assert!(matches!(
        ProjectBackupService.create_archive(
            &link,
            dir.path().join("execution.db"),
            dir.path().join("new.cbackup"),
            PASS
        ),
        Err(ProjectBackupError::UnsafePath)
    ));
    let link_parent = dir.path().join("linked-parent");
    symlink(dir.path(), &link_parent).unwrap();
    assert!(matches!(
        ProjectBackupService.restore_archive(
            dir.path().join("both.cbackup"),
            link_parent.join("restored"),
            PASS
        ),
        Err(ProjectBackupError::UnsafePath)
    ));
}

#[cfg(windows)]
#[test]
fn canonical_verbatim_paths_roundtrip_live_wals_and_still_refuse_clobber() {
    let dir = safe_test_tempdir();
    let (_studio, _flows) = fixture(&dir);
    let canonical = fs::canonicalize(dir.path()).unwrap();
    assert!(
        canonical.to_string_lossy().starts_with(r"\\?\"),
        "Windows canonicalization must exercise an extended-length path"
    );
    let package = canonical.join("verbatim-path-project.cstlproj");
    let info = ProjectBackupService
        .create_archive(
            canonical.join("primary.db"),
            canonical.join("execution.db"),
            &package,
            PASS,
        )
        .unwrap();
    assert_eq!(info.version, ARCHIVE_VERSION);
    assert!(info.studio_bytes > 0 && info.workflows_bytes > 0);
    assert_eq!(
        ProjectBackupService.verify_archive(&package, PASS).unwrap(),
        info
    );

    let original = fs::read(&package).unwrap();
    assert!(matches!(
        ProjectBackupService.create_archive(
            canonical.join("primary.db"),
            canonical.join("execution.db"),
            &package,
            PASS,
        ),
        Err(ProjectBackupError::OutputExists)
    ));
    assert_eq!(fs::read(&package).unwrap(), original);

    let wrong = canonical.join("verbatim-wrong-pass");
    assert!(ProjectBackupService
        .restore_archive(&package, &wrong, b"different-long-test-passphrase")
        .is_err());
    assert!(!wrong.exists());

    let destination = canonical.join("verbatim-restored-project");
    let recovered = ProjectBackupService
        .restore_archive(&package, &destination, PASS)
        .unwrap();
    assert_eq!(
        query_value(&recovered.studio, "notes"),
        "committed studio WAL"
    );
    assert_eq!(
        query_value(&recovered.workflows, "workflow_runs"),
        "committed workflows WAL"
    );
    assert!(matches!(
        ProjectBackupService.restore_archive(&package, &destination, PASS),
        Err(ProjectBackupError::OutputExists)
    ));
    assert_eq!(fs::read(&package).unwrap(), original);
}

#[cfg(windows)]
#[test]
fn canonical_verbatim_paths_still_reject_junction_components() {
    use std::os::windows::process::CommandExt;

    let dir = safe_test_tempdir();
    let (_studio, _flows) = save_package(&dir);
    let canonical = fs::canonicalize(dir.path()).unwrap();
    let junction = dir.path().join("junction-to-fixture");
    // A directory junction does not require Windows developer-mode symlink
    // privileges; it is a real reparse point, not a simulated path alias.
    // Prevent cmd.exe from opening a console on the active desktop.
    let output = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(dir.path())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .unwrap();
    assert!(output.status.success(), "could not create test junction");

    assert!(matches!(
        ProjectBackupService.create_archive(
            junction.join("primary.db"),
            canonical.join("execution.db"),
            canonical.join("should-not-exist.cstlproj"),
            PASS,
        ),
        Err(ProjectBackupError::UnsafePath)
    ));
    assert!(matches!(
        ProjectBackupService.restore_archive(
            canonical.join("both.cbackup"),
            junction.join("restored-via-junction"),
            PASS,
        ),
        Err(ProjectBackupError::UnsafePath)
    ));
    assert!(!dir.path().join("should-not-exist.cstlproj").exists());
    assert!(!dir.path().join("restored-via-junction").exists());
}
