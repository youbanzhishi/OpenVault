//! Self-healing mechanism for OpenVault.
//!
//! Provides automatic detection and repair of corrupted backup data by:
//! 1. Periodically verifying integrity of backed-up data
//! 2. Detecting corruption via checksum mismatches
//! 3. Automatically recovering from healthy replicas on other backends
//!
//! The healing process:
//! - Scan: Verify checksums of all files in a snapshot
//! - Detect: Identify files with checksum mismatches
//! - Recover: Copy healthy data from another storage backend
//! - Verify: Re-check the recovered data

use serde::{Deserialize, Serialize};

use crate::error::VaultResult;
use crate::integrity::{Checksum, HashAlgorithm};
use crate::snapshot::Snapshot;
use crate::storage::VaultStorage;

/// Configuration for the self-healing mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingConfig {
    /// Whether self-healing is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// How often to run integrity checks (in hours).
    #[serde(default = "default_check_interval_hours")]
    pub check_interval_hours: u32,

    /// Maximum number of files to heal in one run (0 = unlimited).
    #[serde(default)]
    pub max_heals_per_run: u32,

    /// Whether to verify after healing.
    #[serde(default = "default_verify_after")]
    pub verify_after_heal: bool,
}

fn default_enabled() -> bool { true }
fn default_check_interval_hours() -> u32 { 24 }
fn default_verify_after() -> bool { true }

impl Default for HealingConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            check_interval_hours: default_check_interval_hours(),
            max_heals_per_run: 0,
            verify_after_heal: default_verify_after(),
        }
    }
}

/// Result of a single file integrity check.
#[derive(Debug, Clone)]
pub struct FileCheckResult {
    /// File path within the snapshot.
    pub path: String,
    /// Whether the file passed the integrity check.
    pub healthy: bool,
    /// Expected checksum.
    pub expected_checksum: String,
    /// Actual checksum (if computed).
    pub actual_checksum: Option<String>,
    /// Error message if check failed.
    pub error: Option<String>,
}

/// Result of a healing scan on a snapshot.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// Snapshot ID that was scanned.
    pub snapshot_id: String,
    /// Total files scanned.
    pub files_scanned: u32,
    /// Files that passed integrity check.
    pub files_healthy: u32,
    /// Files that failed integrity check.
    pub files_corrupt: u32,
    /// Files that could not be checked (e.g., missing).
    pub files_missing: u32,
    /// Individual check results.
    pub checks: Vec<FileCheckResult>,
}

impl ScanResult {
    /// Whether all files are healthy.
    pub fn is_all_healthy(&self) -> bool {
        self.files_corrupt == 0 && self.files_missing == 0
    }

    /// Get summary string.
    pub fn summary(&self) -> String {
        format!(
            "Scan {}: {}/{} healthy, {} corrupt, {} missing",
            self.snapshot_id,
            self.files_healthy,
            self.files_scanned,
            self.files_corrupt,
            self.files_missing
        )
    }
}

/// Result of a healing operation.
#[derive(Debug, Clone, Default)]
pub struct HealingResult {
    /// Snapshot ID that was healed.
    pub snapshot_id: String,
    /// Number of files successfully healed.
    pub files_healed: u32,
    /// Number of files that could not be healed.
    pub files_failed: u32,
    /// Source backend used for recovery.
    pub recovery_source: Option<String>,
    /// Individual file healing results.
    pub file_results: Vec<FileHealingResult>,
}

impl HealingResult {
    /// Whether all corrupt files were healed.
    pub fn is_fully_healed(&self) -> bool {
        self.files_failed == 0 && self.files_healed > 0
    }

    /// Get summary string.
    pub fn summary(&self) -> String {
        format!(
            "Healing {}: {} files healed, {} failed (source: {})",
            self.snapshot_id,
            self.files_healed,
            self.files_failed,
            self.recovery_source.as_deref().unwrap_or("none")
        )
    }
}

/// Result of healing a single file.
#[derive(Debug, Clone)]
pub struct FileHealingResult {
    /// File path.
    pub path: String,
    /// Whether healing succeeded.
    pub success: bool,
    /// Error message if healing failed.
    pub error: Option<String>,
}

/// The self-healing engine.
pub struct HealingEngine;

impl HealingEngine {
    /// Scan a snapshot for corruption by verifying all file checksums.
    pub fn scan(
        storage: &dyn VaultStorage,
        snapshot: &Snapshot,
    ) -> VaultResult<ScanResult> {
        let mut result = ScanResult {
            snapshot_id: snapshot.id.clone(),
            ..Default::default()
        };

        for entry in &snapshot.entries {
            result.files_scanned += 1;

            match storage.retrieve_file(&snapshot.id, &entry.path) {
                Ok(data) => {
                    let checksum = Checksum::compute(&data, HashAlgorithm::Sha256);
                    if checksum.value() == entry.checksum {
                        result.files_healthy += 1;
                        result.checks.push(FileCheckResult {
                            path: entry.path.clone(),
                            healthy: true,
                            expected_checksum: entry.checksum.clone(),
                            actual_checksum: Some(checksum.value().to_string()),
                            error: None,
                        });
                    } else {
                        result.files_corrupt += 1;
                        result.checks.push(FileCheckResult {
                            path: entry.path.clone(),
                            healthy: false,
                            expected_checksum: entry.checksum.clone(),
                            actual_checksum: Some(checksum.value().to_string()),
                            error: Some(format!(
                                "Checksum mismatch: expected {}, got {}",
                                entry.checksum,
                                checksum.value()
                            )),
                        });
                    }
                }
                Err(e) => {
                    result.files_missing += 1;
                    result.checks.push(FileCheckResult {
                        path: entry.path.clone(),
                        healthy: false,
                        expected_checksum: entry.checksum.clone(),
                        actual_checksum: None,
                        error: Some(format!("File missing or unreadable: {}", e)),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Heal corrupt files by recovering from a healthy replica on another storage.
    ///
    /// The healing process:
    /// 1. Scan the target storage for corrupt files
    /// 2. For each corrupt file, retrieve the healthy version from the source storage
    /// 3. Write the healthy data to the target storage
    /// 4. Optionally verify the healed data
    pub fn heal(
        target_storage: &dyn VaultStorage,
        source_storage: &dyn VaultStorage,
        snapshot: &Snapshot,
        config: &HealingConfig,
    ) -> VaultResult<HealingResult> {
        let scan = Self::scan(target_storage, snapshot)?;

        let mut result = HealingResult {
            snapshot_id: snapshot.id.clone(),
            recovery_source: Some(source_storage.backend_name().to_string()),
            ..Default::default()
        };

        if scan.is_all_healthy() {
            return Ok(result);
        }

        let mut heals_this_run = 0u32;

        for check in &scan.checks {
            if check.healthy {
                continue;
            }

            // Check max heals limit
            if config.max_heals_per_run > 0 && heals_this_run >= config.max_heals_per_run {
                break;
            }

            // Try to recover from source
            match source_storage.retrieve_file(&snapshot.id, &check.path) {
                Ok(healthy_data) => {
                    // Verify source data against expected checksum
                    let source_checksum = Checksum::compute(&healthy_data, HashAlgorithm::Sha256);
                    if source_checksum.value() != check.expected_checksum {
                        result.files_failed += 1;
                        result.file_results.push(FileHealingResult {
                            path: check.path.clone(),
                            success: false,
                            error: Some(format!(
                                "Source data also corrupt: expected {}, got {}",
                                check.expected_checksum,
                                source_checksum.value()
                            )),
                        });
                        continue;
                    }

                    // Write healthy data to target
                    match target_storage.store_file(&snapshot.id, &check.path, &healthy_data) {
                        Ok(()) => {
                            // Verify after heal if configured
                            if config.verify_after_heal {
                                match target_storage.retrieve_file(&snapshot.id, &check.path) {
                                    Ok(recovered) => {
                                        let verify_checksum =
                                            Checksum::compute(&recovered, HashAlgorithm::Sha256);
                                        if verify_checksum.value() == check.expected_checksum {
                                            result.files_healed += 1;
                                            heals_this_run += 1;
                                            result.file_results.push(FileHealingResult {
                                                path: check.path.clone(),
                                                success: true,
                                                error: None,
                                            });
                                        } else {
                                            result.files_failed += 1;
                                            result.file_results.push(FileHealingResult {
                                                path: check.path.clone(),
                                                success: false,
                                                error: Some(
                                                    "Post-heal verification failed".to_string(),
                                                ),
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        result.files_failed += 1;
                                        result.file_results.push(FileHealingResult {
                                            path: check.path.clone(),
                                            success: false,
                                            error: Some(format!(
                                                "Post-heal read failed: {}",
                                                e
                                            )),
                                        });
                                    }
                                }
                            } else {
                                result.files_healed += 1;
                                heals_this_run += 1;
                                result.file_results.push(FileHealingResult {
                                    path: check.path.clone(),
                                    success: true,
                                    error: None,
                                });
                            }
                        }
                        Err(e) => {
                            result.files_failed += 1;
                            result.file_results.push(FileHealingResult {
                                path: check.path.clone(),
                                success: false,
                                error: Some(format!("Failed to write healed data: {}", e)),
                            });
                        }
                    }
                }
                Err(e) => {
                    result.files_failed += 1;
                    result.file_results.push(FileHealingResult {
                        path: check.path.clone(),
                        success: false,
                        error: Some(format!("Source data unavailable: {}", e)),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Heal corrupt files by trying multiple source backends in order.
    ///
    /// Tries each source storage in order until one provides healthy data.
    /// This is useful for 3-2-1 strategies where multiple replicas exist.
    pub fn heal_from_sources(
        target_storage: &dyn VaultStorage,
        source_storages: &[&dyn VaultStorage],
        snapshot: &Snapshot,
        config: &HealingConfig,
    ) -> VaultResult<HealingResult> {
        let scan = Self::scan(target_storage, snapshot)?;

        let mut result = HealingResult {
            snapshot_id: snapshot.id.clone(),
            recovery_source: None,
            ..Default::default()
        };

        if scan.is_all_healthy() {
            return Ok(result);
        }

        let mut heals_this_run = 0u32;

        for check in &scan.checks {
            if check.healthy {
                continue;
            }

            if config.max_heals_per_run > 0 && heals_this_run >= config.max_heals_per_run {
                break;
            }

            // Try each source in order
            let mut healed = false;
            for source in source_storages {
                match source.retrieve_file(&snapshot.id, &check.path) {
                    Ok(healthy_data) => {
                        // Verify source data
                        let source_checksum = Checksum::compute(&healthy_data, HashAlgorithm::Sha256);
                        if source_checksum.value() != check.expected_checksum {
                            continue; // Try next source
                        }

                        // Write healthy data to target
                        match target_storage.store_file(&snapshot.id, &check.path, &healthy_data) {
                            Ok(()) => {
                                // Verify after heal
                                if config.verify_after_heal {
                                    match target_storage.retrieve_file(&snapshot.id, &check.path) {
                                        Ok(recovered) => {
                                            let verify_checksum =
                                                Checksum::compute(&recovered, HashAlgorithm::Sha256);
                                            if verify_checksum.value() == check.expected_checksum {
                                                result.files_healed += 1;
                                                heals_this_run += 1;
                                                result.recovery_source =
                                                    Some(source.backend_name().to_string());
                                                result.file_results.push(FileHealingResult {
                                                    path: check.path.clone(),
                                                    success: true,
                                                    error: None,
                                                });
                                                healed = true;
                                                break;
                                            }
                                        }
                                        Err(_) => continue,
                                    }
                                } else {
                                    result.files_healed += 1;
                                    heals_this_run += 1;
                                    result.recovery_source = Some(source.backend_name().to_string());
                                    result.file_results.push(FileHealingResult {
                                        path: check.path.clone(),
                                        success: true,
                                        error: None,
                                    });
                                    healed = true;
                                    break;
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                    Err(_) => continue,
                }
            }

            if !healed {
                result.files_failed += 1;
                result.file_results.push(FileHealingResult {
                    path: check.path.clone(),
                    success: false,
                    error: Some("No healthy source available".to_string()),
                });
            }
        }

        Ok(result)
    }

    /// Scan all snapshots in a storage for corruption.
    pub fn scan_all(storage: &dyn VaultStorage) -> VaultResult<Vec<ScanResult>> {
        let snapshots = storage.list_snapshots()?;
        let mut results = Vec::new();

        for snapshot in &snapshots {
            let scan = Self::scan(storage, snapshot)?;
            results.push(scan);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{BackupStrategy, FileEntry, Snapshot};

    fn make_snapshot(id: &str, source: &str, entries: Vec<FileEntry>) -> Snapshot {
        let mut snap = Snapshot::new(BackupStrategy::Full, source.to_string(), "local".into(), None);
        snap.id = id.to_string();
        for e in entries {
            snap.add_entry(e);
        }
        snap
    }

    fn compute_checksum(data: &[u8]) -> String {
        Checksum::compute(data, HashAlgorithm::Sha256).value().to_string()
    }

    #[test]
    fn test_healing_config_default() {
        let config = HealingConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_hours, 24);
        assert!(config.verify_after_heal);
    }

    #[test]
    fn test_healing_config_custom() {
        let config = HealingConfig {
            enabled: false,
            check_interval_hours: 12,
            max_heals_per_run: 10,
            verify_after_heal: false,
        };
        assert!(!config.enabled);
        assert_eq!(config.check_interval_hours, 12);
        assert_eq!(config.max_heals_per_run, 10);
        assert!(!config.verify_after_heal);
    }

    #[test]
    fn test_scan_result_summary() {
        let result = ScanResult {
            snapshot_id: "snap-001".to_string(),
            files_scanned: 10,
            files_healthy: 8,
            files_corrupt: 1,
            files_missing: 1,
            checks: vec![],
        };
        assert!(!result.is_all_healthy());
        assert!(result.summary().contains("8/10 healthy"));
    }

    #[test]
    fn test_scan_result_all_healthy() {
        let result = ScanResult {
            snapshot_id: "snap-002".to_string(),
            files_scanned: 5,
            files_healthy: 5,
            files_corrupt: 0,
            files_missing: 0,
            checks: vec![],
        };
        assert!(result.is_all_healthy());
    }

    #[test]
    fn test_healing_result_summary() {
        let result = HealingResult {
            snapshot_id: "snap-001".to_string(),
            files_healed: 3,
            files_failed: 0,
            recovery_source: Some("s3".to_string()),
            file_results: vec![],
        };
        assert!(result.is_fully_healed());
        assert!(result.summary().contains("3 files healed"));
    }

    #[test]
    fn test_file_check_result_healthy() {
        let check = FileCheckResult {
            path: "test.txt".to_string(),
            healthy: true,
            expected_checksum: "abc".to_string(),
            actual_checksum: Some("abc".to_string()),
            error: None,
        };
        assert!(check.healthy);
        assert!(check.error.is_none());
    }

    #[test]
    fn test_file_check_result_corrupt() {
        let check = FileCheckResult {
            path: "test.txt".to_string(),
            healthy: false,
            expected_checksum: "abc".to_string(),
            actual_checksum: Some("def".to_string()),
            error: Some("Checksum mismatch".to_string()),
        };
        assert!(!check.healthy);
        assert!(check.error.is_some());
    }

    #[test]
    fn test_file_healing_result() {
        let result = FileHealingResult {
            path: "test.txt".to_string(),
            success: true,
            error: None,
        };
        assert!(result.success);

        let failed = FileHealingResult {
            path: "test.txt".to_string(),
            success: false,
            error: Some("Source unavailable".to_string()),
        };
        assert!(!failed.success);
    }
}
