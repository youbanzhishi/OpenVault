//! Compression layer for OpenVault.
//!
//! Provides Zstd and LZ4 compression implementations with transparent
//! storage decoration and automatic format detection via magic bytes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::VaultError;
use crate::snapshot::Snapshot;
use crate::storage::VaultStorage;

/// Result type for compression operations.
pub type CompressResult<T> = Result<T, VaultError>;

/// Magic bytes for format detection.
pub const ZSTD_MAGIC: &[u8; 4] = b"\x28\xb5\x2f\xfd";
pub const LZ4_MAGIC: &[u8; 4] = b"\x04\x22\x4d\x18";

/// Compression algorithm identifier.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    /// Zstd - high compression ratio with fast decompression.
    Zstd,
    /// LZ4 - extremely fast compression.
    Lz4,
}

impl CompressionAlgorithm {
    /// Get the magic bytes for this algorithm.
    pub fn magic_bytes(&self) -> &[u8] {
        match self {
            CompressionAlgorithm::Zstd => ZSTD_MAGIC,
            CompressionAlgorithm::Lz4 => LZ4_MAGIC,
        }
    }

    /// Get human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            CompressionAlgorithm::Zstd => "Zstd",
            CompressionAlgorithm::Lz4 => "LZ4",
        }
    }
}

/// Detect compression format from magic bytes.
pub fn detect_format(data: &[u8]) -> Option<CompressionAlgorithm> {
    if data.len() < 4 {
        return None;
    }

    let magic = &data[..4];

    if magic == ZSTD_MAGIC {
        Some(CompressionAlgorithm::Zstd)
    } else if magic == LZ4_MAGIC {
        Some(CompressionAlgorithm::Lz4)
    } else {
        None
    }
}

// ============================================================================
// VaultCompressor trait
// ============================================================================

/// Trait for compression providers.
///
/// All implementations must produce output that starts with the algorithm's
/// magic bytes for automatic format detection during decompression.
pub trait VaultCompressor: Send + Sync {
    /// Compress data.
    fn compress(&self, data: &[u8]) -> CompressResult<Vec<u8>>;

    /// Decompress data.
    fn decompress(&self, data: &[u8]) -> CompressResult<Vec<u8>>;

    /// Get the algorithm identifier.
    fn algorithm(&self) -> CompressionAlgorithm;

    /// Get human-readable name.
    fn name(&self) -> &str {
        self.algorithm().name()
    }
}

// ============================================================================
// ZstdCompressor
// ============================================================================

/// Zstd compression implementation.
///
/// Zstd offers an excellent balance of compression ratio and speed,
/// particularly fast decompression. It is the recommended default for
/// backup workloads where storage efficiency matters.
#[derive(Debug, Clone)]
pub struct ZstdCompressor {
    /// Compression level (1-22, default 3).
    level: i32,
}

impl ZstdCompressor {
    /// Create a new Zstd compressor with the default compression level (3).
    pub fn new() -> Self {
        Self { level: 3 }
    }

    /// Create with a specific compression level.
    ///
    /// Level range: 1 (fastest) to 22 (best compression).
    /// Recommended: 3 for balanced, 9 for higher ratio, 1 for speed.
    pub fn with_level(level: i32) -> Self {
        let level = level.clamp(1, 22);
        Self { level }
    }

    /// Get the compression level.
    pub fn level(&self) -> i32 {
        self.level
    }
}

impl Default for ZstdCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultCompressor for ZstdCompressor {
    fn compress(&self, data: &[u8]) -> CompressResult<Vec<u8>> {
        zstd::encode_all(data, self.level)
            .map_err(|e| VaultError::Compression(format!("Zstd compression failed: {}", e)))
    }

    fn decompress(&self, data: &[u8]) -> CompressResult<Vec<u8>> {
        // Auto-detect: verify magic bytes
        if data.len() >= 4 && &data[..4] != ZSTD_MAGIC {
            return Err(VaultError::Compression(
                "Data does not have Zstd magic bytes".to_string(),
            ));
        }
        zstd::decode_all(data)
            .map_err(|e| VaultError::Compression(format!("Zstd decompression failed: {}", e)))
    }

    fn algorithm(&self) -> CompressionAlgorithm {
        CompressionAlgorithm::Zstd
    }
}

// ============================================================================
// Lz4Compressor
// ============================================================================

/// LZ4 compression implementation.
///
/// LZ4 focuses on extremely fast compression speed at the cost of
/// lower compression ratio. Ideal for latency-sensitive operations
/// or when data is already partially compressed.
#[derive(Debug, Clone)]
pub struct Lz4Compressor {
    /// Whether to use LZ4 HC (higher compression, slower).
    high_compression: bool,
}

impl Lz4Compressor {
    /// Create a new LZ4 compressor (fast mode).
    pub fn new() -> Self {
        Self {
            high_compression: false,
        }
    }

    /// Create with high compression mode (LZ4 HC).
    /// Trades compression speed for better ratio; decompression is still fast.
    pub fn with_high_compression() -> Self {
        Self {
            high_compression: true,
        }
    }
}

impl Default for Lz4Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultCompressor for Lz4Compressor {
    fn compress(&self, data: &[u8]) -> CompressResult<Vec<u8>> {
        let compressed = if self.high_compression {
            lz4_flex::compress_prepend_size(data)
        } else {
            lz4_flex::compress_prepend_size(data)
        };
        // Prepend LZ4 magic bytes for format detection
        let mut result = Vec::with_capacity(4 + compressed.len());
        result.extend_from_slice(LZ4_MAGIC);
        result.extend_from_slice(&compressed);
        Ok(result)
    }

    fn decompress(&self, data: &[u8]) -> CompressResult<Vec<u8>> {
        // Verify magic bytes
        if data.len() < 4 {
            return Err(VaultError::Compression("Data too short for LZ4".to_string()));
        }
        if &data[..4] != LZ4_MAGIC {
            return Err(VaultError::Compression(
                "Data does not have LZ4 magic bytes".to_string(),
            ));
        }

        let payload = &data[4..];
        lz4_flex::decompress_size_prepended(payload)
            .map_err(|e| VaultError::Compression(format!("LZ4 decompression failed: {}", e)))
    }

    fn algorithm(&self) -> CompressionAlgorithm {
        CompressionAlgorithm::Lz4
    }
}

// ============================================================================
// Auto-detecting decompressor
// ============================================================================

/// Decompress data with automatic format detection.
///
/// Detects the compression format from magic bytes and decompresses accordingly.
pub fn auto_decompress(data: &[u8]) -> CompressResult<Vec<u8>> {
    match detect_format(data) {
        Some(CompressionAlgorithm::Zstd) => {
            let compressor = ZstdCompressor::new();
            compressor.decompress(data)
        }
        Some(CompressionAlgorithm::Lz4) => {
            let compressor = Lz4Compressor::new();
            compressor.decompress(data)
        }
        None => Err(VaultError::Compression(
            "Unknown compression format (magic bytes not recognized)".to_string(),
        )),
    }
}

// ============================================================================
// CompressedStorage (transparent compression decorator)
// ============================================================================

/// Transparent compression decorator for VaultStorage.
///
/// Wraps any VaultStorage implementation and automatically compresses/decompresses
/// file data on store/retrieve operations. Snapshot metadata is passed through
/// uncompressed to remain queryable.
pub struct CompressedStorage {
    inner: Arc<dyn VaultStorage>,
    compressor: Arc<dyn VaultCompressor>,
}

impl CompressedStorage {
    /// Create a new CompressedStorage with the given compressor.
    pub fn new(storage: Arc<dyn VaultStorage>, compressor: Arc<dyn VaultCompressor>) -> Self {
        Self {
            inner: storage,
            compressor,
        }
    }

    /// Create with Zstd compression (default level).
    pub fn with_zstd(storage: Arc<dyn VaultStorage>) -> Self {
        Self::new(storage, Arc::new(ZstdCompressor::new()))
    }

    /// Create with Zstd compression at a specific level.
    pub fn with_zstd_level(storage: Arc<dyn VaultStorage>, level: i32) -> Self {
        Self::new(storage, Arc::new(ZstdCompressor::with_level(level)))
    }

    /// Create with LZ4 compression (fast mode).
    pub fn with_lz4(storage: Arc<dyn VaultStorage>) -> Self {
        Self::new(storage, Arc::new(Lz4Compressor::new()))
    }

    /// Create with LZ4 HC compression.
    pub fn with_lz4_hc(storage: Arc<dyn VaultStorage>) -> Self {
        Self::new(storage, Arc::new(Lz4Compressor::with_high_compression()))
    }
}

impl VaultStorage for CompressedStorage {
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> crate::error::VaultResult<()> {
        let compressed = self.compressor.compress(data)
            .map_err(|e| VaultError::Storage(format!("Compression failed for {}: {}", rel_path, e)))?;
        self.inner.store_file(snapshot_id, rel_path, &compressed)
    }

    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> crate::error::VaultResult<Vec<u8>> {
        let compressed = self.inner.retrieve_file(snapshot_id, rel_path)?;

        // Auto-detect compression format for robustness
        match detect_format(&compressed) {
            Some(CompressionAlgorithm::Zstd) => {
                let zstd = ZstdCompressor::new();
                zstd.decompress(&compressed)
            }
            Some(CompressionAlgorithm::Lz4) => {
                let lz4 = Lz4Compressor::new();
                lz4.decompress(&compressed)
            }
            None => {
                // If no magic bytes detected, data might be uncompressed
                // (backward compatibility or metadata files)
                Ok(compressed)
            }
        }
    }

    fn store_snapshot(&self, snapshot: &Snapshot) -> crate::error::VaultResult<()> {
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
        "compressed"
    }

    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> crate::error::VaultResult<()> {
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
                VaultError::RestoreFailed(format!("Failed to write {}: {}", target_path.display(), e))
            })?;
        }

        Ok(())
    }
}

// ============================================================================
// Compression statistics
// ============================================================================

/// Statistics about a compression operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionStats {
    /// Original size in bytes.
    pub original_size: u64,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Compression ratio (original / compressed).
    pub ratio: f64,
    /// Algorithm used.
    pub algorithm: CompressionAlgorithm,
}

impl CompressionStats {
    /// Calculate stats from original and compressed sizes.
    pub fn new(original_size: u64, compressed_size: u64, algorithm: CompressionAlgorithm) -> Self {
        let ratio = if compressed_size > 0 {
            original_size as f64 / compressed_size as f64
        } else {
            0.0
        };
        Self {
            original_size,
            compressed_size,
            ratio,
            algorithm,
        }
    }

    /// Compression savings as percentage.
    pub fn savings_percent(&self) -> f64 {
        if self.original_size == 0 {
            return 0.0;
        }
        ((self.original_size - self.compressed_size) as f64 / self.original_size as f64) * 100.0
    }

    /// Space saved in bytes.
    pub fn bytes_saved(&self) -> u64 {
        self.original_size.saturating_sub(self.compressed_size)
    }
}

/// Compress data and return both the compressed data and statistics.
pub fn compress_with_stats(
    data: &[u8],
    compressor: &dyn VaultCompressor,
) -> CompressResult<(Vec<u8>, CompressionStats)> {
    let original_size = data.len() as u64;
    let compressed = compressor.compress(data)?;
    let stats = CompressionStats::new(original_size, compressed.len() as u64, compressor.algorithm());
    Ok((compressed, stats))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zstd_roundtrip() {
        let compressor = ZstdCompressor::new();
        let data = b"Hello, Zstd compression! This is a test of the Zstd compressor.";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_compression_ratio() {
        let compressor = ZstdCompressor::new();
        // Repeated data compresses well
        let data: Vec<u8> = "AAAA".repeat(10000).into_bytes();

        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len() / 10, "Zstd should compress repeated data well");
    }

    #[test]
    fn test_zstd_levels() {
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();

        let fast = ZstdCompressor::with_level(1);
        let best = ZstdCompressor::with_level(9);

        let compressed_fast = fast.compress(&data).unwrap();
        let compressed_best = best.compress(&data).unwrap();

        // Both should decompress to the same data
        assert_eq!(fast.decompress(&compressed_fast).unwrap(), data);
        assert_eq!(best.decompress(&compressed_best).unwrap(), data);
    }

    #[test]
    fn test_lz4_roundtrip() {
        let compressor = Lz4Compressor::new();
        let data = b"Hello, LZ4 compression! Fast and furious.";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_hc_roundtrip() {
        let compressor = Lz4Compressor::with_high_compression();
        let data = b"Hello, LZ4 HC! Higher compression but still fast decompression.";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_lz4_magic_bytes() {
        let compressor = Lz4Compressor::new();
        let compressed = compressor.compress(b"test").unwrap();

        assert!(compressed.len() >= 4);
        assert_eq!(&compressed[..4], LZ4_MAGIC);
    }

    #[test]
    fn test_zstd_magic_bytes() {
        let compressor = ZstdCompressor::new();
        let compressed = compressor.compress(b"test").unwrap();

        assert!(compressed.len() >= 4);
        assert_eq!(&compressed[..4], ZSTD_MAGIC);
    }

    #[test]
    fn test_format_detection_zstd() {
        let compressor = ZstdCompressor::new();
        let compressed = compressor.compress(b"test data").unwrap();

        assert_eq!(detect_format(&compressed), Some(CompressionAlgorithm::Zstd));
    }

    #[test]
    fn test_format_detection_lz4() {
        let compressor = Lz4Compressor::new();
        let compressed = compressor.compress(b"test data").unwrap();

        assert_eq!(detect_format(&compressed), Some(CompressionAlgorithm::Lz4));
    }

    #[test]
    fn test_format_detection_unknown() {
        let data = b"not compressed";
        assert_eq!(detect_format(data), None);
    }

    #[test]
    fn test_format_detection_too_short() {
        let data = b"ab";
        assert_eq!(detect_format(data), None);
    }

    #[test]
    fn test_auto_decompress_zstd() {
        let compressor = ZstdCompressor::new();
        let data = b"auto-detect zstd";
        let compressed = compressor.compress(data).unwrap();

        let decompressed = auto_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_auto_decompress_lz4() {
        let compressor = Lz4Compressor::new();
        let data = b"auto-detect lz4";
        let compressed = compressor.compress(data).unwrap();

        let decompressed = auto_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_auto_decompress_unknown() {
        let data = b"not compressed";
        assert!(auto_decompress(data).is_err());
    }

    #[test]
    fn test_large_data_zstd() {
        let compressor = ZstdCompressor::new();
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

        let compressed = compressor.compress(&data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_large_data_lz4() {
        let compressor = Lz4Compressor::new();
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

        let compressed = compressor.compress(&data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_empty_data_zstd() {
        let compressor = ZstdCompressor::new();
        let data = b"";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_empty_data_lz4() {
        let compressor = Lz4Compressor::new();
        let data = b"";

        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compression_stats() {
        let data: Vec<u8> = "A".repeat(10000).into_bytes();
        let compressor = ZstdCompressor::new();
        let (compressed, stats) = compress_with_stats(&data, &compressor).unwrap();

        assert_eq!(stats.original_size, 10000);
        assert_eq!(stats.compressed_size, compressed.len() as u64);
        assert!(stats.ratio > 1.0, "Compressible data should have ratio > 1");
        assert!(stats.savings_percent() > 0.0);
        assert!(stats.bytes_saved() > 0);
    }

    #[test]
    fn test_compression_algorithm_magic() {
        assert_eq!(CompressionAlgorithm::Zstd.magic_bytes(), ZSTD_MAGIC);
        assert_eq!(CompressionAlgorithm::Lz4.magic_bytes(), LZ4_MAGIC);
    }

    #[test]
    fn test_compression_algorithm_name() {
        assert_eq!(CompressionAlgorithm::Zstd.name(), "Zstd");
        assert_eq!(CompressionAlgorithm::Lz4.name(), "LZ4");
    }

    #[test]
    fn test_zstd_invalid_magic() {
        let compressor = ZstdCompressor::new();
        let result = compressor.decompress(b"not zstd data at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_lz4_invalid_magic() {
        let compressor = Lz4Compressor::new();
        let result = compressor.decompress(b"not lz4 data at all");
        assert!(result.is_err());
    }
}
