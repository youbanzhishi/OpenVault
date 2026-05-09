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
        /// Access key ID.
        access_key_id: Option<String>,
        /// Secret access key.
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
        // S3 backend: warn if credentials are missing (but allow it for dry-run)
        if let StorageConfig::S3 {
            access_key_id: None,
            secret_access_key: None,
            ..
        } = &self.storage
        {
            // Credentials are optional for interface testing
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
  bucket: "my-backup-bucket"
  prefix: "openvault/"
  endpoint: "https://s3.example.com"
  region: "us-east-1"
"#;
        let config = BackupConfig::load_from_str(yaml).unwrap();
        assert_eq!(config.name, "s3-backup");
        match config.storage {
            StorageConfig::S3 { bucket, prefix, endpoint, region, .. } => {
                assert_eq!(bucket, "my-backup-bucket");
                assert_eq!(prefix, "openvault/");
                assert_eq!(endpoint, Some("https://s3.example.com".to_string()));
                assert_eq!(region, Some("us-east-1".to_string()));
            }
            _ => panic!("Expected S3 config"),
        }
    }

    #[test]
    fn test_invalid_yaml() {
        let yaml = "not: valid: yaml: {{{";
        let result = BackupConfig::load_from_str(yaml);
        assert!(result.is_err());
    }
}
