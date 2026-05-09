use std::fs;
use std::thread;
use std::time::Duration;

use openvault_core::config::BackupConfig;
use openvault_core::engine::engine_for_strategy;
use openvault_core::snapshot::BackupStrategy;
use openvault_core::storage::VaultStorage;
use openvault_storage::LocalVaultStorage;

/// Helper: create a temp source directory with some files.
fn setup_source_dir() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello from a").unwrap();
    fs::write(dir.path().join("b.txt"), "hello from b").unwrap();
    fs::create_dir_all(dir.path().join("subdir")).unwrap();
    fs::write(dir.path().join("subdir/c.txt"), "hello from c").unwrap();
    dir
}

fn make_config(source: &std::path::Path, vault: &std::path::Path, strategy: BackupStrategy) -> BackupConfig {
    BackupConfig {
        name: "integration-test".into(),
        source: source.to_path_buf(),
        storage: openvault_core::config::StorageConfig::Local {
            path: vault.to_path_buf(),
        },
        strategy,
        exclude: vec!["*.tmp".into()],
        schedule: None,
    }
}

#[test]
fn test_full_backup_and_restore() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&BackupStrategy::Full);

    // Execute full backup
    let snapshot = engine.execute(&config, &storage).unwrap();
    assert_eq!(snapshot.file_count(), 3); // a.txt, b.txt, subdir/c.txt
    assert_eq!(snapshot.strategy, BackupStrategy::Full);
    assert!(snapshot.parent_id.is_none());

    // Restore
    let restore_dir = tempfile::TempDir::new().unwrap();
    storage.restore_snapshot(&snapshot, restore_dir.path()).unwrap();

    // Verify restored files
    assert_eq!(
        fs::read_to_string(restore_dir.path().join("a.txt")).unwrap(),
        "hello from a"
    );
    assert_eq!(
        fs::read_to_string(restore_dir.path().join("subdir/c.txt")).unwrap(),
        "hello from c"
    );
}

#[test]
fn test_incremental_backup_only_changed_files() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let config = make_config(source.path(), vault.path(), BackupStrategy::Incremental);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let inc_engine = engine_for_strategy(&BackupStrategy::Incremental);

    // First: full backup
    let full_snap = full_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Full),
        &storage,
    ).unwrap();
    assert_eq!(full_snap.file_count(), 3);

    // Second: incremental with NO changes → should be 0 files
    let inc1 = inc_engine.execute(&config, &storage).unwrap();
    assert_eq!(inc1.file_count(), 0, "Incremental should have 0 changed files when nothing changed");
    assert_eq!(inc1.parent_id.as_ref(), Some(&full_snap.id));

    // Now modify one file — sleep briefly to ensure mtime changes on filesystem
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified b content").unwrap();

    // Third: incremental → should detect 1 change
    let inc2 = inc_engine.execute(&config, &storage).unwrap();
    assert_eq!(inc2.file_count(), 1, "Incremental should detect the 1 modified file");
    assert_eq!(inc2.entries[0].path, "b.txt");
}

#[test]
fn test_snapshot_list_and_delete() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&BackupStrategy::Full);

    let snap1 = engine.execute(&config, &storage).unwrap();
    let _snap2 = engine.execute(&config, &storage).unwrap();

    // List
    let list = storage.list_snapshots().unwrap();
    assert!(list.len() >= 2, "Should have at least 2 snapshots, got {}", list.len());

    // Delete
    storage.delete_snapshot(&snap1.id).unwrap();
    let list_after = storage.list_snapshots().unwrap();
    assert_eq!(list_after.len(), list.len() - 1);
    assert!(list_after.iter().all(|s| s.id != snap1.id));
}

#[test]
fn test_exclude_patterns() {
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("keep.txt"), "data").unwrap();
    fs::write(source.path().join("skip.tmp"), "temp").unwrap();

    let vault = tempfile::TempDir::new().unwrap();
    let config = BackupConfig {
        name: "exclude-test".into(),
        source: source.path().to_path_buf(),
        storage: openvault_core::config::StorageConfig::Local {
            path: vault.path().to_path_buf(),
        },
        strategy: BackupStrategy::Full,
        exclude: vec!["*.tmp".into()],
        schedule: None,
    };

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&BackupStrategy::Full);

    let snapshot = engine.execute(&config, &storage).unwrap();
    assert_eq!(snapshot.file_count(), 1);
    assert_eq!(snapshot.entries[0].path, "keep.txt");
}

#[test]
fn test_yaml_config_drives_backup() {
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("config_test.txt"), "yaml driven").unwrap();

    let vault = tempfile::TempDir::new().unwrap();

    let yaml = format!(
        r#"
name: "yaml-driven-test"
source: "{}"
strategy: "full"
storage:
  type: "local"
  path: "{}"
exclude:
  - "*.log"
"#,
        source.path().display(),
        vault.path().display()
    );

    let config = BackupConfig::load_from_str(&yaml).unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&config.strategy);

    let snapshot = engine.execute(&config, &storage).unwrap();
    assert_eq!(snapshot.file_count(), 1);
    assert_eq!(snapshot.entries[0].path, "config_test.txt");
}
