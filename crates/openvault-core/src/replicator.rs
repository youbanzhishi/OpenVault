//! Multi-backend replication coordinator for OpenVault.
//!
//! Implements the 3-2-1 backup strategy by coordinating replication
//! across multiple storage backends:
//! - 3 copies of data (1 production + 2 backups)
//! - 2 different storage media/types
//! - 1 copy offsite
//!
//! The replicator ensures snapshots are automatically distributed
//! to satisfy the 3-2-1 policy and can auto-remediate when copies
//! are missing or corrupted.

use crate::error::{VaultError, VaultResult};
use crate::healing::{HealingConfig, HealingEngine, ScanResult};
use crate::policy::{Policy321, PolicyEngine, PolicyHealth};
use crate::snapshot::Snapshot;
use crate::storage::VaultStorage;

/// Configuration for the replication coordinator.
#[derive(Debug, Clone)]
pub struct ReplicatorConfig {
    /// The 3-2-1 policy to enforce.
    pub policy: Policy321,
    /// Whether to auto-remediate on policy violations.
    pub auto_remediate: bool,
    /// Whether to verify replicas after replication.
    pub verify_after_replicate: bool,
}

impl Default for ReplicatorConfig {
    fn default() -> Self {
        Self {
            policy: Policy321::default(),
            auto_remediate: false,
            verify_after_replicate: true,
        }
    }
}

/// Result of a replication operation.
#[derive(Debug, Clone, Default)]
pub struct ReplicationResult {
    /// Snapshot ID that was replicated.
    pub snapshot_id: String,
    /// Number of backends successfully replicated to.
    pub backends_replicated: u32,
    /// Number of backends that failed.
    pub backends_failed: u32,
    /// Names of backends where replication succeeded.
    pub successful_backends: Vec<String>,
    /// Names of backends where replication failed.
    pub failed_backends: Vec<String>,
    /// Error details for failed backends.
    pub errors: Vec<(String, String)>,
}

impl ReplicationResult {
    /// Whether all replications succeeded.
    pub fn is_full_success(&self) -> bool {
        self.backends_failed == 0
    }

    /// Get summary string.
    pub fn summary(&self) -> String {
        format!(
            "Replication {}: {} backends OK, {} failed ({})",
            self.snapshot_id,
            self.backends_replicated,
            self.backends_failed,
            self.successful_backends.join(", ")
        )
    }
}

/// Result of a full health check across all backends.
#[derive(Debug, Clone, Default)]
pub struct HealthCheckResult {
    /// Policy health status.
    pub policy_health: PolicyHealth,
    /// Per-backend scan results (backend_name -> scan_result).
    pub scan_results: Vec<(String, ScanResult)>,
    /// Number of backends checked.
    pub backends_checked: u32,
    /// Number of backends with corruption.
    pub backends_with_corruption: u32,
}

impl HealthCheckResult {
    /// Whether all backends are healthy.
    pub fn is_healthy(&self) -> bool {
        self.policy_health.healthy && self.backends_with_corruption == 0
    }

    /// Get summary.
    pub fn summary(&self) -> String {
        let corruption_msg = if self.backends_with_corruption > 0 {
            format!(", {} backends with corruption", self.backends_with_corruption)
        } else {
            String::new()
        };
        format!(
            "Health: {} backends checked, policy={}{}",
            self.backends_checked,
            if self.policy_health.healthy { "OK" } else { "VIOLATED" },
            corruption_msg
        )
    }
}

/// The replication coordinator.
///
/// Manages snapshot replication across multiple storage backends
/// to satisfy the 3-2-1 backup policy.
pub struct ReplicationCoordinator {
    config: ReplicatorConfig,
}

impl ReplicationCoordinator {
    /// Create a new replication coordinator.
    pub fn new(config: ReplicatorConfig) -> Self {
        Self { config }
    }

    /// Create with the default 3-2-1 policy.
    pub fn with_default_policy() -> Self {
        Self::new(ReplicatorConfig::default())
    }

    /// Create with strict 3-2-1 policy and auto-remediation.
    pub fn strict_with_auto_remediate() -> Self {
        Self::new(ReplicatorConfig {
            policy: Policy321::strict(),
            auto_remediate: true,
            verify_after_replicate: true,
        })
    }

    /// Replicate a snapshot to all target backends that don't already have it.
    ///
    /// The source storage is used as the primary copy; all other storages
    /// receive a replica. If a target already has the snapshot, it is skipped.
    pub fn replicate_snapshot(
        &self,
        snapshot: &Snapshot,
        source: &dyn VaultStorage,
        targets: &[&dyn VaultStorage],
    ) -> VaultResult<ReplicationResult> {
        let mut result = ReplicationResult {
            snapshot_id: snapshot.id.clone(),
            ..Default::default()
        };

        for target in targets {
            let backend_name = target.backend_name().to_string();

            // Check if target already has this snapshot
            match target.load_snapshot(&snapshot.id) {
                Ok(_) => {
                    // Already exists, skip
                    result.successful_backends.push(format!("{} (existing)", backend_name));
                    result.backends_replicated += 1;
                    continue;
                }
                Err(_) => {
                    // Doesn't exist, replicate
                }
            }

            // Copy snapshot metadata
            if let Err(e) = target.store_snapshot(snapshot) {
                result.backends_failed += 1;
                result.failed_backends.push(backend_name.clone());
                result.errors.push((backend_name.clone(), format!("Failed to store snapshot metadata: {}", e)));
                continue;
            }

            // Copy all file data
            let mut copy_ok = true;
            for entry in &snapshot.entries {
                match source.retrieve_file(&snapshot.id, &entry.path) {
                    Ok(data) => {
                        if let Err(e) = target.store_file(&snapshot.id, &entry.path, &data) {
                            result.errors.push((
                                backend_name.clone(),
                                format!("Failed to store file {}: {}", entry.path, e),
                            ));
                            copy_ok = false;
                            break;
                        }
                    }
                    Err(e) => {
                        result.errors.push((
                            backend_name.clone(),
                            format!("Failed to read file {} from source: {}", entry.path, e),
                        ));
                        copy_ok = false;
                        break;
                    }
                }
            }

            if copy_ok {
                // Verify after replicate if configured
                if self.config.verify_after_replicate {
                    let scan = HealingEngine::scan(target, snapshot)?;
                    if !scan.is_all_healthy() {
                        result.backends_failed += 1;
                        result.failed_backends.push(format!("{} (verify failed)", backend_name));
                        continue;
                    }
                }
                result.backends_replicated += 1;
                result.successful_backends.push(backend_name);
            } else {
                result.backends_failed += 1;
                result.failed_backends.push(backend_name);
            }
        }

        Ok(result)
    }

    /// Check the health of all backends for a given source.
    ///
    /// This performs:
    /// 1. Policy health check (3-2-1 compliance)
    /// 2. Integrity scan on each backend
    pub fn check_health(
        &self,
        source: &str,
        storages: &[&dyn VaultStorage],
    ) -> VaultResult<HealthCheckResult> {
        let engine = PolicyEngine::new(self.config.policy.clone());
        let policy_health = engine.check_health(source, storages)?;

        let mut result = HealthCheckResult {
            policy_health,
            backends_checked: storages.len() as u32,
            ..Default::default()
        };

        for storage in storages {
            let backend_name = storage.backend_name().to_string();
            match storage.latest_snapshot(source.to_string()) {
                Ok(Some(snapshot)) => {
                    let scan = HealingEngine::scan(storage, &snapshot)?;
                    if !scan.is_all_healthy() {
                        result.backends_with_corruption += 1;
                    }
                    result.scan_results.push((backend_name, scan));
                }
                Ok(None) => {
                    // No snapshot on this backend
                }
                Err(_) => {
                    // Backend unavailable
                }
            }
        }

        Ok(result)
    }

    /// Auto-remediate policy violations by replicating to additional backends.
    ///
    /// Returns the number of remediation actions performed.
    pub fn auto_remediate(
        &self,
        source: &str,
        source_storage: &dyn VaultStorage,
        target_storages: &[&dyn VaultStorage],
    ) -> VaultResult<u32> {
        let engine = PolicyEngine::new(self.config.policy.clone());
        let all_storages: Vec<&dyn VaultStorage> = std::iter::once(source_storage)
            .chain(target_storages.iter().copied())
            .collect();

        let health = engine.check_health(source, &all_storages)?;
        if health.healthy {
            return Ok(0);
        }

        // Find the latest snapshot from source
        let snapshot = source_storage
            .latest_snapshot(source.to_string())?
            .ok_or_else(|| VaultError::PolicyViolation("No snapshot found to remediate".to_string()))?;

        let mut actions = 0u32;

        // Copy to target storages that don't have it
        for target in target_storages {
            let target_has = target
                .latest_snapshot(source.to_string())
                .map(|opt| opt.is_some())
                .unwrap_or(false);

            if !target_has {
                match target.store_snapshot(&snapshot) {
                    Ok(_) => {
                        let mut all_ok = true;
                        for entry in &snapshot.entries {
                            if let Ok(data) = source_storage.retrieve_file(&snapshot.id, &entry.path) {
                                if target.store_file(&snapshot.id, &entry.path, &data).is_err() {
                                    all_ok = false;
                                    break;
                                }
                            }
                        }
                        if all_ok {
                            actions += 1;
                        }
                    }
                    Err(_) => continue,
                }
            }
        }

        Ok(actions)
    }

    /// Perform a full 3-2-1 compliance check and optionally remediate.
    ///
    /// This is the main entry point for scheduled maintenance:
    /// 1. Check policy compliance
    /// 2. Scan all backends for corruption
    /// 3. Auto-remediate if configured
    /// 4. Heal any corrupt data
    pub fn maintain_321(
        &self,
        source: &str,
        primary: &dyn VaultStorage,
        replicas: &[&dyn VaultStorage],
    ) -> VaultResult<MaintenanceResult> {
        // Step 1: Health check
        let all_storages: Vec<&dyn VaultStorage> = std::iter::once(primary)
            .chain(replicas.iter().copied())
            .collect();
        let health = self.check_health(source, &all_storages)?;

        let mut result = MaintenanceResult {
            policy_healthy: health.policy_health.healthy,
            backends_with_corruption: health.backends_with_corruption,
            remediation_actions: 0,
            healing_actions: 0,
            ..Default::default()
        };

        // Step 2: Auto-remediate if configured
        if self.config.auto_remediate && !health.policy_health.healthy {
            let actions = self.auto_remediate(source, primary, replicas)?;
            result.remediation_actions = actions;
        }

        // Step 3: Heal corrupt data on replicas
        for replica in replicas {
            if let Ok(Some(snap)) = replica.latest_snapshot(source.to_string()) {
                let scan = HealingEngine::scan(replica, &snap)?;
                if !scan.is_all_healthy() {
                    let healing_config = HealingConfig::default();
                    if let Ok(heal_result) = HealingEngine::heal(replica, primary, &snap, &healing_config) {
                        result.healing_actions += heal_result.files_healed;
                    }
                }
            }
        }

        result.summary = format!(
            "3-2-1 Maintenance: policy={}, corruption={}, remediated={}, healed={}",
            if result.policy_healthy { "OK" } else { "VIOLATED" },
            result.backends_with_corruption,
            result.remediation_actions,
            result.healing_actions
        );

        Ok(result)
    }
}

/// Result of a maintenance run.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceResult {
    /// Whether the 3-2-1 policy is satisfied.
    pub policy_healthy: bool,
    /// Number of backends with corruption.
    pub backends_with_corruption: u32,
    /// Number of remediation actions performed.
    pub remediation_actions: u32,
    /// Number of files healed.
    pub healing_actions: u32,
    /// Summary string.
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{BackupStrategy, FileEntry, Snapshot};

    #[test]
    fn test_replicator_config_default() {
        let config = ReplicatorConfig::default();
        assert!(!config.auto_remediate);
        assert!(config.verify_after_replicate);
    }

    #[test]
    fn test_replication_result_summary() {
        let result = ReplicationResult {
            snapshot_id: "snap-001".to_string(),
            backends_replicated: 2,
            backends_failed: 0,
            successful_backends: vec!["local".to_string(), "s3".to_string()],
            failed_backends: vec![],
            errors: vec![],
        };
        assert!(result.is_full_success());
        assert!(result.summary().contains("2 backends OK"));
    }

    #[test]
    fn test_health_check_result() {
        let result = HealthCheckResult {
            policy_health: crate::policy::PolicyHealth {
                source: "/data".to_string(),
                healthy: true,
                copies: 3,
                copies_required: 3,
                media_types: 2,
                media_types_required: 2,
                has_offsite: true,
                offsite_required: true,
                backend_names: vec!["local".to_string(), "s3".to_string()],
                violations: vec![],
                remediation: vec![],
            },
            scan_results: vec![],
            backends_checked: 3,
            backends_with_corruption: 0,
        };
        assert!(result.is_healthy());
        assert!(result.summary().contains("policy=OK"));
    }

    #[test]
    fn test_maintenance_result() {
        let result = MaintenanceResult {
            policy_healthy: true,
            backends_with_corruption: 0,
            remediation_actions: 0,
            healing_actions: 0,
            summary: "OK".to_string(),
        };
        assert!(result.policy_healthy);
    }
}
