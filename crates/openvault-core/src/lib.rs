//! OpenVault Core Library
//!
//! Core abstractions, types, and backup engine for the OpenVault file backup system.
//!
//! # Phase 6 Features
//!
//! - **Encryption**: `VaultCrypto` trait, `Aes256GcmCrypto`, `KeyDerivation` (PBKDF2),
//!   `KeyManager` (hierarchical keys), `EncryptedStorage` (transparent decorator)
//! - **Compression**: `VaultCompressor` trait, `ZstdCompressor`, `Lz4Compressor`,
//!   `CompressedStorage` (transparent decorator), auto format detection
//! - **Incremental**: Content-defined chunking (CDC), dedup chunk store,
//!   `IncrementalBackup`, `BackupManifest`, `BackupRestore`
//! - **Pipeline**: Composable `StoragePipeline` with builder pattern

pub mod config;
pub mod crypto;
pub mod engine;
pub mod error;
pub mod healing;
pub mod integrity;
pub mod policy;
pub mod restore;
pub mod snapshot;
pub mod storage;
pub mod replicator;
pub mod strategy;

#[cfg(feature = "compress")]
pub mod compress;

#[cfg(feature = "incremental")]
pub mod incremental;

#[cfg(feature = "pipeline")]
pub mod pipeline;

// Legacy re-exports (Phase 1-5)
pub use config::BackupConfig;
pub use crypto::{
    AesGcmEncryption, EncryptionAlgorithm, EncryptionProvider,
    EncryptionProviderFactory, Key256,
};
pub use engine::BackupEngine;
pub use error::{VaultError, VaultResult};
pub use healing::{HealingConfig, HealingEngine, HealingResult, ScanResult};
pub use integrity::{Checksum, HashAlgorithm, IntegrityEngine, IntegrityCheck, IntegrityReport};
pub use policy::{Policy321, PolicyEngine, PolicyHealth, PolicyViolation, ViolationType, RemediationAction, RemediationType};
pub use restore::{
    ConflictStrategy, RestoreEngine, RestoreError, RestoreOptions, RestoreReport,
    VerifyError, VerifyReport, EncryptedBlock,
};
pub use replicator::{ReplicatorConfig, ReplicationCoordinator, ReplicationResult, HealthCheckResult, MaintenanceResult};
pub use snapshot::{BackupEntry, BackupStrategy, FileEntry, Snapshot, SnapshotId};
pub use storage::VaultStorage;

// Phase 6 re-exports
#[cfg(feature = "crypto-advanced")]
pub use crypto::{VaultCrypto, Aes256GcmCrypto, EncryptedData, KeyDerivation, KeyManager, DataKeyInfo, EncryptedStorage};

#[cfg(feature = "compress")]
pub use compress::{
    VaultCompressor, ZstdCompressor, Lz4Compressor, CompressedStorage,
    CompressionAlgorithm, CompressionStats, compress_with_stats,
    auto_decompress, detect_format, ZSTD_MAGIC, LZ4_MAGIC,
};

#[cfg(feature = "incremental")]
pub use incremental::{
    Chunker, Chunk, ChunkStore, ChunkRef, ChunkStoreMeta,
    IncrementalBackup, BackupManifest, FileChunks, BackupReport,
    BackupRestore, RestoreReport as IncrementalRestoreReport,
};

#[cfg(feature = "pipeline")]
pub use pipeline::{
    StoragePipeline, PipelineBuilder, PipelineConfig,
};
