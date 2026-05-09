//! Restore engine for OpenVault.
//!
//! Provides full and partial restore capabilities with optional decryption
//! and conflict resolution strategies.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::crypto::{EncryptionAlgorithm, EncryptionProvider, EncryptionProviderFactory, Key256};
use crate::error::{VaultError, VaultResult};
use crate::integrity::{Checksum, HashAlgorithm};
use crate::snapshot::{BackupEntry, FileEntry, Snapshot};
use crate::storage::VaultStorage;

/// Conflict resolution strategy when target file exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Skip restoring files that already exist.
    Skip,
    /// Overwrite existing files.
    Overwrite,
    /// Rename restored file (append .restored suffix).
    Rename,
    /// Fail if any file conflicts.
    Fail,
}

impl Default for ConflictStrategy {
    fn default() -> Self {
        ConflictStrategy::Overwrite
    }
}

/// Options for restore operations.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Target directory for restored files.
    pub target: PathBuf,
    /// Conflict resolution strategy.
    pub conflict: ConflictStrategy,
    /// Whether to verify checksums after restore.
    pub verify_checksums: bool,
    /// Optional encryption algorithm (if backup was encrypted).
    pub encryption: Option<EncryptionAlgorithm>,
    /// Optional encryption key (base64 encoded).
    pub encryption_key: Option<String>,
    /// Specific file paths to restore (empty = all files).
    pub filter_paths: Vec<String>,
    /// Whether to preserve original timestamps.
    pub preserve_timestamps: bool,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            target: PathBuf::from("."),
            conflict: ConflictStrategy::default(),
            verify_checksums: true,
            encryption: None,
            encryption_key: None,
            filter_paths: Vec::new(),
            preserve_timestamps: true,
        }
    }
}

impl RestoreOptions {
    /// Create options targeting a specific directory.
    pub fn to(target: impl Into<PathBuf>) -> Self {
        Self {
            target: target.into(),
            ..Default::default()
        }
    }

    /// Set encryption for encrypted backups.
    pub fn with_encryption(mut self, algorithm: EncryptionAlgorithm, key: String) -> Self {
        self.encryption = Some(algorithm);
        self.encryption_key = Some(key);
        self
    }

    /// Set conflict strategy.
    pub fn with_conflict_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.conflict = strategy;
        self
    }

    /// Skip existing files.
    pub fn skip_existing(mut self) -> Self {
        self.conflict = ConflictStrategy::Skip;
        self
    }

    /// Enable overwriting existing files.
    pub fn overwrite_existing(mut self) -> Self {
        self.conflict = ConflictStrategy::Overwrite;
        self
    }

    /// Rename conflicting files.
    pub fn rename_existing(mut self) -> Self {
        self.conflict = ConflictStrategy::Rename;
        self
    }

    /// Only restore specific paths.
    pub fn filter_files(mut self, paths: Vec<String>) -> Self {
        self.filter_paths = paths;
        self
    }
}

/// Report of a restore operation.
#[derive(Debug, Clone, Default)]
pub struct RestoreReport {
    /// Number of files successfully restored.
    pub files_restored: u32,
    /// Number of files skipped.
    pub files_skipped: u32,
    /// Number of files that failed checksum verification.
    pub checksum_failures: u32,
    /// Number of errors encountered.
    pub errors: Vec<RestoreError>,
    /// Total bytes restored.
    pub bytes_restored: u64,
    /// Files that were renamed due to conflict.
    pub files_renamed: u32,
}

impl RestoreReport {
    /// Check if the restore was completely successful.
    pub fn is_success(&self) -> bool {
        self.errors.is_empty() && self.checksum_failures == 0
    }

    /// Get summary string.
    pub fn summary(&self) -> String {
        format!(
            "Restored: {}, Skipped: {}, Renamed: {}, Checksum failures: {}, Errors: {}, Bytes: {}",
            self.files_restored,
            self.files_skipped,
            self.files_renamed,
            self.checksum_failures,
            self.errors.len(),
            self.bytes_restored
        )
    }
}

/// Single error during restore.
#[derive(Debug, Clone)]
pub struct RestoreError {
    /// File path that failed.
    pub path: String,
    /// Error message.
    pub message: String,
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Encrypted file block metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EncryptedBlock {
    /// Original file path.
    pub path: String,
    /// Original size.
    pub original_size: u64,
    /// Checksum of original data.
    pub checksum: String,
    /// Checksum algorithm used.
    pub checksum_algorithm: HashAlgorithm,
    /// Encryption algorithm used.
    pub encryption_algorithm: EncryptionAlgorithm,
}

/// Verification report for snapshot integrity.
#[derive(Debug, Clone, Default)]
pub struct VerifyReport {
    /// Number of files that passed verification.
    pub files_ok: u32,
    /// Number of files that failed verification.
    pub files_failed: u32,
    /// Verification errors.
    pub errors: Vec<VerifyError>,
}

impl VerifyReport {
    /// Check if all files passed verification.
    pub fn is_ok(&self) -> bool {
        self.files_failed == 0
    }

    /// Get summary.
    pub fn summary(&self) -> String {
        format!(
            "Verified: {}/{} OK",
            self.files_ok,
            self.files_ok + self.files_failed
        )
    }
}

/// Single verification error.
#[derive(Debug, Clone)]
pub struct VerifyError {
    /// File path that failed.
    pub path: String,
    /// Error message.
    pub message: String,
}

/// The restore engine.
pub struct RestoreEngine {
    storage: Arc<dyn VaultStorage>,
    crypto: Option<Arc<dyn EncryptionProvider>>,
}

impl RestoreEngine {
    /// Create a new restore engine.
    pub fn new(storage: Arc<dyn VaultStorage>) -> Self {
        Self {
            storage,
            crypto: None,
        }
    }

    /// Create with encryption support.
    pub fn with_encryption(
        storage: Arc<dyn VaultStorage>,
        algorithm: EncryptionAlgorithm,
        key_base64: &str,
    ) -> VaultResult<Self> {
        let key_bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            key_base64,
        )
        .map_err(|e| VaultError::Crypto(format!("Invalid base64 key: {}", e)))?;
        let key = Key256::from_bytes(&key_bytes)?;
        let crypto =
            EncryptionProviderFactory::create(algorithm, key.as_bytes())?;

        Ok(Self {
            storage,
            crypto: Some(crypto),
        })
    }

    /// Create with a pre-built encryption provider.
    pub fn with_provider(storage: Arc<dyn VaultStorage>, crypto: Arc<dyn EncryptionProvider>) -> Self {
        Self {
            storage,
            crypto: Some(crypto),
        }
    }

    /// Restore an entire snapshot.
    pub async fn restore(
        &self,
        snapshot: &Snapshot,
        options: RestoreOptions,
    ) -> VaultResult<RestoreReport> {
        let mut report = RestoreReport::default();

        // Create target directory
        std::fs::create_dir_all(&options.target).map_err(|e| {
            VaultError::RestoreFailed(format!(
                "Failed to create target directory: {}",
                e
            ))
        })?;

        for entry in &snapshot.entries {
            // Apply filter if specified
            if !options.filter_paths.is_empty() {
                if !options.filter_paths.contains(&entry.path) {
                    report.files_skipped += 1;
                    continue;
                }
            }

            match self.restore_file(snapshot, entry, &options, &mut report).await {
                Ok(bytes) => {
                    report.files_restored += 1;
                    report.bytes_restored += bytes;
                }
                Err(e) => {
                    report.errors.push(RestoreError {
                        path: entry.path.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }

        Ok(report)
    }

    /// Restore a single file from a snapshot.
    pub async fn restore_file(
        &self,
        snapshot: &Snapshot,
        entry: &FileEntry,
        options: &RestoreOptions,
        report: &mut RestoreReport,
    ) -> VaultResult<u64> {
        let target_path = options.target.join(&entry.path);

        // Resolve conflict
        let final_path = match self.resolve_conflict(&target_path, options.conflict) {
            Ok(path) => path,
            Err(e) => {
                report.files_skipped += 1;
                return Err(e);
            }
        };

        // Ensure parent directory exists
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VaultError::RestoreFailed(format!("Failed to create parent directory: {}", e))
            })?;
        }

        // Retrieve data from storage
        let mut data = self.storage.retrieve_file(&snapshot.id, &entry.path)?;

        // Decrypt if necessary
        if self.crypto.is_some() {
            data = self.crypto.as_ref().unwrap().decrypt(&data).map_err(|e| {
                VaultError::RestoreFailed(format!("Decryption failed for {}: {}", entry.path, e))
            })?;
        }

        // Verify checksum if requested
        if options.verify_checksums {
            let checksum = Checksum::compute(&data, HashAlgorithm::Sha256);
            if checksum.value != entry.checksum {
                report.checksum_failures += 1;
                return Err(VaultError::ChecksumMismatch {
                    path: entry.path.clone(),
                    expected: entry.checksum.clone(),
                    actual: checksum.value,
                });
            }
        }

        // Write to target
        let bytes = data.len() as u64;
        std::fs::write(&final_path, &data).map_err(|e| {
            VaultError::RestoreFailed(format!("Failed to write file {}: {}", final_path.display(), e))
        })?;

        // Preserve timestamps if requested
        if options.preserve_timestamps {
            if let Some(_mtime) = chrono::DateTime::from_timestamp(entry.mtime, 0) {
                let filetime = filetime::FileTime::from_unix_time(entry.mtime, 0);
                filetime::set_file_mtime(&final_path, filetime).ok();
            }
        }

        // Track if file was renamed
        if final_path != target_path {
            report.files_renamed += 1;
        }

        Ok(bytes)
    }

    /// Resolve file conflict based on strategy.
    fn resolve_conflict(
        &self,
        target_path: &Path,
        strategy: ConflictStrategy,
    ) -> VaultResult<PathBuf> {
        if !target_path.exists() {
            return Ok(target_path.to_path_buf());
        }

        match strategy {
            ConflictStrategy::Skip => {
                return Err(VaultError::RestoreFailed(format!(
                    "File already exists (skip): {}",
                    target_path.display()
                )));
            }
            ConflictStrategy::Overwrite => Ok(target_path.to_path_buf()),
            ConflictStrategy::Rename => {
                let mut counter = 1;
                let stem = target_path.file_stem().unwrap_or_default().to_string_lossy();
                let ext = target_path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                let parent = target_path.parent().unwrap_or(Path::new("."));
                
                loop {
                    let new_name = format!("{}{}.restored{}", stem, counter, ext);
                    let new_path = parent.join(&new_name);
                    if !new_path.exists() {
                        return Ok(new_path);
                    }
                    counter += 1;
                    if counter > 1000 {
                        return Err(VaultError::RestoreFailed("Too many conflicting files".to_string()));
                    }
                }
            }
            ConflictStrategy::Fail => {
                return Err(VaultError::RestoreFailed(format!(
                    "File already exists (fail): {}",
                    target_path.display()
                )));
            }
        }
    }

    /// List all versions of a file across snapshots.
    pub fn list_versions(&self, file_path: &str) -> VaultResult<Vec<BackupEntry>> {
        let snapshots = self.storage.list_snapshots()?;
        let mut versions = Vec::new();

        for snapshot in snapshots {
            let snapshot_strategy = snapshot.strategy.clone();
            for entry in &snapshot.entries {
                if entry.path == file_path {
                    versions.push(BackupEntry {
                        snapshot_id: snapshot.id.clone(),
                        file_entry: entry.clone(),
                        created_at: snapshot.created_at,
                        strategy: snapshot_strategy.clone(),
                    });
                }
            }
        }

        // Sort by creation time, newest first
        versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(versions)
    }

    /// Find the most recent snapshot containing a specific file.
    pub fn find_latest_version(&self, file_path: &str) -> VaultResult<Option<BackupEntry>> {
        let versions = self.list_versions(file_path)?;
        Ok(versions.into_iter().next())
    }

    /// Restore a specific version of a file.
    pub async fn restore_version(
        &self,
        file_path: &str,
        snapshot_id: &str,
        options: RestoreOptions,
    ) -> VaultResult<RestoreReport> {
        let snapshots = self.storage.list_snapshots()?;
        let snapshot = snapshots
            .into_iter()
            .find(|s| s.id == snapshot_id)
            .ok_or_else(|| VaultError::SnapshotNotFound(snapshot_id.to_string()))?;

        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.path == file_path)
            .ok_or_else(|| {
                VaultError::RestoreFailed(format!("File {} not found in snapshot {}", file_path, snapshot_id))
            })?
            .clone();

        let mut report = RestoreReport::default();
        self.restore_file(&snapshot, &entry, &options, &mut report).await?;
        report.files_restored = 1;

        Ok(report)
    }

    /// Verify integrity of a snapshot.
    pub async fn verify(&self, snapshot: &Snapshot) -> VaultResult<VerifyReport> {
        let mut report = VerifyReport::default();

        for entry in &snapshot.entries {
            match self.verify_file(snapshot, entry).await {
                Ok(_) => report.files_ok += 1,
                Err(e) => {
                    report.files_failed += 1;
                    report.errors.push(VerifyError {
                        path: entry.path.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }

        Ok(report)
    }

    /// Verify a single file in a snapshot.
    pub async fn verify_file(
        &self,
        snapshot: &Snapshot,
        entry: &FileEntry,
    ) -> VaultResult<()> {
        let data = self.storage.retrieve_file(&snapshot.id, &entry.path)?;
        
        // Decrypt if necessary
        let data = if self.crypto.is_some() {
            self.crypto.as_ref().unwrap().decrypt(&data)?
        } else {
            data
        };

        // Verify checksum
        let checksum = Checksum::compute(&data, HashAlgorithm::Sha256);
        if checksum.value != entry.checksum {
            return Err(VaultError::ChecksumMismatch {
                path: entry.path.clone(),
                expected: entry.checksum.clone(),
                actual: checksum.value,
            });
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_strategy_default() {
        let strategy = ConflictStrategy::default();
        assert_eq!(strategy, ConflictStrategy::Overwrite);
    }

    #[test]
    fn test_restore_options_default() {
        let options = RestoreOptions::default();
        assert_eq!(options.target, PathBuf::from("."));
        assert_eq!(options.conflict, ConflictStrategy::Overwrite);
        assert!(options.verify_checksums);
        assert!(options.filter_paths.is_empty());
    }

    #[test]
    fn test_restore_options_builder() {
        let options = RestoreOptions::to("/target/dir")
            .skip_existing()
            .filter_files(vec!["file1.txt".to_string()]);
        
        assert_eq!(options.target, PathBuf::from("/target/dir"));
        assert_eq!(options.conflict, ConflictStrategy::Skip);
        assert_eq!(options.filter_paths, vec!["file1.txt"]);
    }

    #[test]
    fn test_restore_options_with_encryption() {
        let options = RestoreOptions::default()
            .with_encryption(EncryptionAlgorithm::Aes256Gcm, "base64key123".to_string());
        
        assert_eq!(options.encryption, Some(EncryptionAlgorithm::Aes256Gcm));
        assert_eq!(options.encryption_key, Some("base64key123".to_string()));
    }

    #[test]
    fn test_restore_report_success() {
        let mut report = RestoreReport::default();
        report.files_restored = 5;
        report.bytes_restored = 1024;
        
        assert!(report.is_success());
        assert!(report.summary().contains("Restored: 5"));
    }

    #[test]
    fn test_restore_report_with_errors() {
        let mut report = RestoreReport::default();
        report.files_restored = 4;
        report.checksum_failures = 1;
        report.errors.push(RestoreError {
            path: "failed.txt".to_string(),
            message: "IO error".to_string(),
        });
        
        assert!(!report.is_success());
        assert!(report.summary().contains("Checksum failures: 1"));
    }

    #[test]
    fn test_restore_error_display() {
        let error = RestoreError {
            path: "/test/file.txt".to_string(),
            message: "Permission denied".to_string(),
        };
        
        assert_eq!(error.to_string(), "/test/file.txt: Permission denied");
    }

    #[test]
    fn test_verify_report() {
        let mut report = VerifyReport::default();
        report.files_ok = 10;
        report.files_failed = 2;
        
        assert!(!report.is_ok());
        assert_eq!(report.summary(), "Verified: 10/12 OK");
    }

    #[test]
    fn test_encrypted_block_serde() {
        let block = EncryptedBlock {
            path: "test.txt".to_string(),
            original_size: 1024,
            checksum: "abc123".to_string(),
            checksum_algorithm: HashAlgorithm::Sha256,
            encryption_algorithm: EncryptionAlgorithm::Aes256Gcm,
        };
        
        let json = serde_json::to_string(&block).unwrap();
        let decoded: EncryptedBlock = serde_json::from_str(&json).unwrap();
        
        assert_eq!(decoded.path, "test.txt");
        assert_eq!(decoded.original_size, 1024);
    }

    #[test]
    fn test_conflict_strategy_all_variants() {
        // Test all strategies can be created
        let _ = ConflictStrategy::Skip;
        let _ = ConflictStrategy::Overwrite;
        let _ = ConflictStrategy::Rename;
        let _ = ConflictStrategy::Fail;
    }
}
