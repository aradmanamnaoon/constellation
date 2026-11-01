use std::{fs, io::Write, path::Path};

use constellation_backup::{
    BackupCodec, BackupError, BackupLimits, FORMAT_VERSION, HARD_MAX_PLAINTEXT, MAGIC,
};
use sha2::{Digest, Sha256};

const PASSWORD: &[u8] = b"correct horse battery staple";

fn checksum_rewrite(package: &mut [u8]) {
    let end = package.len() - 32;
    let hash = Sha256::digest(&package[..end]);
    package[end..].copy_from_slice(&hash);
}

#[test]
fn binary_roundtrip_and_portable_versioned_format() {
    let codec = BackupCodec::default();
    let input = [0, 1, 0xff, 0, 42, 0x80, 0x21];
    let package = codec.encrypt(&input, PASSWORD).unwrap();
    assert_eq!(&package[..MAGIC.len()], &MAGIC);
    assert_eq!(
        u16::from_le_bytes(package[8..10].try_into().unwrap()),
        FORMAT_VERSION
    );
    assert_eq!(codec.inspect(&package).unwrap().plaintext_len, input.len());
    assert_eq!(codec.inspect(&package).unwrap().package_len, package.len());
    assert_eq!(codec.decrypt(&package, PASSWORD).unwrap().as_slice(), input);
    assert_eq!(package.len(), 140 + input.len());
    assert!(!package.windows(input.len()).any(|window| window == input));
}

#[test]
fn empty_payload_roundtrip_is_supported() {
    let codec = BackupCodec::default();
    let ciphertext = codec.encrypt(b"", PASSWORD).unwrap();
    assert_eq!(ciphertext.len(), 140);
    assert!(codec.decrypt(&ciphertext, PASSWORD).unwrap().is_empty());
}

#[test]
fn salt_nonce_and_ciphertext_are_randomized() {
    let codec = BackupCodec::default();
    let a = codec.encrypt(b"snapshot", PASSWORD).unwrap();
    let b = codec.encrypt(b"snapshot", PASSWORD).unwrap();
    assert_ne!(&a[32..48], &b[32..48], "salt must differ");
    assert_ne!(&a[48..60], &b[48..60], "nonce must differ");
    assert_ne!(a, b);
    assert_eq!(*codec.decrypt(&a, PASSWORD).unwrap(), b"snapshot".to_vec());
    assert_eq!(*codec.decrypt(&b, PASSWORD).unwrap(), b"snapshot".to_vec());
}

#[test]
fn wrong_passphrase_rejected() {
    let codec = BackupCodec::default();
    let package = codec.encrypt(b"private research", PASSWORD).unwrap();
    assert!(matches!(
        codec.decrypt(&package, b"definitely the wrong passphrase"),
        Err(BackupError::AuthenticationFailed)
    ));
}

#[test]
fn ciphertext_corruption_rejected_even_after_attacker_recomputes_checksum() {
    let codec = BackupCodec::default();
    let mut package = codec.encrypt(b"important data", PASSWORD).unwrap();
    package[63] ^= 0x40;
    assert!(matches!(
        codec.decrypt(&package, PASSWORD),
        Err(BackupError::ChecksumMismatch)
    ));
    checksum_rewrite(&mut package);
    assert!(matches!(
        codec.decrypt(&package, PASSWORD),
        Err(BackupError::AuthenticationFailed)
    ));
}

#[test]
fn authenticated_header_rejects_tampered_salt_and_nonce() {
    let codec = BackupCodec::default();
    let package = codec.encrypt(b"data", PASSWORD).unwrap();
    for offset in [33, 49] {
        let mut modified = package.clone();
        modified[offset] ^= 0x80;
        checksum_rewrite(&mut modified);
        assert!(matches!(
            codec.decrypt(&modified, PASSWORD),
            Err(BackupError::AuthenticationFailed)
        ));
    }
}

#[test]
fn unknown_version_magic_and_untrusted_kdf_parameters_are_rejected_without_kdf() {
    let codec = BackupCodec::default();
    let mut package = codec.encrypt(b"data", PASSWORD).unwrap();
    package[8..10].copy_from_slice(&2_u16.to_le_bytes());
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::UnsupportedVersion)
    ));
    package[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    package[0] = b'X';
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::InvalidMagic)
    ));
    package[0] = MAGIC[0];
    package[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::UnsupportedParameters)
    ));
    package[12..16].copy_from_slice(&65_536_u32.to_le_bytes());
    package[10] = 1;
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::UnsupportedParameters)
    ));
}

#[test]
fn every_truncation_is_rejected_without_panicking_or_kdf() {
    let codec = BackupCodec::default();
    let package = codec.encrypt(b"data", PASSWORD).unwrap();
    for size in 0..package.len() {
        assert!(
            codec.inspect(&package[..size]).is_err(),
            "accepted truncation at {size}"
        );
    }
}

#[test]
fn inconsistent_declared_length_and_oversize_are_rejected() {
    let codec = BackupCodec::new(BackupLimits::new(32).unwrap());
    let mut package = codec.encrypt(b"abcd", PASSWORD).unwrap();
    package[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::SizeLimit)
    ));
    package[24..32].copy_from_slice(&5_u64.to_le_bytes());
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::InvalidLength)
    ));
    assert!(matches!(
        codec.encrypt(&[0_u8; 33], PASSWORD),
        Err(BackupError::SizeLimit)
    ));
    let larger_codec = BackupCodec::new(BackupLimits::new(64).unwrap());
    let larger = larger_codec.encrypt(&[1_u8; 33], PASSWORD).unwrap();
    assert!(matches!(
        codec.decrypt(&larger, PASSWORD),
        Err(BackupError::SizeLimit)
    ));
    assert!(matches!(BackupLimits::new(0), Err(BackupError::SizeLimit)));
    assert!(matches!(
        BackupLimits::new(HARD_MAX_PLAINTEXT + 1),
        Err(BackupError::SizeLimit)
    ));
}

#[test]
fn corrupt_outer_checksum_is_rejected_before_expensive_kdf() {
    let codec = BackupCodec::default();
    let mut package = codec.encrypt(b"payload", PASSWORD).unwrap();
    *package.last_mut().unwrap() ^= 1;
    assert!(matches!(
        codec.inspect(&package),
        Err(BackupError::ChecksumMismatch)
    ));
    assert!(matches!(
        codec.decrypt(&package, PASSWORD),
        Err(BackupError::ChecksumMismatch)
    ));
}

#[test]
fn weak_or_excessively_long_passphrases_are_rejected() {
    let codec = BackupCodec::default();
    assert!(matches!(
        codec.encrypt(b"data", b"short"),
        Err(BackupError::InvalidPassphrase)
    ));
    assert!(matches!(
        codec.encrypt(b"data", &[7_u8; 1025]),
        Err(BackupError::InvalidPassphrase)
    ));
    let package = codec.encrypt(b"data", PASSWORD).unwrap();
    assert!(matches!(
        codec.decrypt(&package, b""),
        Err(BackupError::InvalidPassphrase)
    ));
}

#[test]
fn atomic_encrypted_file_roundtrip_and_failure_preserves_existing_package() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("local.cbackup");
    let codec = BackupCodec::default();
    let info = codec
        .encrypt_to_file(&target, b"secret patient-free fixture", PASSWORD)
        .unwrap();
    let first = fs::read(&target).unwrap();
    assert_eq!(info.package_len, first.len());
    assert_eq!(
        codec
            .decrypt_from_file(&target, PASSWORD)
            .unwrap()
            .as_slice(),
        b"secret patient-free fixture"
    );
    assert!(!first
        .windows(b"secret patient-free fixture".len())
        .any(|part| part == b"secret patient-free fixture"));
    assert!(matches!(
        codec.encrypt_to_file(&target, b"other", b"short"),
        Err(BackupError::InvalidPassphrase)
    ));
    assert_eq!(fs::read(&target).unwrap(), first);
    codec
        .encrypt_to_file(&target, b"second snapshot", PASSWORD)
        .unwrap();
    assert_eq!(
        codec
            .decrypt_from_file(&target, PASSWORD)
            .unwrap()
            .as_slice(),
        b"second snapshot"
    );
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "no temporary files should remain"
    );
}

#[test]
fn file_read_rejects_oversize_before_allocating() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("oversize.cbackup");
    let codec = BackupCodec::new(BackupLimits::new(1).unwrap());
    let mut file = fs::File::create(&target).unwrap();
    file.write_all(&[0; 200]).unwrap();
    drop(file);
    assert!(matches!(
        codec.decrypt_from_file(&target, PASSWORD),
        Err(BackupError::SizeLimit)
    ));
}

#[test]
fn atomic_writer_does_not_create_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("missing").join("snapshot.cbackup");
    let codec = BackupCodec::default();
    assert!(matches!(
        codec.encrypt_to_file(&target, b"data", PASSWORD),
        Err(BackupError::Io(_))
    ));
    assert!(!Path::new(&target).exists());
}

#[test]
fn exclusive_writer_rejects_existing_files_without_modifying_them() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new.cbackup");
    let codec = BackupCodec::default();
    codec
        .encrypt_to_new_file(&target, b"first revision", PASSWORD)
        .unwrap();
    let original = fs::read(&target).unwrap();
    assert!(matches!(
        codec.encrypt_to_new_file(&target, b"second revision", PASSWORD),
        Err(BackupError::OutputExists)
    ));
    assert_eq!(fs::read(&target).unwrap(), original);
    assert_eq!(
        codec.decrypt(&original, PASSWORD).unwrap().as_slice(),
        b"first revision"
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn codec_refuses_symlinks_and_non_file_targets() {
    let dir = tempfile::tempdir().unwrap();
    let codec = BackupCodec::default();
    let directory = dir.path().join("output-dir");
    fs::create_dir(&directory).unwrap();
    assert!(matches!(
        codec.encrypt_to_file(&directory, b"data", PASSWORD),
        Err(BackupError::InvalidFilePath)
    ));

    #[cfg(unix)]
    let link_created = {
        use std::os::unix::fs::symlink;
        let target = dir.path().join("target.cbackup");
        fs::write(&target, b"unchanged").unwrap();
        let link = dir.path().join("link.cbackup");
        symlink(&target, &link).unwrap();
        Some((target, link))
    };
    #[cfg(windows)]
    let link_created = {
        use std::os::windows::fs::symlink_file;
        let target = dir.path().join("target.cbackup");
        fs::write(&target, b"unchanged").unwrap();
        let link = dir.path().join("link.cbackup");
        symlink_file(&target, &link).ok().map(|_| (target, link))
    };
    if let Some((target, link)) = link_created {
        assert!(matches!(
            codec.encrypt_to_file(&link, b"new data", PASSWORD),
            Err(BackupError::InvalidFilePath)
        ));
        assert!(matches!(
            codec.encrypt_to_new_file(&link, b"new data", PASSWORD),
            Err(BackupError::OutputExists)
        ));
        assert!(matches!(
            codec.decrypt_from_file(&link, PASSWORD),
            Err(BackupError::InvalidFilePath)
        ));
        assert_eq!(fs::read(target).unwrap(), b"unchanged");
    }
}

#[test]
fn portable_unicode_filename_and_relative_parent_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    let target = dir
        .path()
        .join("nested")
        .join("..")
        .join("نسخه-研究-🔒.cbackup");
    let codec = BackupCodec::default();
    codec
        .encrypt_to_new_file(&target, b"portable data", PASSWORD)
        .unwrap();
    assert_eq!(
        codec
            .decrypt_from_file(target, PASSWORD)
            .unwrap()
            .as_slice(),
        b"portable data"
    );
}
