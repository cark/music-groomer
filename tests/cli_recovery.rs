use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use music_groomer::config::AppConfig;
use music_groomer::recovery::{
    RecoveryIndex, RecoveryStore, ReleaseLineage, RetainedVersion, new_lineage_id, new_version_id,
};
use tempfile::TempDir;

#[test]
fn recovery_without_an_action_opens_the_guided_manager() {
    let temporary = TempDir::new().unwrap();
    let config_home = temporary.path().join("config-home");

    let output = Command::new(binary())
        .arg("recovery")
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Recovery"));
    assert!(stdout.contains("Library: not configured"));
    assert!(stdout.contains("No retained copies are available"));
    assert!(stdout.contains("Preferences"));
    assert!(!config_home.exists());
}

#[test]
fn maintenance_without_a_recovery_store_is_a_successful_read_only_noop() {
    let temporary = TempDir::new().unwrap();
    let library = temporary.path().join("library");
    fs::create_dir(&library).unwrap();
    let canonical_library = library.canonicalize().unwrap();
    let config_home = configured_library(&temporary, &library, 1);

    let output = Command::new(binary())
        .args(["recovery", "maintain"])
        .env("XDG_CONFIG_HOME", config_home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Recovery maintenance"));
    assert!(stdout.contains(canonical_library.to_str().unwrap()));
    assert!(stdout.contains("No eligible retained copies needed eviction"));
    assert!(stdout.contains("Recovery usage: 0 B / 1.0 MiB"));
    assert!(RecoveryStore::open_existing(&library).unwrap().is_none());
}

#[test]
fn maintenance_evicts_an_eligible_copy_and_reports_what_changed() {
    let temporary = TempDir::new().unwrap();
    let library = temporary.path().join("library");
    fs::create_dir(&library).unwrap();
    let config_home = configured_library(&temporary, &library, 1);
    let store = RecoveryStore::create_or_open(&library).unwrap();
    let lock = store.lock().unwrap();
    let lineage_id = new_lineage_id();
    let eligible = prepared_version(
        &store,
        &lock,
        &lineage_id,
        "Artist — Eligible Album",
        1,
        1,
        &vec![b'a'; 1024 * 1024 + 1],
        None,
    );
    let lineage_storage = eligible.storage_path.parent().unwrap().to_owned();
    let protected = prepared_version(
        &store,
        &lock,
        &lineage_id,
        "Artist — Protected Album",
        2,
        u64::MAX,
        b"safe",
        Some(&lineage_storage),
    );
    let mut index = RecoveryIndex::default();
    index.lineages.push(ReleaseLineage {
        lineage_id,
        active_version_id: new_version_id(),
        expected_active_path: PathBuf::from("Artist/Active Album"),
        retained_versions: vec![eligible.clone(), protected.clone()],
    });
    store.save_index(&lock, &index).unwrap();
    drop(lock);

    let output = Command::new(binary())
        .args(["recovery", "maintain"])
        .env("XDG_CONFIG_HOME", config_home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Removed Artist — Eligible Album"));
    assert!(stdout.contains("Recovery usage: 4 B / 1.0 MiB"));
    assert!(
        !store
            .retained_payload_path(&eligible.storage_path)
            .unwrap()
            .exists()
    );
    assert!(
        store
            .retained_payload_path(&protected.storage_path)
            .unwrap()
            .exists()
    );
    let updated = store.load_index().unwrap();
    assert_eq!(updated.lineages[0].retained_versions.len(), 1);
    assert_eq!(
        updated.lineages[0].retained_versions[0].version_id,
        protected.version_id
    );
}

fn configured_library(temporary: &TempDir, library: &Path, max_mib: u64) -> PathBuf {
    let config_home = temporary.path().join("config-home");
    let config_path = config_home.join("music-groomer/config.toml");
    AppConfig {
        destination: Some(library.to_owned()),
        recovery_max_mib: Some(max_mib),
        ..AppConfig::default()
    }
    .save_to(&config_path)
    .unwrap();
    config_home
}

#[allow(clippy::too_many_arguments)]
fn prepared_version(
    store: &RecoveryStore,
    lock: &music_groomer::recovery::RecoveryLock,
    lineage_id: &str,
    display_label: &str,
    retained_at: u64,
    protected_until: u64,
    contents: &[u8],
    lineage_storage: Option<&Path>,
) -> RetainedVersion {
    let version_id = new_version_id();
    let prepared = store
        .prepare_retained_payload(
            lock,
            lineage_id,
            &version_id,
            display_label,
            retained_at,
            lineage_storage,
        )
        .unwrap();
    fs::create_dir(&prepared.payload).unwrap();
    fs::write(prepared.payload.join("track.flac"), contents).unwrap();
    RetainedVersion {
        version_id,
        historical_path: PathBuf::from("Artist/Album"),
        display_label: display_label.into(),
        retained_at,
        protected_until,
        size_bytes: 0,
        storage_path: prepared.storage_path,
    }
}

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_music-groomer"))
}
