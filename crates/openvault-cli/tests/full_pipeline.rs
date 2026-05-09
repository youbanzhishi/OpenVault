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

// ===========================================================================
// Phase 1 Tests (preserved)
// ===========================================================================

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

// ===========================================================================
// Phase 2 Tests
// ===========================================================================

#[test]
fn test_hash_based_incremental_detects_content_change() {
    // Test that incremental backup uses hash comparison, not just mtime+size.
    // This is the key Phase 2 improvement: even if mtime changes but content
    // is the same, the file won't be re-backed-up.
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("a.txt"), "original content").unwrap();

    let vault = tempfile::TempDir::new().unwrap();
    let config = make_config(source.path(), vault.path(), BackupStrategy::Incremental);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let inc_engine = engine_for_strategy(&BackupStrategy::Incremental);

    // Full backup
    let full_snap = full_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Full),
        &storage,
    ).unwrap();
    assert_eq!(full_snap.file_count(), 1);

    // Touch the file (change mtime but NOT content)
    thread::sleep(Duration::from_millis(100));
    // Use touch-like operation: rewrite with same content
    let original = fs::read(source.path().join("a.txt")).unwrap();
    fs::write(source.path().join("a.txt"), &original).unwrap();

    // Incremental: should detect 0 changes (content hash is the same)
    let inc1 = inc_engine.execute(&config, &storage).unwrap();
    assert_eq!(
        inc1.file_count(), 0,
        "Hash-based incremental should not re-backup file with same content"
    );

    // Now actually change content
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("a.txt"), "new content").unwrap();

    // Incremental: should detect 1 change
    let inc2 = inc_engine.execute(&config, &storage).unwrap();
    assert_eq!(
        inc2.file_count(), 1,
        "Hash-based incremental should detect actual content change"
    );
}

#[test]
fn test_differential_backup_against_full() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let diff_engine = engine_for_strategy(&BackupStrategy::Differential);

    // First: full backup
    let full_snap = full_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Full),
        &storage,
    ).unwrap();
    assert_eq!(full_snap.file_count(), 3);

    // Differential with NO changes → 0 files
    let diff1 = diff_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Differential),
        &storage,
    ).unwrap();
    assert_eq!(
        diff1.file_count(), 0,
        "Differential should have 0 changed files when nothing changed since full"
    );
    assert_eq!(diff1.base_snapshot_id.as_ref(), Some(&full_snap.id));

    // Modify one file
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified b").unwrap();

    // Differential → 1 file changed since full
    let diff2 = diff_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Differential),
        &storage,
    ).unwrap();
    assert_eq!(diff2.file_count(), 1);
    assert_eq!(diff2.entries[0].path, "b.txt");
}

#[test]
fn test_differential_includes_all_changes_since_full() {
    // Key test: differential captures ALL changes since full,
    // not just changes since last snapshot
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("a.txt"), "aaa").unwrap();
    fs::write(source.path().join("b.txt"), "bbb").unwrap();

    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let diff_engine = engine_for_strategy(&BackupStrategy::Differential);

    // Full backup
    let _full_snap = full_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Full),
        &storage,
    ).unwrap();

    // Modify a.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("a.txt"), "modified aaa").unwrap();

    // First differential → 1 change (a.txt)
    let diff1 = diff_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Differential),
        &storage,
    ).unwrap();
    assert_eq!(diff1.file_count(), 1);

    // Also modify b.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified bbb").unwrap();

    // Second differential → 2 changes (a.txt + b.txt, both changed since full)
    let diff2 = diff_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Differential),
        &storage,
    ).unwrap();
    assert_eq!(
        diff2.file_count(), 2,
        "Differential should include ALL changes since full, not just since last diff"
    );
}

#[test]
fn test_differential_fallback_to_full() {
    // When no full backup exists, differential should fall back to full
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("a.txt"), "aaa").unwrap();

    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let diff_engine = engine_for_strategy(&BackupStrategy::Differential);

    let snap = diff_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Differential),
        &storage,
    ).unwrap();

    // Should have fallen back to full backup
    assert_eq!(snap.strategy, BackupStrategy::Full);
    assert!(snap.parent_id.is_none());
    assert_eq!(snap.file_count(), 1);
}

#[test]
fn test_incremental_chain_restore() {
    // Test restoring from an incremental snapshot chain
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("a.txt"), "aaa").unwrap();
    fs::write(source.path().join("b.txt"), "bbb").unwrap();
    fs::write(source.path().join("c.txt"), "ccc").unwrap();

    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let inc_engine = engine_for_strategy(&BackupStrategy::Incremental);

    // Full backup
    let _full_snap = full_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Full),
        &storage,
    ).unwrap();

    // Modify b.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified bbb").unwrap();

    // Incremental #1
    let inc1 = inc_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Incremental),
        &storage,
    ).unwrap();
    assert_eq!(inc1.file_count(), 1);

    // Modify a.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("a.txt"), "modified aaa").unwrap();

    // Incremental #2
    let inc2 = inc_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Incremental),
        &storage,
    ).unwrap();
    assert_eq!(inc2.file_count(), 1);

    // Restore from inc2 → should get all files with latest versions
    let target = tempfile::TempDir::new().unwrap();
    storage.restore_snapshot(&inc2, target.path()).unwrap();

    assert_eq!(
        fs::read_to_string(target.path().join("a.txt")).unwrap(),
        "modified aaa",
        "a.txt should be from inc2"
    );
    assert_eq!(
        fs::read_to_string(target.path().join("b.txt")).unwrap(),
        "modified bbb",
        "b.txt should be from inc1"
    );
    assert_eq!(
        fs::read_to_string(target.path().join("c.txt")).unwrap(),
        "ccc",
        "c.txt should be from full"
    );
}

#[test]
fn test_differential_restore() {
    // Test restoring from a differential snapshot
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("a.txt"), "aaa").unwrap();
    fs::write(source.path().join("b.txt"), "bbb").unwrap();

    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let diff_engine = engine_for_strategy(&BackupStrategy::Differential);

    // Full backup
    let _full_snap = full_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Full),
        &storage,
    ).unwrap();

    // Modify b.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified bbb").unwrap();

    // Differential backup
    let diff_snap = diff_engine.execute(
        &make_config(source.path(), vault.path(), BackupStrategy::Differential),
        &storage,
    ).unwrap();
    assert_eq!(diff_snap.file_count(), 1);

    // Restore from differential → should get all files
    let target = tempfile::TempDir::new().unwrap();
    storage.restore_snapshot(&diff_snap, target.path()).unwrap();

    assert_eq!(
        fs::read_to_string(target.path().join("a.txt")).unwrap(),
        "aaa",
        "a.txt should be from full"
    );
    assert_eq!(
        fs::read_to_string(target.path().join("b.txt")).unwrap(),
        "modified bbb",
        "b.txt should be from differential"
    );
}

#[test]
fn test_yaml_differential_config() {
    let source = tempfile::TempDir::new().unwrap();
    fs::write(source.path().join("diff_test.txt"), "differential").unwrap();

    let vault = tempfile::TempDir::new().unwrap();

    let yaml = format!(
        r#"
name: "diff-yaml-test"
source: "{}"
strategy: "differential"
storage:
  type: "local"
  path: "{}"
"#,
        source.path().display(),
        vault.path().display()
    );

    let config = BackupConfig::load_from_str(&yaml).unwrap();
    assert_eq!(config.strategy, BackupStrategy::Differential);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&config.strategy);

    // First diff without full → falls back to full
    let snapshot = engine.execute(&config, &storage).unwrap();
    assert_eq!(snapshot.strategy, BackupStrategy::Full);
    assert_eq!(snapshot.file_count(), 1);
}

#[test]
fn test_s3_config_parsing() {
    let yaml = r#"
name: "s3-test"
source: "/tmp/data"
strategy: "full"
storage:
  type: "s3"
  bucket: "my-backups"
  prefix: "vault/"
  endpoint: "https://s3.example.com"
  region: "ap-southeast-1"
"#;
    let config = BackupConfig::load_from_str(yaml).unwrap();
    assert_eq!(config.name, "s3-test");
    match &config.storage {
        openvault_core::config::StorageConfig::S3 { bucket, prefix, endpoint, region, .. } => {
            assert_eq!(bucket, "my-backups");
            assert_eq!(prefix, "vault/");
            assert_eq!(endpoint.as_deref(), Some("https://s3.example.com"));
            assert_eq!(region.as_deref(), Some("ap-southeast-1"));
        }
        _ => panic!("Expected S3 storage config"),
    }
}
