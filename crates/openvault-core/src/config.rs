use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{VaultError, VaultResult};
use crate::snapshot::BackupStrategy;

/// Top-level configuration loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Human-readable name for this backup task.
    pub name: String,
    /// Source directory to back up.
    pub source: PathBuf,
    /// Storage backend configuration.
    pub storage: StorageConfig,
    /// Backup strategy.
    pub strategy: BackupStrategy,
    /// Optional: patterns to exclude (glob).
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Optional: schedule expression (cron-like, for future use).
    pub schedule: Option<String>,
}

/// Configuration for the storage backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfig {
    /// Local filesystem storage.
    Local {
        /// Directory where backups are stored.
        path: PathBuf,
    },
    /// S3-compatible storage.
    S3 {
        bucket: String,
        prefix: String,
        endpoint: Option<String>,
        region: Option<String>,
        /// Access key ID for S3 authentication.
        access_key_id: Option<String>,
        /// Secret access key for S3 authentication.
        secret_access_key: Option<String>,
    },
    /// Cloudflare R2 storage (S3-compatible with custom endpoint).
    R2 {
        bucket: String,
        prefix: String,
        /// R2 account ID used to construct endpoint.
        account_id: String,
        /// Access key ID for R2 authentication.
        access_key_id: Option<String>,
        /// Secret access key for R2 authentication.
        secret_access_key: Option<String>,
    },
}

impl BackupConfig {
    /// Load configuration from a YAML file.
    pub fn load_from_file(path: &std::path::Path) -> VaultResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| VaultError::Config(format!("Failed to read config file: {}", e)))?;
        Self::load_from_str(&content)
    }

    /// Parse configuration from a YAML string.
    pub fn load_from_str(yaml: &str) -> VaultResult<Self> {
        serde_yaml::from_str(yaml)
            .map_err(|e| VaultError::Config(format!("Invalid YAML config: {}", e)))
    }

    /// Validate the configuration.
    pub fn validate(&self) -> VaultResult<()> {
        if !self.source.exists() {
            return Err(VaultError::Config(format!(
                "Source directory does not exist: {}",
                self.source.display()
            )));
        }
        if !self.source.is_dir() {
            return Err(VaultError::Config(format!(
                "Source path is not a directory: {}",
                self.source.display()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_local_config() {
        let yaml = r#"
name: "test-backup"
source: "/tmp/test-source"
strategy: "full"
storage:
  type: "local"
  path: "/tmp/test-vault"
exclude:
  - "*.tmp"
  - ".git"
"#;
        let config = BackupConfig::load_from_str(yaml).unwrap();
        assert_eq!(config.name, "test-backup");
        assert_eq!(config.strategy, BackupStrategy::Full);
        assert!(matches!(config.storage, StorageConfig::Local { .. }));
        assert_eq!(config.exclude.len(), 2);
    }

    #[test]
    fn test_parse_incremental_config() {
        let yaml = r#"
name: "incremental-backup"
source: "/tmp/data"
strategy: "incremental"
storage:
  type: "local"
  path: "/tmp/vault-data"
"#;
        let config = BackupConfig::load_from_str(yaml).unwrap();
        assert_eq!(config.strategy, BackupStrategy::Incremental);
    }

    #[test]
    fn test_parse_differential_config() {
        let yaml = r#"
name: "differential-backup"
source: "/tmp/data"
strategy: "differential"
storage:
  type: "local"
  path: "/tmp/vault-data"
"#;
        let config = BackupConfig::load_from_str(yaml).unwrap();
        assert_eq!(config.strategy, BackupStrategy::Differential);
    }

    #[test]
    fn test_parse_s3_config() {
        let yaml = r#"
name: "s3-backup"
source: "/tmp/data"
strategy: "full"
storage:
  type: "s3"
  bucket: "my-backups"
  prefix: "vault/"
  endpoint: "https://s3.amazonaws.com"
  region: "us-east-1"
  access_key_id: "AKIAIOSFODNN7EXAMPLE"
  secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
"#;
        let config = BackupConfig::load_from_str(yaml).unwrap();
        assert_eq!(config.name, "s3-backup");
        match config.storage {
            StorageConfig::S3 { bucket, region, .. } => {
                assert_eq!(bucket, "my-backups");
                assert_eq!(region, Some("us-east-1".to_string()));
            }
            _ => panic!("Expected S3 storage config"),
        }
    }

    #[test]
    fn test_parse_r2_config() {
        let yaml = r#"
name: "r2-backup"
source: "/tmp/data"
strategy: "full"
storage:
  type: "r2"
  bucket: "my-r2-bucket"
  prefix: "backups/"
  account_id: "abc123def456"
"#;
        let config = BackupConfig::load_from_str(yaml).unwrap();
        match config.storage {
            StorageConfig::R2 {
                bucket, account_id, ..
            } => {
                assert_eq!(bucket, "my-r2-bucket");
                assert_eq!(account_id, "abc123def456");
            }
            _ => panic!("Expected R2 storage config"),
        }
    }

    #[test]
    fn test_invalid_yaml() {
        let yaml = "not: valid: yaml: {{{";
        let result = BackupConfig::load_from_str(yaml);
        assert!(result.is_err());
    }
}
