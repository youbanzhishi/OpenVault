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
//!
//! # Phase 7 Features
//!
//! - **Search**: `FileIndex`, `TextExtractor`, `KeywordSearch`, `SemanticSearch` trait
//! - **NL Restore**: `NaturalLanguageQuery` for natural language restore queries
//!
//! # Phase 8 Features
//!
//! - **Audit**: `AuditLog`, `AuditEntry`, `AuditQuery` — tamper-proof audit logging
//! - **Compliance**: `ComplianceRule`, `ComplianceChecker`, `RetentionManager` — compliance & retention
//! - **Tenant**: `Tenant`, `TenantManager`, `AccessControl` — multi-tenant & RBAC
//! - **Notification**: `NotificationSvc`, `NotificationRule` — notification with dedup

pub mod audit;
pub mod bench;
pub mod compliance;
pub mod config;
pub mod crypto;
pub mod engine;
pub mod error;
pub mod healing;
pub mod integrity;
pub mod notification;
pub mod policy;
pub mod replicator;
pub mod restore;
pub mod search;
pub mod snapshot;
pub mod storage;
pub mod strategy;
pub mod tenant;

#[cfg(feature = "compress")]
pub mod compress;

#[cfg(feature = "incremental")]
pub mod incremental;

#[cfg(feature = "pipeline")]
pub mod pipeline;

// Legacy re-exports (Phase 1-5)
pub use config::BackupConfig;
pub use crypto::{
    AesGcmEncryption, EncryptionAlgorithm, EncryptionProvider, EncryptionProviderFactory, Key256,
};
pub use engine::BackupEngine;
pub use error::{VaultError, VaultResult};
pub use healing::{HealingConfig, HealingEngine, HealingResult, ScanResult};
pub use integrity::{Checksum, HashAlgorithm, IntegrityCheck, IntegrityEngine, IntegrityReport};
pub use policy::{
    Policy321, PolicyEngine, PolicyHealth, PolicyViolation, RemediationAction, RemediationType,
    ViolationType,
};
pub use replicator::{
    HealthCheckResult, MaintenanceResult, ReplicationCoordinator, ReplicationResult,
    ReplicatorConfig,
};
pub use restore::{
    ConflictStrategy, EncryptedBlock, RestoreEngine, RestoreError, RestoreOptions, RestoreReport,
    VerifyError, VerifyReport,
};
pub use snapshot::{BackupEntry, BackupStrategy, FileEntry, Snapshot, SnapshotId};
pub use storage::VaultStorage;

// Phase 6 re-exports
#[cfg(feature = "crypto-advanced")]
pub use crypto::{
    Aes256GcmCrypto, DataKeyInfo, EncryptedData, EncryptedStorage, KeyDerivation, KeyManager,
    VaultCrypto,
};

#[cfg(feature = "compress")]
pub use compress::{
    auto_decompress, compress_with_stats, detect_format, CompressedStorage, CompressionAlgorithm,
    CompressionStats, Lz4Compressor, VaultCompressor, ZstdCompressor, LZ4_MAGIC, ZSTD_MAGIC,
};

#[cfg(feature = "incremental")]
pub use incremental::{
    BackupManifest, BackupReport, BackupRestore, Chunk, ChunkRef, ChunkStore, ChunkStoreMeta,
    Chunker, FileChunks, IncrementalBackup, RestoreReport as IncrementalRestoreReport,
};

#[cfg(feature = "pipeline")]
pub use pipeline::{PipelineBuilder, PipelineConfig, StoragePipeline};

// Phase 7 re-exports
pub use restore::NaturalLanguageQuery;
pub use search::{
    ExtractedText, FileIndex, FileIndexEntry, KeywordSemanticSearch, SearchResult, SemanticSearch,
    TextExtractor,
};

// Phase 8 re-exports
pub use audit::{
    AuditEntry, AuditLog, AuditOperation, AuditQuery, AuditQueryResult, AuditResult, RotationConfig,
};
pub use compliance::{
    ComplianceChecker, ComplianceFinding, ComplianceReport, ComplianceRule, ComplianceStatus,
    DataClassification, FindingSeverity, RetentionManager, RetentionPolicy, RetentionRecord,
    RetentionSweepResult,
};
pub use notification::{
    Channel, Notification as NotificationRecord, NotificationRule, NotificationSvc,
    NotificationType, Severity,
};
pub use tenant::{
    role_permissions, AccessControl, Permission, QuotaKind, QuotaResult, QuotaViolation, Role,
    Tenant, TenantManager, TenantQuota, TenantUsage, UserAccess,
};
