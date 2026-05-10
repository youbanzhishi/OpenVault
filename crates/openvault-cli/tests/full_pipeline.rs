//! End-to-end integration tests for OpenVault Phase 5.
//!
//! Tests cover:
//! - Full backup → incremental → differential → restore → verify pipeline
//! - 3-2-1 policy engine
//! - Self-healing mechanism
//! - Multi-storage backend scenarios

use std::fs;
use std::thread;
use std::time::Duration;

use openvault_core::config::BackupConfig;
use openvault_core::engine::engine_for_strategy;
use openvault_core::healing::{HealingConfig, HealingEngine};
use openvault_core::integrity::{Checksum, HashAlgorithm};
use openvault_core::policy::{Policy321, PolicyEngine};
use openvault_core::restore::{ConflictStrategy, RestoreEngine, RestoreOptions};
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

fn make_config(
    source: &std::path::Path,
    vault: &std::path::Path,
    strategy: BackupStrategy,
) -> BackupConfig {
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

// ============================================================================
// Full pipeline: backup → incremental → differential → restore → verify
// ============================================================================

#[test]
fn test_full_pipeline_with_all_strategies() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    // Step 1: Full backup
    let full_config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let full_snap = full_engine.execute(&full_config, &storage).unwrap();
    assert_eq!(full_snap.file_count(), 3); // a.txt, b.txt, subdir/c.txt
    assert_eq!(full_snap.strategy, BackupStrategy::Full);
    assert!(full_snap.parent_id.is_none());

    // Step 2: Modify a file
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified b content").unwrap();

    // Step 3: Incremental backup
    let inc_config = make_config(source.path(), vault.path(), BackupStrategy::Incremental);
    let inc_engine = engine_for_strategy(&BackupStrategy::Incremental);
    let inc_snap = inc_engine.execute(&inc_config, &storage).unwrap();
    assert_eq!(
        inc_snap.file_count(),
        1,
        "Incremental should detect 1 changed file"
    );
    assert_eq!(inc_snap.entries[0].path, "b.txt");
    assert_eq!(inc_snap.parent_id, Some(full_snap.id.clone()));

    // Step 4: Modify another file
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("a.txt"), "modified a content").unwrap();

    // Step 5: Differential backup (compares against the full, not the incremental)
    let diff_config = make_config(source.path(), vault.path(), BackupStrategy::Differential);
    let diff_engine = engine_for_strategy(&BackupStrategy::Differential);
    let diff_snap = diff_engine.execute(&diff_config, &storage).unwrap();
    // Differential should find both a.txt and b.txt changed since the full backup
    assert!(
        diff_snap.file_count() >= 2,
        "Differential should detect changes since full backup, got {}",
        diff_snap.file_count()
    );
    assert!(diff_snap.entries.iter().any(|e| e.path == "a.txt"));
    assert!(diff_snap.entries.iter().any(|e| e.path == "b.txt"));

    // Step 6: Restore from full backup
    let restore_dir = tempfile::TempDir::new().unwrap();
    storage
        .restore_snapshot(&full_snap, restore_dir.path())
        .unwrap();
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
fn test_full_backup_and_restore() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&BackupStrategy::Full);

    let snapshot = engine.execute(&config, &storage).unwrap();
    assert_eq!(snapshot.file_count(), 3);
    assert_eq!(snapshot.strategy, BackupStrategy::Full);

    let restore_dir = tempfile::TempDir::new().unwrap();
    storage
        .restore_snapshot(&snapshot, restore_dir.path())
        .unwrap();

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

    let full_snap = full_engine
        .execute(
            &make_config(source.path(), vault.path(), BackupStrategy::Full),
            &storage,
        )
        .unwrap();
    assert_eq!(full_snap.file_count(), 3);

    let inc1 = inc_engine.execute(&config, &storage).unwrap();
    assert_eq!(inc1.file_count(), 0);

    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified b content").unwrap();

    let inc2 = inc_engine.execute(&config, &storage).unwrap();
    assert_eq!(inc2.file_count(), 1);
    assert_eq!(inc2.entries[0].path, "b.txt");
}

#[test]
fn test_differential_compares_to_full() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    // Full backup
    let full_config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let _full_snap = full_engine.execute(&full_config, &storage).unwrap();

    // Modify b.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("b.txt"), "modified b").unwrap();

    // Incremental (only b changed)
    let inc_engine = engine_for_strategy(&BackupStrategy::Incremental);
    let inc_snap = inc_engine
        .execute(
            &make_config(source.path(), vault.path(), BackupStrategy::Incremental),
            &storage,
        )
        .unwrap();
    assert_eq!(inc_snap.file_count(), 1);

    // Modify a.txt
    thread::sleep(Duration::from_millis(100));
    fs::write(source.path().join("a.txt"), "modified a").unwrap();

    // Differential should compare against the full, not the incremental
    let diff_engine = engine_for_strategy(&BackupStrategy::Differential);
    let diff_snap = diff_engine
        .execute(
            &make_config(source.path(), vault.path(), BackupStrategy::Differential),
            &storage,
        )
        .unwrap();
    // Both a.txt and b.txt changed since full
    assert!(diff_snap.file_count() >= 2);
}

// ============================================================================
// Integrity verification
// ============================================================================

#[test]
fn test_verify_snapshot_integrity() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let snapshot = engine.execute(&config, &storage).unwrap();

    // Verify using RestoreEngine
    let restore_engine =
        RestoreEngine::new(std::sync::Arc::new(storage) as std::sync::Arc<dyn VaultStorage>);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let report = rt.block_on(restore_engine.verify(&snapshot)).unwrap();
    assert!(report.is_ok());
    assert_eq!(report.files_ok, 3);
}

// ============================================================================
// Snapshot management
// ============================================================================

#[test]
fn test_snapshot_list_and_delete() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);

    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    let engine = engine_for_strategy(&BackupStrategy::Full);

    let snap1 = engine.execute(&config, &storage).unwrap();
    let _snap2 = engine.execute(&config, &storage).unwrap();

    let list = storage.list_snapshots().unwrap();
    assert!(list.len() >= 2);

    storage.delete_snapshot(&snap1.id).unwrap();
    let list_after = storage.list_snapshots().unwrap();
    assert_eq!(list_after.len(), list.len() - 1);
    assert!(list_after.iter().all(|s| s.id != snap1.id));
}

#[test]
fn test_latest_full_snapshot() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    let full_engine = engine_for_strategy(&BackupStrategy::Full);
    let inc_engine = engine_for_strategy(&BackupStrategy::Incremental);

    let full_snap = full_engine
        .execute(
            &make_config(source.path(), vault.path(), BackupStrategy::Full),
            &storage,
        )
        .unwrap();

    let _inc_snap = inc_engine
        .execute(
            &make_config(source.path(), vault.path(), BackupStrategy::Incremental),
            &storage,
        )
        .unwrap();

    let latest_full = storage
        .latest_full_snapshot(source.path().to_string_lossy().to_string())
        .unwrap();
    assert!(latest_full.is_some());
    assert_eq!(latest_full.unwrap().id, full_snap.id);
}

// ============================================================================
// 3-2-1 Policy engine
// ============================================================================

#[test]
fn test_321_policy_single_local_violates() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let _snap = engine.execute(&config, &storage).unwrap();

    // Strict 3-2-1 policy should fail with only one local copy
    let policy = Policy321::strict();
    let policy_engine = PolicyEngine::new(policy);
    let source_key = source.path().to_string_lossy().to_string();
    let health = policy_engine
        .check_health(&source_key, &[&storage])
        .unwrap();

    assert!(!health.healthy);
    assert!(health.copies < 3);
    assert!(!health.has_offsite);
    assert!(!health.violations.is_empty());
}

#[test]
fn test_321_policy_relaxed_single_local_passes() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let _snap = engine.execute(&config, &storage).unwrap();

    // Relaxed policy should pass with one local copy
    let policy = Policy321::relaxed();
    let policy_engine = PolicyEngine::new(policy);
    let source_key = source.path().to_string_lossy().to_string();
    let health = policy_engine
        .check_health(&source_key, &[&storage])
        .unwrap();

    assert!(health.healthy);
    assert_eq!(health.copies, 1);
}

#[test]
fn test_321_policy_multi_storage() {
    let source = setup_source_dir();
    let vault1 = tempfile::TempDir::new().unwrap();
    let vault2 = tempfile::TempDir::new().unwrap();
    let storage1 = LocalVaultStorage::new(vault1.path()).unwrap();
    let storage2 = LocalVaultStorage::new(vault2.path()).unwrap();

    let config = make_config(source.path(), vault1.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let snap = engine.execute(&config, &storage1).unwrap();

    // Copy to second storage
    storage2.store_snapshot(&snap).unwrap();
    for entry in &snap.entries {
        let data = storage1.retrieve_file(&snap.id, &entry.path).unwrap();
        storage2.store_file(&snap.id, &entry.path, &data).unwrap();
    }

    // With 2 local copies, we still violate strict 3-2-1 (need offsite)
    let policy = Policy321::strict();
    let policy_engine = PolicyEngine::new(policy);
    let source_key = source.path().to_string_lossy().to_string();
    let health = policy_engine
        .check_health(&source_key, &[&storage1, &storage2])
        .unwrap();

    assert!(!health.healthy); // No offsite copy
    assert_eq!(health.copies, 2);
    assert!(!health.has_offsite);
}

// ============================================================================
// Self-healing
// ============================================================================

#[test]
fn test_healing_scan_healthy() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let snap = engine.execute(&config, &storage).unwrap();

    let result = HealingEngine::scan(&storage, &snap).unwrap();
    assert!(result.is_all_healthy());
    assert_eq!(result.files_scanned, 3);
    assert_eq!(result.files_healthy, 3);
    assert_eq!(result.files_corrupt, 0);
}

#[test]
fn test_healing_detects_corruption() {
    let source = setup_source_dir();
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();

    let config = make_config(source.path(), vault.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let snap = engine.execute(&config, &storage).unwrap();

    // Corrupt one file by overwriting it with different data
    storage
        .store_file(&snap.id, "b.txt", b"corrupted data")
        .unwrap();

    let result = HealingEngine::scan(&storage, &snap).unwrap();
    assert!(!result.is_all_healthy());
    assert_eq!(result.files_corrupt, 1);

    let corrupt_file = result.checks.iter().find(|c| !c.healthy).unwrap();
    assert_eq!(corrupt_file.path, "b.txt");
}

#[test]
fn test_healing_repairs_from_healthy_replica() {
    let source = setup_source_dir();
    let healthy_vault = tempfile::TempDir::new().unwrap();
    let corrupt_vault = tempfile::TempDir::new().unwrap();

    let healthy_storage = LocalVaultStorage::new(healthy_vault.path()).unwrap();
    let corrupt_storage = LocalVaultStorage::new(corrupt_vault.path()).unwrap();

    let config = make_config(source.path(), healthy_vault.path(), BackupStrategy::Full);
    let engine = engine_for_strategy(&BackupStrategy::Full);
    let snap = engine.execute(&config, &healthy_storage).unwrap();

    // Copy snapshot to corrupt storage
    corrupt_storage.store_snapshot(&snap).unwrap();
    for entry in &snap.entries {
        let data = healthy_storage
            .retrieve_file(&snap.id, &entry.path)
            .unwrap();
        corrupt_storage
            .store_file(&snap.id, &entry.path, &data)
            .unwrap();
    }

    // Corrupt one file
    corrupt_storage
        .store_file(&snap.id, "b.txt", b"corrupted!!!")
        .unwrap();

    // Verify corruption detected
    let scan = HealingEngine::scan(&corrupt_storage, &snap).unwrap();
    assert!(!scan.is_all_healthy());

    // Heal from healthy replica
    let config = HealingConfig::default();
    let result = HealingEngine::heal(&corrupt_storage, &healthy_storage, &snap, &config).unwrap();
    assert_eq!(result.files_healed, 1);
    assert_eq!(result.files_failed, 0);

    // Verify data is now correct
    let healed_data = corrupt_storage.retrieve_file(&snap.id, "b.txt").unwrap();
    let original_data = healthy_storage.retrieve_file(&snap.id, "b.txt").unwrap();
    assert_eq!(healed_data, original_data);

    // Full scan should now pass
    let rescan = HealingEngine::scan(&corrupt_storage, &snap).unwrap();
    assert!(rescan.is_all_healthy());
}

// ============================================================================
// Config and strategy parsing
// ============================================================================

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

#[test]
fn test_differential_strategy_yaml_config() {
    let yaml = r#"
name: "diff-test"
source: "/tmp/nonexistent"
strategy: "differential"
storage:
  type: "local"
  path: "/tmp/vault-diff"
"#;
    let config = BackupConfig::load_from_str(yaml).unwrap();
    assert_eq!(config.strategy, BackupStrategy::Differential);
}
