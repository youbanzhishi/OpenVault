//! OpenVault Core Library
//!
//! Core abstractions, types, and backup engine for the OpenVault file backup system.

pub mod config;
pub mod crypto;
pub mod engine;
pub mod error;
pub mod integrity;
pub mod restore;
pub mod snapshot;
pub mod storage;
pub mod strategy;

pub use config::BackupConfig;
pub use crypto::{
    AesGcmEncryption, EncryptionAlgorithm, EncryptionProvider,
    EncryptionProviderFactory, Key256,
};
pub use engine::BackupEngine;
pub use error::{VaultError, VaultResult};
pub use integrity::{Checksum, HashAlgorithm, IntegrityEngine, IntegrityCheck, IntegrityReport};
pub use restore::{
    ConflictStrategy, RestoreEngine, RestoreError, RestoreOptions, RestoreReport,
    VerifyError, VerifyReport, EncryptedBlock,
};
pub use snapshot::{BackupEntry, BackupStrategy, FileEntry, Snapshot, SnapshotId};
pub use storage::VaultStorage;
