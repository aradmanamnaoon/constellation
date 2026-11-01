#[test]
fn diagnose_live_profile_archive() {
    let Some(path) = std::env::var_os("CONSTELLATION_E2E_DATA_PATH") else {
        return;
    };
    let data = std::path::PathBuf::from(path);
    let target = data
        .join("backups-and-recovery")
        .join("diagnostic-project.cstlproj");
    let result = constellation_project_backup::ProjectBackupService.create_archive(
        data.join("constellation.sqlite3"),
        data.join("constellation-workflows.sqlite3"),
        target,
        b"diagnostic-only-not-saved-2026",
    );
    assert!(
        result.is_ok(),
        "isolated desktop archive failed: {result:?}"
    );
}
