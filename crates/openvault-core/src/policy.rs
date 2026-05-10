//! 3-2-1 backup policy engine for OpenVault.
//!
//! Implements the 3-2-1 backup rule:
//! - 3 copies of data (1 production + 2 backups)
//! - 2 different storage media/types
//! - 1 copy offsite
//!
//! The policy engine evaluates current backup state against the 3-2-1 rule
//! and can auto-remediate when the policy is violated.

use serde::{Deserialize, Serialize};

use crate::error::{VaultError, VaultResult};
use crate::storage::VaultStorage;

/// 3-2-1 backup policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy321 {
    /// Minimum number of copies required (default: 3).
    #[serde(default = "default_copies")]
    pub min_copies: u32,

    /// Minimum number of different storage types (default: 2).
    #[serde(default = "default_media_types")]
    pub min_media_types: u32,

    /// Whether at least one copy must be offsite (default: true).
    #[serde(default = "default_offsite")]
    pub require_offsite: bool,

    /// Storage backends considered "offsite".
    #[serde(default = "default_offsite_backends")]
    pub offsite_backends: Vec<String>,

    /// Whether to auto-remediate when policy is violated.
    #[serde(default)]
    pub auto_remediate: bool,

    /// Name of this policy.
    #[serde(default = "default_policy_name")]
    pub name: String,
}

fn default_copies() -> u32 {
    3
}
fn default_media_types() -> u32 {
    2
}
fn default_offsite() -> bool {
    true
}
fn default_offsite_backends() -> Vec<String> {
    vec!["s3".to_string(), "r2".to_string(), "openlink".to_string()]
}
fn default_policy_name() -> String {
    "3-2-1-default".to_string()
}

impl Default for Policy321 {
    fn default() -> Self {
        Self {
            min_copies: default_copies(),
            min_media_types: default_media_types(),
            require_offsite: default_offsite(),
            offsite_backends: default_offsite_backends(),
            auto_remediate: false,
            name: default_policy_name(),
        }
    }
}

impl Policy321 {
    /// Create a strict 3-2-1 policy.
    pub fn strict() -> Self {
        Self {
            min_copies: 3,
            min_media_types: 2,
            require_offsite: true,
            offsite_backends: default_offsite_backends(),
            auto_remediate: false,
            name: "3-2-1-strict".to_string(),
        }
    }

    /// Create a relaxed policy for development/testing.
    pub fn relaxed() -> Self {
        Self {
            min_copies: 1,
            min_media_types: 1,
            require_offsite: false,
            offsite_backends: vec![],
            auto_remediate: false,
            name: "3-2-1-relaxed".to_string(),
        }
    }
}

/// Health status of the 3-2-1 policy for a source.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyHealth {
    /// Source path being evaluated.
    pub source: String,
    /// Whether the policy is satisfied.
    pub healthy: bool,
    /// Current number of copies found.
    pub copies: u32,
    /// Required number of copies.
    pub copies_required: u32,
    /// Current number of distinct storage media.
    pub media_types: u32,
    /// Required number of media types.
    pub media_types_required: u32,
    /// Whether an offsite copy exists.
    pub has_offsite: bool,
    /// Whether offsite is required.
    pub offsite_required: bool,
    /// Storage backends where copies exist.
    pub backend_names: Vec<String>,
    /// List of violations.
    pub violations: Vec<PolicyViolation>,
    /// Suggested remediation actions.
    pub remediation: Vec<RemediationAction>,
}

impl PolicyHealth {
    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        if self.healthy {
            format!(
                "✅ Policy satisfied: {}/{} copies, {}/{} media types, offsite={}",
                self.copies,
                self.copies_required,
                self.media_types,
                self.media_types_required,
                if self.has_offsite { "yes" } else { "n/a" }
            )
        } else {
            let issues: Vec<String> = self.violations.iter().map(|v| v.message.clone()).collect();
            format!(
                "❌ Policy violated: {} copies, {} media types, offsite={}. Issues: {}",
                self.copies,
                self.media_types,
                if self.has_offsite { "yes" } else { "no" },
                issues.join("; ")
            )
        }
    }
}

/// A specific policy violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyViolation {
    /// Type of violation.
    pub violation_type: ViolationType,
    /// Human-readable message.
    pub message: String,
}

/// Types of policy violations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViolationType {
    InsufficientCopies,
    InsufficientMediaTypes,
    NoOffsiteCopy,
}

/// Suggested remediation action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationAction {
    /// Type of action.
    pub action_type: RemediationType,
    /// Description of the action.
    pub description: String,
    /// Suggested target backend.
    pub target_backend: Option<String>,
}

/// Types of remediation actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemediationType {
    CopyToBackend,
    CreateOffsiteCopy,
    CreateBackup,
}

/// The 3-2-1 policy evaluation engine.
pub struct PolicyEngine {
    policy: Policy321,
}

impl PolicyEngine {
    /// Create a new policy engine with the given policy.
    pub fn new(policy: Policy321) -> Self {
        Self { policy }
    }

    /// Get the current policy.
    pub fn policy(&self) -> &Policy321 {
        &self.policy
    }

    /// Check the health of a source against all provided storages.
    ///
    /// This evaluates the 3-2-1 policy by checking snapshots across
    /// all provided storage backends.
    pub fn check_health(
        &self,
        source: &str,
        storages: &[&dyn VaultStorage],
    ) -> VaultResult<PolicyHealth> {
        let mut backend_names: Vec<String> = Vec::new();
        let mut total_copies: u32 = 0;
        let mut has_offsite = false;

        for storage in storages {
            match storage.latest_snapshot(source.to_string()) {
                Ok(Some(_)) => {
                    total_copies += 1;
                    let backend = storage.backend_name().to_string();
                    if !backend_names.contains(&backend) {
                        backend_names.push(backend.clone());
                    }
                    if self.policy.offsite_backends.contains(&backend) {
                        has_offsite = true;
                    }
                }
                Ok(None) => {}
                Err(_) => {} // Storage unavailable, skip
            }
        }

        let media_types = backend_names.len() as u32;

        // Evaluate violations
        let mut violations = Vec::new();
        let mut remediation = Vec::new();

        if total_copies < self.policy.min_copies {
            violations.push(PolicyViolation {
                violation_type: ViolationType::InsufficientCopies,
                message: format!(
                    "Only {} copies exist, need {}",
                    total_copies, self.policy.min_copies
                ),
            });
            let missing = self.policy.min_copies - total_copies;
            remediation.push(RemediationAction {
                action_type: RemediationType::CreateBackup,
                description: format!("Create {} additional backup copies", missing),
                target_backend: None,
            });
        }

        if media_types < self.policy.min_media_types {
            violations.push(PolicyViolation {
                violation_type: ViolationType::InsufficientMediaTypes,
                message: format!(
                    "Only {} media types used, need {}",
                    media_types, self.policy.min_media_types
                ),
            });
            // Suggest a backend not already used
            let suggested = ["s3", "r2", "local"]
                .iter()
                .find(|b| !backend_names.contains(&b.to_string()))
                .map(|b| b.to_string());
            remediation.push(RemediationAction {
                action_type: RemediationType::CopyToBackend,
                description: format!(
                    "Add a backup on a different storage type (suggested: {})",
                    suggested.as_deref().unwrap_or("any new backend")
                ),
                target_backend: suggested,
            });
        }

        if self.policy.require_offsite && !has_offsite {
            violations.push(PolicyViolation {
                violation_type: ViolationType::NoOffsiteCopy,
                message: "No offsite backup copy found".to_string(),
            });
            let suggested = self
                .policy
                .offsite_backends
                .iter()
                .find(|b| !backend_names.contains(b))
                .cloned();
            remediation.push(RemediationAction {
                action_type: RemediationType::CreateOffsiteCopy,
                description: format!(
                    "Create an offsite backup (suggested: {})",
                    suggested.as_deref().unwrap_or("s3 or r2")
                ),
                target_backend: suggested,
            });
        }

        let healthy = violations.is_empty();

        Ok(PolicyHealth {
            source: source.to_string(),
            healthy,
            copies: total_copies,
            copies_required: self.policy.min_copies,
            media_types,
            media_types_required: self.policy.min_media_types,
            has_offsite,
            offsite_required: self.policy.require_offsite,
            backend_names,
            violations,
            remediation,
        })
    }

    /// Auto-remediate policy violations by copying snapshots to additional backends.
    ///
    /// Returns the number of remediation actions performed.
    pub fn remediate(
        &self,
        source: &str,
        source_storage: &dyn VaultStorage,
        target_storages: &[&dyn VaultStorage],
    ) -> VaultResult<u32> {
        let health = self.check_health(
            source,
            &std::iter::once(source_storage)
                .chain(target_storages.iter().copied())
                .collect::<Vec<_>>(),
        )?;

        if health.healthy {
            return Ok(0);
        }

        let mut actions_performed = 0u32;

        // Find the latest snapshot from the source storage
        let snapshot = source_storage
            .latest_snapshot(source.to_string())?
            .ok_or_else(|| {
                VaultError::PolicyViolation("No snapshot found to remediate".to_string())
            })?;

        // Copy snapshot to target storages that don't have it
        for target in target_storages {
            let target_has = target
                .latest_snapshot(source.to_string())
                .map(|opt| opt.is_some())
                .unwrap_or(false);

            if !target_has {
                // Copy snapshot metadata
                target.store_snapshot(&snapshot)?;

                // Copy all files
                for entry in &snapshot.entries {
                    let data = source_storage.retrieve_file(&snapshot.id, &entry.path)?;
                    target.store_file(&snapshot.id, &entry.path, &data)?;
                }

                actions_performed += 1;
            }
        }

        Ok(actions_performed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{BackupStrategy, FileEntry, Snapshot};
    use tempfile::TempDir;

    fn make_snapshot(
        id: &str,
        source: &str,
        strategy: BackupStrategy,
        entries: Vec<FileEntry>,
    ) -> Snapshot {
        let mut snap = Snapshot::new(strategy, source.to_string(), "local".into(), None);
        snap.id = id.to_string();
        for e in entries {
            snap.add_entry(e);
        }
        snap
    }

    #[test]
    fn test_default_policy() {
        let policy = Policy321::default();
        assert_eq!(policy.min_copies, 3);
        assert_eq!(policy.min_media_types, 2);
        assert!(policy.require_offsite);
    }

    #[test]
    fn test_strict_policy() {
        let policy = Policy321::strict();
        assert_eq!(policy.min_copies, 3);
        assert_eq!(policy.min_media_types, 2);
        assert!(policy.require_offsite);
    }

    #[test]
    fn test_relaxed_policy() {
        let policy = Policy321::relaxed();
        assert_eq!(policy.min_copies, 1);
        assert_eq!(policy.min_media_types, 1);
        assert!(!policy.require_offsite);
    }

    #[test]
    fn test_policy_health_satisfied() {
        let health = PolicyHealth {
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
        };
        assert!(health.healthy);
        assert!(health.summary().contains("Policy satisfied"));
    }

    #[test]
    fn test_policy_health_violated() {
        let health = PolicyHealth {
            source: "/data".to_string(),
            healthy: false,
            copies: 1,
            copies_required: 3,
            media_types: 1,
            media_types_required: 2,
            has_offsite: false,
            offsite_required: true,
            backend_names: vec!["local".to_string()],
            violations: vec![PolicyViolation {
                violation_type: ViolationType::InsufficientCopies,
                message: "Only 1 copies exist, need 3".to_string(),
            }],
            remediation: vec![],
        };
        assert!(!health.healthy);
        assert!(health.summary().contains("Policy violated"));
    }

    #[test]
    fn test_relaxed_policy_engine() {
        let policy = Policy321::relaxed();
        let engine = PolicyEngine::new(policy);
        assert_eq!(engine.policy().min_copies, 1);
    }

    #[test]
    fn test_violation_types() {
        let v1 = PolicyViolation {
            violation_type: ViolationType::InsufficientCopies,
            message: "test".to_string(),
        };
        let v2 = PolicyViolation {
            violation_type: ViolationType::InsufficientMediaTypes,
            message: "test".to_string(),
        };
        let v3 = PolicyViolation {
            violation_type: ViolationType::NoOffsiteCopy,
            message: "test".to_string(),
        };
        assert_ne!(v1.violation_type, v2.violation_type);
        assert_ne!(v2.violation_type, v3.violation_type);
    }
}
