use std::fs;
use std::thread;
use std::time::Duration;

use openvault_core::config::BackupConfig;
use openvault_core::crypto::{AesGcmEncryption, EncryptionProvider, Key256};
use openvault_core::engine::engine_for_strategy;
use openvault_core::integrity::{Checksum, HashAlgorithm, IntegrityEngine};
use openvault_core::restore::RestoreOptions;
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

#[test]
fn test_encrypt_backup_verify_restore_pipeline() {
    // Setup: create source files
    let source = tempfile::TempDir::new().unwrap();
    let original_content = "secret data for encryption test";
    fs::write(source.path().join("secret.txt"), original_content).unwrap();
    fs::write(source.path().join("config.json"), r#"{"key": "value"}"#).unwrap();
    
    // Step 1: Generate encryption key
    let key = Key256::generate();
    let encryptor = AesGcmEncryption::new(key.as_bytes()).unwrap();
    
    // Step 2: Encrypt files manually for storage
    let vault = tempfile::TempDir::new().unwrap();
    let storage = LocalVaultStorage::new(vault.path()).unwrap();
    
    // Encrypt and store each file
    let secret_encrypted = encryptor.encrypt(original_content.as_bytes()).unwrap();
    let config_encrypted = encryptor.encrypt(br#"{"key": "value"}"#).unwrap();
    
    // Store encrypted data
    storage.store_file("encrypted-backup", "secret.txt", &secret_encrypted).unwrap();
    storage.store_file("encrypted-backup", "config.json", &config_encrypted).unwrap();
    
    // Step 3: Verify encrypted data is not plaintext
    let retrieved = storage.retrieve_file("encrypted-backup", "secret.txt").unwrap();
    assert_ne!(retrieved.as_slice(), original_content.as_bytes(), 
        "Encrypted data should not equal plaintext");
    
    // Step 4: Decrypt and verify content
    let decrypted = encryptor.decrypt(&retrieved).unwrap();
    assert_eq!(String::from_utf8(decrypted).unwrap(), original_content);
    
    // Step 5: Test integrity verification with SHA-256
    let checksum = Checksum::compute(original_content.as_bytes(), HashAlgorithm::Sha256);
    assert!(checksum.verify(original_content.as_bytes()));
    
    // Step 6: Test key derivation from password
    let salt = b"test_salt_1234567890";
    let derived_key = Key256::from_password("test_password", salt).unwrap();
    assert_ne!(derived_key.to_hex(), key.to_hex(), "Derived key should differ from random key");
    
    // Step 7: Verify same password + salt produces same key
    let derived_key2 = Key256::from_password("test_password", salt).unwrap();
    assert_eq!(derived_key.to_hex(), derived_key2.to_hex());
}

#[test]
fn test_integrity_engine_multi_file_verification() {
    // Create temp directory with test files
    let temp_dir = tempfile::TempDir::new().unwrap();
    
    // Create test files
    let file1_path = temp_dir.path().join("file1.txt");
    let file2_path = temp_dir.path().join("file2.txt");
    
    fs::write(&file1_path, "content of file 1").unwrap();
    fs::write(&file2_path, "content of file 2").unwrap();
    
    // Compute checksums
    let checksum1 = IntegrityEngine::verify_file(&file1_path, &Checksum::compute(b"content of file 1", HashAlgorithm::Sha256).value).unwrap();
    let checksum2 = IntegrityEngine::verify_file(&file2_path, &Checksum::compute(b"content of file 2", HashAlgorithm::Sha256).value).unwrap();
    
    // Verify individual files
    assert!(checksum1.passed);
    assert!(checksum2.passed);
    
    // Test checksum mismatch detection
    let bad_check = IntegrityEngine::verify_file(&file1_path, "0000000000000000000000000000000000000000000000000000000000000000").unwrap();
    assert!(!bad_check.passed);
    assert!(bad_check.error.is_some());
    
    // Test aggregate checksum
    let checksums = vec![
        Checksum::new(HashAlgorithm::Sha256, "aaa".to_string()),
        Checksum::new(HashAlgorithm::Sha256, "bbb".to_string()),
    ];
    let aggregate = IntegrityEngine::compute_aggregate_checksum(&checksums).unwrap();
    assert!(!aggregate.is_empty());
    assert_eq!(aggregate.len(), 64); // SHA-256 produces 64 hex chars
}

#[test]
fn test_restore_options_builder() {
    // Test builder pattern for RestoreOptions
    let options = RestoreOptions::to("/target")
        .skip_existing()
        .filter_files(vec!["file1.txt".to_string()]);
    
    assert_eq!(options.target.to_str(), Some("/target"));
    assert_eq!(options.conflict, openvault_core::restore::ConflictStrategy::Skip);
    assert_eq!(options.filter_paths, vec!["file1.txt"]);
}

#[test]
fn test_aes_gcm_different_nonces_each_encryption() {
    // Verify that each encryption produces unique ciphertext due to random nonce
    let encryptor = AesGcmEncryption::generate();
    let data = b"test data";
    
    let ciphertext1 = encryptor.encrypt(data).unwrap();
    let ciphertext2 = encryptor.encrypt(data).unwrap();
    
    // Nonces are different, so ciphertexts should be different
    assert_ne!(ciphertext1, ciphertext2);
    
    // Both should decrypt to the same plaintext
    let decrypted1 = encryptor.decrypt(&ciphertext1).unwrap();
    let decrypted2 = encryptor.decrypt(&ciphertext2).unwrap();
    
    assert_eq!(decrypted1, data);
    assert_eq!(decrypted2, data);
}

#[test]
fn test_encryption_with_wrong_key_fails() {
    let encryptor = AesGcmEncryption::generate();
    let wrong_key = Key256::generate();
    let wrong_decryptor = AesGcmEncryption::new(wrong_key.as_bytes()).unwrap();
    
    let ciphertext = encryptor.encrypt(b"secret data").unwrap();
    let result = wrong_decryptor.decrypt(&ciphertext);
    
    assert!(result.is_err());
}

#[test]
fn test_tampered_ciphertext_fails_authentication() {
    let encryptor = AesGcmEncryption::generate();
    let mut ciphertext = encryptor.encrypt(b"original data").unwrap();
    
    // Tamper with the ciphertext
    if ciphertext.len() > 20 {
        ciphertext[20] ^= 0xFF;
    }
    
    let result = encryptor.decrypt(&ciphertext);
    assert!(result.is_err());
}
