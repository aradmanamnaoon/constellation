use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use constellation_backup::{BackupCodec, BackupLimits};
use constellation_backup_cli::{BackupService, CliError};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const SECRET: &[u8] = b"very-long-fixture-secret-2026";

fn test_db(path: &Path) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE samples(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         INSERT INTO samples(name) VALUES ('before-wal');",
    )
    .unwrap();
    db
}

fn verify_rows(path: &Path) -> Vec<String> {
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut query = db.prepare("SELECT name FROM samples ORDER BY id").unwrap();
    query
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn consistent_backup_includes_committed_rows_still_in_active_wal() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("active.sqlite3");
    let package = dir.path().join("wal.cbackup");
    let restored = dir.path().join("restored.sqlite3");
    let db = test_db(&source);
    let journal: String = db
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(journal.to_lowercase(), "delete");
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA wal_autocheckpoint=0;
         INSERT INTO samples(name) VALUES ('committed-in-wal');
         INSERT INTO samples(name) VALUES ('another-in-wal');",
    )
    .unwrap();
    let wal_path = dir.path().join("active.sqlite3-wal");
    assert!(
        fs::metadata(&wal_path).unwrap().len() > 0,
        "active WAL must be nonempty"
    );
    let original_wal_len = fs::metadata(&wal_path).unwrap().len();
    let service = BackupService::new(BackupLimits::default());

    service.backup_database(&source, &package, SECRET).unwrap();
    assert!(fs::metadata(&wal_path).unwrap().len() >= original_wal_len);
    assert_eq!(verify_rows(&source).len(), 3);
    service
        .restore_database(&package, &restored, SECRET)
        .unwrap();
    assert_eq!(
        verify_rows(&restored),
        vec!["before-wal", "committed-in-wal", "another-in-wal",]
    );
    let reopened = Connection::open(&restored).unwrap();
    let integrity: String = reopened
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(verify_rows(&source), verify_rows(&restored));
    for entry in fs::read_dir(dir.path()).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            !name.to_string_lossy().starts_with(".constellation-"),
            "restore or backup leaked a staging file"
        );
    }
}

#[test]
fn wrong_passphrase_and_corruption_never_create_a_restored_database() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.sqlite3");
    let package = dir.path().join("encrypted.cbackup");
    let wrong_target = dir.path().join("wrong.sqlite3");
    let corrupt_target = dir.path().join("corrupt.sqlite3");
    let db = test_db(&source);
    drop(db);
    let service = BackupService::new(BackupLimits::default());
    service.backup_database(&source, &package, SECRET).unwrap();

    assert!(matches!(
        service.restore_database(&package, &wrong_target, b"wrong-but-long-passphrase"),
        Err(CliError::Package(
            constellation_backup::BackupError::AuthenticationFailed
        ))
    ));
    assert!(!wrong_target.exists());

    let mut damaged = fs::read(&package).unwrap();
    damaged[64] ^= 0x80;
    let end = damaged.len() - 32;
    let hash = Sha256::digest(&damaged[..end]);
    damaged[end..].copy_from_slice(&hash);
    let mutated_package = dir.path().join("corrupt.cbackup");
    fs::write(mutated_package.as_path(), damaged).unwrap();
    assert!(matches!(
        service.restore_database(mutated_package.as_path(), &corrupt_target, SECRET),
        Err(CliError::Package(
            constellation_backup::BackupError::AuthenticationFailed
        ))
    ));
    assert!(!corrupt_target.exists());
}

#[test]
fn authenticated_non_sqlite_payload_is_rejected_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("forged.cbackup");
    let restored = dir.path().join("invalid.sqlite3");
    let bytes = BackupCodec::default()
        .encrypt(b"not a valid sqlite image", SECRET)
        .unwrap();
    fs::write(&package, bytes).unwrap();
    let service = BackupService::new(BackupLimits::default());
    assert!(matches!(
        service.restore_database(&package, &restored, SECRET),
        Err(CliError::InvalidDatabase)
    ));
    assert!(!restored.exists());
}

#[test]
fn authenticated_sqlite_with_corrupted_btree_is_rejected_before_output() {
    let dir = tempfile::tempdir().unwrap();
    let valid_db = dir.path().join("source.sqlite3");
    drop(test_db(&valid_db));
    let mut damaged = fs::read(&valid_db).unwrap();
    assert!(damaged.len() > 120);
    damaged[100..110].fill(0xff); // Corrupt the database's first b-tree page.
    let encrypted = BackupCodec::default().encrypt(&damaged, SECRET).unwrap();
    let package = dir.path().join("corrupt-page.cbackup");
    let target = dir.path().join("never-created.sqlite3");
    fs::write(&package, encrypted).unwrap();
    let service = BackupService::new(BackupLimits::default());
    assert!(service.restore_database(&package, &target, SECRET).is_err());
    assert!(!target.exists());
}

#[test]
fn existing_output_is_never_overwritten_for_backup_or_restore() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.sqlite3");
    let package = dir.path().join("encrypted.cbackup");
    let restored = dir.path().join("existing.sqlite3");
    let db = test_db(&source);
    drop(db);
    let service = BackupService::new(BackupLimits::default());

    fs::write(&package, b"preserve this package").unwrap();
    assert!(matches!(
        service.backup_database(&source, &package, SECRET),
        Err(CliError::OutputExists)
    ));
    assert_eq!(fs::read(&package).unwrap(), b"preserve this package");

    fs::remove_file(&package).unwrap();
    service.backup_database(&source, &package, SECRET).unwrap();
    fs::write(&restored, b"preserve this database").unwrap();
    assert!(matches!(
        service.restore_database(&package, &restored, SECRET),
        Err(CliError::OutputExists)
    ));
    assert_eq!(fs::read(&restored).unwrap(), b"preserve this database");
}

#[test]
fn malformed_db_and_tight_size_limit_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = dir.path().join("bad.sqlite3");
    let package = dir.path().join("bad.cbackup");
    fs::write(&invalid, b"not sqlite data").unwrap();
    let service = BackupService::new(BackupLimits::default());
    assert!(service.backup_database(&invalid, &package, SECRET).is_err());
    assert!(!package.exists());

    let db = dir.path().join("valid.sqlite3");
    let connection = test_db(&db);
    connection
        .execute_batch("INSERT INTO samples(name) VALUES (hex(randomblob(2000000)));")
        .unwrap();
    drop(connection);
    let tight = BackupService::new(BackupLimits::new(1024 * 1024).unwrap());
    assert!(matches!(
        tight.backup_database(&db, &package, SECRET),
        Err(CliError::SizeLimit)
    ));
    assert!(!package.exists());
}

#[test]
fn restore_and_backup_refuse_missing_parent_or_missing_source() {
    let dir = tempfile::tempdir().unwrap();
    let service = BackupService::new(BackupLimits::default());
    let missing = dir.path().join("missing.sqlite3");
    let target = dir.path().join("new.cbackup");
    assert!(matches!(
        service.backup_database(&missing, &target, SECRET),
        Err(CliError::InvalidSource)
    ));
    let src = dir.path().join("valid.sqlite3");
    drop(test_db(&src));
    assert!(matches!(
        service.backup_database(&src, dir.path().join("absent").join("out.cbackup"), SECRET),
        Err(CliError::MissingParent)
    ));
}

#[test]
fn restore_refuses_stale_sqlite_sidecars_without_creating_database() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.sqlite3");
    let package = dir.path().join("snapshot.cbackup");
    drop(test_db(&source));
    let service = BackupService::new(BackupLimits::default());
    service.backup_database(&source, &package, SECRET).unwrap();

    for suffix in ["-wal", "-shm", "-journal"] {
        let output = dir.path().join(format!("recover-{suffix}.sqlite3"));
        let mut sidecar_name = output.as_os_str().to_os_string();
        sidecar_name.push(suffix);
        let sidecar = std::path::PathBuf::from(sidecar_name);
        fs::write(&sidecar, b"existing sidecar; preserve me").unwrap();
        assert!(matches!(
            service.restore_database(&package, &output, SECRET),
            Err(CliError::ExistingSidecar)
        ));
        assert!(!output.exists());
        assert_eq!(
            fs::read(&sidecar).unwrap(),
            b"existing sidecar; preserve me"
        );
        fs::remove_file(sidecar).unwrap();
    }
    assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".constellation-")
    }));
}

#[test]
fn backup_and_restore_reject_symlink_sources_and_targets() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.sqlite3");
    let package = dir.path().join("snapshot.cbackup");
    drop(test_db(&source));
    let service = BackupService::new(BackupLimits::default());
    service.backup_database(&source, &package, SECRET).unwrap();

    #[cfg(unix)]
    fn link_file(original: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    fn link_file(original: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(original, link)
    }

    let linked_source = dir.path().join("linked.sqlite3");
    if link_file(&source, &linked_source).is_err() {
        // Creating symlinks requires Developer Mode or privileges on Windows.
        return;
    }
    let output = dir.path().join("backup-new.cbackup");
    assert!(matches!(
        service.backup_database(&linked_source, &output, SECRET),
        Err(CliError::InvalidSource)
    ));
    assert!(!output.exists());

    let linked_package = dir.path().join("linked.cbackup");
    link_file(&package, &linked_package).unwrap();
    let restored = dir.path().join("restored.sqlite3");
    assert!(matches!(
        service.restore_database(&linked_package, &restored, SECRET),
        Err(CliError::InvalidSource)
    ));
    assert!(!restored.exists());

    let target_link = dir.path().join("target-link.sqlite3");
    link_file(&restored, &target_link).unwrap(); // dangling link also occupies output
    assert!(matches!(
        service.restore_database(&package, &target_link, SECRET),
        Err(CliError::OutputExists)
    ));
}

#[test]
fn backup_and_restore_resolve_parent_symlinks_to_same_directory() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    let source = real.join("source.sqlite3");
    drop(test_db(&source));

    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&real, dir.path().join("alias"));
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(&real, dir.path().join("alias"));
    if result.is_err() {
        return;
    }
    let alias = dir.path().join("alias");
    let service = BackupService::new(BackupLimits::default());
    service
        .backup_database(&source, alias.join("backup.cbackup"), SECRET)
        .unwrap();
    service
        .restore_database(
            real.join("backup.cbackup"),
            alias.join("recovered.sqlite3"),
            SECRET,
        )
        .unwrap();
    assert_eq!(
        verify_rows(&real.join("recovered.sqlite3")),
        vec!["before-wal"]
    );
}

#[test]
fn executable_never_accepts_password_args_or_non_tty_passphrase_input() {
    let bin = env!("CARGO_BIN_EXE_constellation-backup-cli");
    let help = Command::new(bin).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("backup <source.sqlite3>"));
    assert!(help_text.contains("without echo"));

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("valid.sqlite3");
    let target = dir.path().join("new.cbackup");
    drop(test_db(&source));
    let bad_arg = Command::new(bin)
        .arg("backup")
        .arg(&source)
        .arg(&target)
        .arg("--password")
        .arg("this-is-a-disallowed-secret")
        .output()
        .unwrap();
    assert!(!bad_arg.status.success());
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&bad_arg.stderr).contains("this-is-a-disallowed-secret"));

    let no_tty = Command::new(bin)
        .arg("backup")
        .arg(&source)
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!no_tty.status.success());
    assert!(String::from_utf8_lossy(&no_tty.stderr).contains("interactive terminal required"));
    assert!(!target.exists());
}
