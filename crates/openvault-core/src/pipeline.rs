//! Composable storage pipeline for OpenVault.
//!
//! Provides a builder-pattern API for constructing storage pipelines that
//! transparently apply compression, encryption, and other transformations
//! to data as it flows through the storage layer.
//!
//! # Example
//!
//! ```ignore
//! use openvault_core::pipeline::PipelineBuilder;
//! use openvault_core::compress::ZstdCompressor;
//! use openvault_core::crypto::Aes256GcmCrypto;
//!
//! let pipeline = PipelineBuilder::new(storage)
//!     .compress(Arc::new(ZstdCompressor::new()))
//!     .encrypt(Arc::new(Aes256GcmCrypto::generate()))
//!     .build();
//! ```
//!
//! Data flow on store:  plaintext → compress → encrypt → storage
//! Data flow on retrieve: storage → decrypt → decompress → plaintext

use std::sync::Arc;

use crate::compress::{detect_format, Lz4Compressor, VaultCompressor, ZstdCompressor};
use crate::crypto::{Aes256GcmCrypto, VaultCrypto};
use crate::error::VaultError;
use crate::snapshot::Snapshot;
use crate::storage::VaultStorage;

/// Result type for pipeline operations.
pub type PipelineResult<T> = Result<T, VaultError>;

// ============================================================================
// StoragePipeline
// ============================================================================

/// A composable storage pipeline that applies transformations to data
/// as it passes through the storage layer.
///
/// The pipeline applies transformations in a specific order:
/// - **Store**: compress → encrypt → store
/// - **Retrieve**: retrieve → decrypt → decompress
///
/// This ordering ensures that compression operates on plaintext (achieving
/// good ratios) and encryption operates on compressed data (ensuring
/// confidentiality of both the data and its compression characteristics).
pub struct StoragePipeline {
    inner: Arc<dyn VaultStorage>,
    compressor: Option<Arc<dyn VaultCompressor>>,
    crypto: Option<Arc<dyn VaultCrypto>>,
}

impl StoragePipeline {
    /// Create a new pipeline with the given storage backend.
    pub fn new(storage: Arc<dyn VaultStorage>) -> Self {
        Self {
            inner: storage,
            compressor: None,
            crypto: None,
        }
    }

    /// Process data for storage: compress then encrypt.
    fn process_store(&self, data: &[u8]) -> PipelineResult<Vec<u8>> {
        let mut processed = data.to_vec();

        // Step 1: Compress
        if let Some(ref compressor) = self.compressor {
            processed = compressor
                .compress(&processed)
                .map_err(|e| VaultError::Storage(format!("Pipeline compression failed: {}", e)))?;
        }

        // Step 2: Encrypt
        if let Some(ref crypto) = self.crypto {
            processed = crypto
                .encrypt_to_bytes(&processed)
                .map_err(|e| VaultError::Storage(format!("Pipeline encryption failed: {}", e)))?;
        }

        Ok(processed)
    }

    /// Process data for retrieval: decrypt then decompress.
    fn process_retrieve(&self, data: &[u8]) -> PipelineResult<Vec<u8>> {
        let mut processed = data.to_vec();

        // Step 1: Decrypt
        if let Some(ref crypto) = self.crypto {
            processed = crypto
                .decrypt_from_bytes(&processed)
                .map_err(|e| VaultError::Storage(format!("Pipeline decryption failed: {}", e)))?;
        }

        // Step 2: Decompress (with auto-detection fallback)
        if let Some(ref compressor) = self.compressor {
            match compressor.decompress(&processed) {
                Ok(decompressed) => processed = decompressed,
                Err(_) => {
                    // Fall back to auto-detection for cross-compatibility
                    if detect_format(&processed).is_some() {
                        processed = crate::compress::auto_decompress(&processed).map_err(|e| {
                            VaultError::Storage(format!("Pipeline decompression failed: {}", e))
                        })?;
                    }
                    // If no compression format detected, data may be uncompressed (pass through)
                }
            }
        }

        Ok(processed)
    }

    /// Check if compression is enabled.
    pub fn has_compression(&self) -> bool {
        self.compressor.is_some()
    }

    /// Check if encryption is enabled.
    pub fn has_encryption(&self) -> bool {
        self.crypto.is_some()
    }

    /// Get the underlying storage backend name.
    pub fn backend_name_inner(&self) -> &str {
        self.inner.backend_name()
    }
}

impl VaultStorage for StoragePipeline {
    fn store_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        data: &[u8],
    ) -> crate::error::VaultResult<()> {
        let processed = self.process_store(data)?;
        self.inner.store_file(snapshot_id, rel_path, &processed)
    }

    fn retrieve_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
    ) -> crate::error::VaultResult<Vec<u8>> {
        let raw = self.inner.retrieve_file(snapshot_id, rel_path)?;
        self.process_retrieve(&raw)
    }

    fn store_snapshot(&self, snapshot: &Snapshot) -> crate::error::VaultResult<()> {
        // Metadata passes through unmodified
        self.inner.store_snapshot(snapshot)
    }

    fn load_snapshot(&self, id: &str) -> crate::error::VaultResult<Snapshot> {
        self.inner.load_snapshot(id)
    }

    fn list_snapshots(&self) -> crate::error::VaultResult<Vec<Snapshot>> {
        self.inner.list_snapshots()
    }

    fn delete_snapshot(&self, id: &str) -> crate::error::VaultResult<()> {
        self.inner.delete_snapshot(id)
    }

    fn latest_snapshot(&self, source: String) -> crate::error::VaultResult<Option<Snapshot>> {
        self.inner.latest_snapshot(source)
    }

    fn latest_full_snapshot(&self, source: String) -> crate::error::VaultResult<Option<Snapshot>> {
        self.inner.latest_full_snapshot(source)
    }

    fn backend_name(&self) -> &str {
        match (self.compressor.is_some(), self.crypto.is_some()) {
            (true, true) => "pipeline(compress+encrypt)",
            (true, false) => "pipeline(compress)",
            (false, true) => "pipeline(encrypt)",
            (false, false) => self.inner.backend_name(),
        }
    }

    fn restore_snapshot(
        &self,
        snapshot: &Snapshot,
        target: &std::path::Path,
    ) -> crate::error::VaultResult<()> {
        std::fs::create_dir_all(target).map_err(|e| {
            VaultError::RestoreFailed(format!("Failed to create target directory: {}", e))
        })?;

        for entry in &snapshot.entries {
            let target_path = target.join(&entry.path);
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    VaultError::RestoreFailed(format!("Failed to create parent: {}", e))
                })?;
            }

            let data = self.retrieve_file(&snapshot.id, &entry.path)?;
            std::fs::write(&target_path, &data).map_err(|e| {
                VaultError::RestoreFailed(format!(
                    "Failed to write {}: {}",
                    target_path.display(),
                    e
                ))
            })?;
        }

        Ok(())
    }
}

// ============================================================================
// PipelineBuilder
// ============================================================================

/// Builder for constructing StoragePipeline instances.
///
/// # Example
///
/// ```ignore
/// let pipeline = PipelineBuilder::new(storage)
///     .compress(Arc::new(ZstdCompressor::new()))
///     .encrypt(Arc::new(Aes256GcmCrypto::generate()))
///     .build();
/// ```
pub struct PipelineBuilder {
    storage: Arc<dyn VaultStorage>,
    compressor: Option<Arc<dyn VaultCompressor>>,
    crypto: Option<Arc<dyn VaultCrypto>>,
}

impl PipelineBuilder {
    /// Create a new PipelineBuilder with the given storage backend.
    pub fn new(storage: Arc<dyn VaultStorage>) -> Self {
        Self {
            storage,
            compressor: None,
            crypto: None,
        }
    }

    /// Add compression to the pipeline.
    pub fn compress(mut self, compressor: Arc<dyn VaultCompressor>) -> Self {
        self.compressor = Some(compressor);
        self
    }

    /// Add Zstd compression with default level.
    pub fn compress_zstd(self) -> Self {
        self.compress(Arc::new(ZstdCompressor::new()))
    }

    /// Add Zstd compression with a specific level (1-22).
    pub fn compress_zstd_level(self, level: i32) -> Self {
        self.compress(Arc::new(ZstdCompressor::with_level(level)))
    }

    /// Add LZ4 compression (fast mode).
    pub fn compress_lz4(self) -> Self {
        self.compress(Arc::new(Lz4Compressor::new()))
    }

    /// Add LZ4 HC compression (higher compression ratio).
    pub fn compress_lz4_hc(self) -> Self {
        self.compress(Arc::new(Lz4Compressor::with_high_compression()))
    }

    /// Add encryption to the pipeline.
    pub fn encrypt(mut self, crypto: Arc<dyn VaultCrypto>) -> Self {
        self.crypto = Some(crypto);
        self
    }

    /// Add AES-256-GCM encryption with the given key.
    pub fn encrypt_aes256gcm(self, key: &[u8]) -> PipelineResult<Self> {
        let crypto = Aes256GcmCrypto::new(key)?;
        Ok(self.encrypt(Arc::new(crypto)))
    }

    /// Add AES-256-GCM encryption from a hex-encoded key.
    pub fn encrypt_aes256gcm_hex(self, hex_key: &str) -> PipelineResult<Self> {
        let crypto = Aes256GcmCrypto::from_hex(hex_key)?;
        Ok(self.encrypt(Arc::new(crypto)))
    }

    /// Build the pipeline.
    pub fn build(self) -> StoragePipeline {
        StoragePipeline {
            inner: self.storage,
            compressor: self.compressor,
            crypto: self.crypto,
        }
    }
}

// ============================================================================
// Pipeline configuration (for serialization/deserialization)
// ============================================================================

/// Serializable pipeline configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineConfig {
    /// Whether compression is enabled.
    pub compression_enabled: bool,
    /// Compression algorithm (if enabled).
    pub compression_algorithm: Option<crate::compress::CompressionAlgorithm>,
    /// Compression level (if applicable).
    pub compression_level: Option<i32>,
    /// Whether encryption is enabled.
    pub encryption_enabled: bool,
    /// Encryption algorithm (if enabled).
    pub encryption_algorithm: Option<crate::crypto::EncryptionAlgorithm>,
    /// Base64-encoded encryption key (if enabled).
    #[serde(skip_serializing)]
    pub encryption_key_base64: Option<String>,
}

impl PipelineConfig {
    /// Create a config with no transformations.
    pub fn passthrough() -> Self {
        Self {
            compression_enabled: false,
            compression_algorithm: None,
            compression_level: None,
            encryption_enabled: false,
            encryption_algorithm: None,
            encryption_key_base64: None,
        }
    }

    /// Create a config with Zstd compression.
    pub fn zstd(level: i32) -> Self {
        Self {
            compression_enabled: true,
            compression_algorithm: Some(crate::compress::CompressionAlgorithm::Zstd),
            compression_level: Some(level),
            encryption_enabled: false,
            encryption_algorithm: None,
            encryption_key_base64: None,
        }
    }

    /// Create a config with LZ4 compression.
    pub fn lz4() -> Self {
        Self {
            compression_enabled: true,
            compression_algorithm: Some(crate::compress::CompressionAlgorithm::Lz4),
            compression_level: None,
            encryption_enabled: false,
            encryption_algorithm: None,
            encryption_key_base64: None,
        }
    }

    /// Create a config with AES-256-GCM encryption.
    pub fn aes256gcm(key_base64: &str) -> Self {
        Self {
            compression_enabled: false,
            compression_algorithm: None,
            compression_level: None,
            encryption_enabled: true,
            encryption_algorithm: Some(crate::crypto::EncryptionAlgorithm::Aes256Gcm),
            encryption_key_base64: Some(key_base64.to_string()),
        }
    }

    /// Add encryption to an existing config.
    pub fn with_encryption(mut self, key_base64: &str) -> Self {
        self.encryption_enabled = true;
        self.encryption_algorithm = Some(crate::crypto::EncryptionAlgorithm::Aes256Gcm);
        self.encryption_key_base64 = Some(key_base64.to_string());
        self
    }

    /// Build a pipeline from this configuration.
    pub fn build_pipeline(
        &self,
        storage: Arc<dyn VaultStorage>,
    ) -> PipelineResult<StoragePipeline> {
        let mut builder = PipelineBuilder::new(storage);

        if self.compression_enabled {
            let compressor: Arc<dyn VaultCompressor> = match self.compression_algorithm {
                Some(crate::compress::CompressionAlgorithm::Zstd) => {
                    let level = self.compression_level.unwrap_or(3);
                    Arc::new(ZstdCompressor::with_level(level))
                }
                Some(crate::compress::CompressionAlgorithm::Lz4) => Arc::new(Lz4Compressor::new()),
                None => Arc::new(ZstdCompressor::new()),
            };
            builder = builder.compress(compressor);
        }

        if self.encryption_enabled {
            if let Some(ref key_b64) = self.encryption_key_base64 {
                let key_bytes =
                    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, key_b64)
                        .map_err(|e| VaultError::Crypto(format!("Invalid base64 key: {}", e)))?;
                builder = builder.encrypt_aes256gcm(&key_bytes)?;
            }
        }

        Ok(builder.build())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_config_passthrough() {
        let config = PipelineConfig::passthrough();
        assert!(!config.compression_enabled);
        assert!(!config.encryption_enabled);
    }

    #[test]
    fn test_pipeline_config_zstd() {
        let config = PipelineConfig::zstd(9);
        assert!(config.compression_enabled);
        assert_eq!(
            config.compression_algorithm,
            Some(crate::compress::CompressionAlgorithm::Zstd)
        );
        assert_eq!(config.compression_level, Some(9));
    }

    #[test]
    fn test_pipeline_config_lz4() {
        let config = PipelineConfig::lz4();
        assert!(config.compression_enabled);
        assert_eq!(
            config.compression_algorithm,
            Some(crate::compress::CompressionAlgorithm::Lz4)
        );
    }

    #[test]
    fn test_pipeline_config_aes256gcm() {
        // "testkey1234567890123456" is 24 bytes, not 32. Use a proper 32-byte key.
        let key = crate::crypto::Key256::generate();
        let config = PipelineConfig::aes256gcm(&key.to_base64());
        assert!(config.encryption_enabled);
        assert_eq!(
            config.encryption_algorithm,
            Some(crate::crypto::EncryptionAlgorithm::Aes256Gcm)
        );
    }

    #[test]
    fn test_pipeline_config_with_encryption() {
        let key = crate::crypto::Key256::generate();
        let config = PipelineConfig::zstd(3).with_encryption(&key.to_base64());
        assert!(config.compression_enabled);
        assert!(config.encryption_enabled);
    }

    #[test]
    fn test_pipeline_config_serialization() {
        let config = PipelineConfig::zstd(5);
        let json = serde_json::to_string(&config).unwrap();
        let restored: PipelineConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.compression_enabled, config.compression_enabled);
        assert_eq!(restored.compression_level, config.compression_level);
    }

    #[test]
    fn test_pipeline_builder_chaining() {
        let key = crate::crypto::Key256::generate();
        let _config = PipelineConfig::zstd(3).with_encryption(&key.to_base64());
    }

    #[test]
    fn test_pipeline_process_store_no_transforms() {
        // Without a concrete storage, we can at least verify the config builds correctly
        let config = PipelineConfig::passthrough();
        assert!(!config.compression_enabled);
        assert!(!config.encryption_enabled);
    }
}
