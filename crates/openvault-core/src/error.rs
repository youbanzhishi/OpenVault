use thiserror::Error;

/// Unified error type for OpenVault operations.
#[derive(Error, Debug)]
pub enum VaultError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Backup failed: {0}")]
    BackupFailed(String),

    #[error("Restore failed: {0}")]
    RestoreFailed(String),

    #[error("Checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_yaml::Error),

    #[error("Cryptography error: {0}")]
    Crypto(String),

    #[error("Integrity verification failed: {0}")]
    Integrity(String),
}

pub type VaultResult<T> = Result<T, VaultError>;
