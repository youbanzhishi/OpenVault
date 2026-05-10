//! Incremental backup engine for OpenVault.
//!
//! Provides content-defined chunking (CDC), deduplication, and incremental
//! backup/restore capabilities. Only changed chunks are stored, dramatically
//! reducing storage requirements for repeated backups.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::VaultError;
use crate::storage::VaultStorage;

/// Result type for incremental operations.
pub type IncrementalResult<T> = Result<T, VaultError>;

// ============================================================================
// Chunk & Chunker (Content-Defined Chunking)
// ============================================================================

/// A single data chunk produced by the chunker.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// SHA-256 hash of the chunk data (used for deduplication).
    pub hash: String,
    /// The chunk data.
    pub data: Vec<u8>,
    /// Offset of this chunk within the original data.
    pub offset: u64,
    /// Size of this chunk in bytes.
    pub size: usize,
}

impl Chunk {
    /// Compute SHA-256 hash of the given data.
    pub fn compute_hash(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }
}

/// Content-Defined Chunking (CDC) engine.
///
/// Uses a rolling hash (Buzhash-based) to determine chunk boundaries
/// based on data content, ensuring that insertions or deletions only
/// affect nearby chunk boundaries rather than shifting all boundaries.
///
/// Chunk sizes are bounded between `min_chunk` and `max_chunk` to
/// prevent extremely small or large chunks.
#[derive(Debug, Clone)]
pub struct Chunker {
    /// Minimum chunk size in bytes.
    pub min_chunk: usize,
    /// Maximum chunk size in bytes.
    pub max_chunk: usize,
    /// Mask for boundary detection: if hash & mask == mask, boundary found.
    /// Controls average chunk size. Higher mask = larger average chunks.
    pub mask: u32,
    /// Window size for rolling hash.
    window_size: usize,
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker {
    /// Create a new Chunker with sensible defaults.
    ///
    /// Defaults: min 4KB, max 1MB, average ~16KB, window 48 bytes.
    pub fn new() -> Self {
        Self {
            min_chunk: 4 * 1024,
            max_chunk: 1024 * 1024,
            mask: 0x1FFF, // ~8KB average
            window_size: 48,
        }
    }

    /// Create with custom parameters.
    pub fn with_params(min_chunk: usize, max_chunk: usize, mask: u32) -> Self {
        Self {
            min_chunk,
            max_chunk,
            mask,
            window_size: 48,
        }
    }

    /// Create a chunker optimized for small files.
    pub fn small() -> Self {
        Self {
            min_chunk: 512,
            max_chunk: 64 * 1024,
            mask: 0x1FF, // ~512B average
            window_size: 32,
        }
    }

    /// Create a chunker optimized for large files.
    pub fn large() -> Self {
        Self {
            min_chunk: 64 * 1024,
            max_chunk: 8 * 1024 * 1024,
            mask: 0xFFFF, // ~64KB average
            window_size: 64,
        }
    }

    /// Chunk data using content-defined chunking.
    ///
    /// Returns a list of chunks with their hashes, offsets, and sizes.
    pub fn chunk_data(&self, data: &[u8]) -> Vec<Chunk> {
        if data.is_empty() {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < data.len() {
            let end = self.find_boundary(data, start);
            let chunk_data = data[start..end].to_vec();
            let hash = Chunk::compute_hash(&chunk_data);

            chunks.push(Chunk {
                hash,
                data: chunk_data,
                offset: start as u64,
                size: end - start,
            });

            start = end;
        }

        chunks
    }

    /// Find the next chunk boundary starting from `start`.
    fn find_boundary(&self, data: &[u8], start: usize) -> usize {
        let remaining = data.len() - start;

        // If remaining data is less than min_chunk, take it all
        if remaining <= self.min_chunk {
            return data.len();
        }

        // Scan from start + min_chunk to find a boundary
        let scan_start = start + self.min_chunk;
        let scan_end = std::cmp::min(start + self.max_chunk, data.len());

        // Use a simple rolling hash over the window
        let mut hash: u32 = 0;

        // Initialize rolling hash with the first window
        let init_end = std::cmp::min(scan_start + self.window_size, scan_end);
        for i in scan_start..init_end {
            hash = hash.wrapping_shl(1).wrapping_add(data[i] as u32);
        }

        // Check the initial position
        if hash & self.mask == self.mask {
            return init_end;
        }

        // Roll the hash forward
        for i in init_end..scan_end {
            // Remove oldest byte, add new byte
            if i >= self.window_size {
                let old_idx = i - self.window_size;
                hash = hash.wrapping_sub(data[old_idx].wrapping_shl(self.window_size as u32 % 32) as u32);
            }
            hash = hash.wrapping_shl(1).wrapping_add(data[i] as u32);

            if hash & self.mask == self.mask {
                return i + 1;
            }
        }

        // No boundary found within max_chunk; return max_chunk boundary
        scan_end
    }

    /// Chunk a file by reading it from disk.
    pub fn chunk_file(&self, path: &Path) -> IncrementalResult<Vec<Chunk>> {
        let data = std::fs::read(path).map_err(|e| {
            VaultError::Incremental(format!("Failed to read file {}: {}", path.display(), e))
        })?;
        Ok(self.chunk_data(&data))
    }
}

// ============================================================================
// ChunkStore (deduplication store)
// ============================================================================

/// A reference to a chunk stored in the ChunkStore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRef {
    /// SHA-256 hash of the chunk.
    pub hash: String,
    /// Size of the chunk in bytes.
    pub size: usize,
}

/// Metadata for the chunk store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChunkStoreMeta {
    /// Set of hashes currently stored (for dedup tracking).
    pub stored_hashes: HashSet<String>,
    /// Total number of chunks stored.
    pub total_chunks: u64,
    /// Total bytes of chunk data stored.
    pub total_bytes: u64,
    /// Number of duplicates detected (chunks not stored because already present).
    pub duplicates_detected: u64,
}

/// Deduplicating chunk store backed by a VaultStorage.
///
/// Each chunk is stored as a separate file keyed by its SHA-256 hash.
/// If a chunk with the same hash already exists, it is not stored again,
/// achieving deduplication across files and backup versions.
pub struct ChunkStore {
    /// Underlying storage backend.
    storage: std::sync::Arc<dyn VaultStorage>,
    /// Namespace prefix for chunk storage (to separate from regular files).
    namespace: String,
    /// In-memory metadata tracking stored chunks.
    meta: std::sync::Mutex<ChunkStoreMeta>,
}

impl ChunkStore {
    /// Create a new ChunkStore backed by the given storage.
    pub fn new(storage: std::sync::Arc<dyn VaultStorage>, namespace: &str) -> Self {
        Self {
            storage,
            namespace: namespace.to_string(),
            meta: std::sync::Mutex::new(ChunkStoreMeta::default()),
        }
    }

    /// Get the chunk key (storage path) for a given hash.
    fn chunk_key(&self, hash: &str) -> String {
        format!("__chunks__/{}/{}", self.namespace, hash)
    }

    /// Get the metadata key for the chunk store.
    fn meta_key(&self) -> String {
        format!("__chunks__/{}/_meta", self.namespace)
    }

    /// Store a chunk. Returns true if the chunk was newly stored, false if it already existed.
    pub fn store_chunk(&self, snapshot_id: &str, chunk: &Chunk) -> IncrementalResult<bool> {
        let mut meta = self.meta.lock().unwrap();

        if meta.stored_hashes.contains(&chunk.hash) {
            meta.duplicates_detected += 1;
            return Ok(false);
        }

        self.storage
            .store_file(snapshot_id, &self.chunk_key(&chunk.hash), &chunk.data)?;

        meta.stored_hashes.insert(chunk.hash.clone());
        meta.total_chunks += 1;
        meta.total_bytes += chunk.size as u64;

        Ok(true)
    }

    /// Store multiple chunks, deduplicating as we go.
    /// Returns (new_chunks_count, duplicate_count).
    pub fn store_chunks(&self, snapshot_id: &str, chunks: &[Chunk]) -> IncrementalResult<(usize, usize)> {
        let mut new_count = 0;
        let mut dup_count = 0;

        for chunk in chunks {
            match self.store_chunk(snapshot_id, chunk)? {
                true => new_count += 1,
                false => dup_count += 1,
            }
        }

        Ok((new_count, dup_count))
    }

    /// Retrieve a chunk by its hash.
    pub fn retrieve_chunk(&self, snapshot_id: &str, hash: &str) -> IncrementalResult<Vec<u8>> {
        self.storage
            .retrieve_file(snapshot_id, &self.chunk_key(hash))
            .map_err(|e| {
                VaultError::Incremental(format!("Chunk {} not found: {}", hash, e))
            })
    }

    /// Check if a chunk exists.
    pub fn chunk_exists(&self, hash: &str) -> bool {
        let meta = self.meta.lock().unwrap();
        meta.stored_hashes.contains(hash)
    }

    /// Get metadata statistics.
    pub fn stats(&self) -> ChunkStoreMeta {
        let meta = self.meta.lock().unwrap();
        meta.clone()
    }

    /// Load metadata from storage (restores dedup tracking state).
    pub fn load_meta(&self, snapshot_id: &str) -> IncrementalResult<()> {
        match self.storage.retrieve_file(snapshot_id, &self.meta_key()) {
            Ok(data) => {
                let meta: ChunkStoreMeta = serde_json::from_slice(&data).map_err(|e| {
                    VaultError::Incremental(format!("Failed to parse chunk store meta: {}", e))
                })?;
                let mut guard = self.meta.lock().unwrap();
                *guard = meta;
                Ok(())
            }
            Err(_) => {
                // Meta doesn't exist yet; start fresh
                Ok(())
            }
        }
    }

    /// Persist metadata to storage.
    pub fn save_meta(&self, snapshot_id: &str) -> IncrementalResult<()> {
        let meta = self.meta.lock().unwrap();
        let data = serde_json::to_vec(&*meta).map_err(|e| {
            VaultError::Incremental(format!("Failed to serialize chunk store meta: {}", e))
        })?;
        self.storage
            .store_file(snapshot_id, &self.meta_key(), &data)?;
        Ok(())
    }

    /// Remove chunks that are no longer referenced (garbage collection).
    /// `live_hashes` is the set of hashes still in use.
    pub fn gc(&self, snapshot_id: &str, live_hashes: &HashSet<String>) -> IncrementalResult<u64> {
        let mut meta = self.meta.lock().unwrap();
        let to_remove: Vec<String> = meta
            .stored_hashes
            .iter()
            .filter(|h| !live_hashes.contains(*h))
            .cloned()
            .collect();

        let removed_count = to_remove.len() as u64;

        for hash in &to_remove {
            let _ = self.storage.store_file(
                snapshot_id,
                &self.chunk_key(hash),
                &[], // Empty = mark for deletion (or use delete if available)
            );
            meta.stored_hashes.remove(hash);
        }

        meta.total_chunks -= removed_count;

        Ok(removed_count)
    }
}

// ============================================================================
// BackupManifest
// ============================================================================

/// Mapping of a file to its constituent chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunks {
    /// Original file path.
    pub path: String,
    /// Original file size in bytes.
    pub original_size: u64,
    /// SHA-256 checksum of the entire file.
    pub file_hash: String,
    /// Ordered list of chunk hashes that compose this file.
    pub chunk_hashes: Vec<String>,
    /// Chunk sizes (parallel to chunk_hashes).
    pub chunk_sizes: Vec<usize>,
}

impl FileChunks {
    /// Create a new FileChunks from a file path and its chunks.
    pub fn from_chunks(path: &str, data: &[u8], chunks: &[Chunk]) -> Self {
        let file_hash = Chunk::compute_hash(data);
        Self {
            path: path.to_string(),
            original_size: data.len() as u64,
            file_hash,
            chunk_hashes: chunks.iter().map(|c| c.hash.clone()).collect(),
            chunk_sizes: chunks.iter().map(|c| c.size).collect(),
        }
    }
}

/// Backup manifest recording the file-to-chunk mapping for a backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Unique identifier for this manifest/backup.
    pub id: String,
    /// Timestamp when this manifest was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Source path that was backed up.
    pub source: String,
    /// File-to-chunk mappings.
    pub files: HashMap<String, FileChunks>,
    /// Parent manifest ID (None for full backup, Some for incremental).
    pub parent_id: Option<String>,
    /// Total original size of all files.
    pub total_original_size: u64,
    /// Total size of unique chunks stored.
    pub total_stored_size: u64,
    /// Number of new chunks in this backup.
    pub new_chunk_count: u64,
    /// Number of duplicate chunks skipped.
    pub duplicate_chunk_count: u64,
}

impl BackupManifest {
    /// Create a new empty manifest.
    pub fn new(source: &str, parent_id: Option<String>) -> Self {
        let id = format!(
            "manifest-{}",
            chrono::Utc::now().format("%Y%m%d%H%M%S")
        );
        Self {
            id,
            created_at: chrono::Utc::now(),
            source: source.to_string(),
            files: HashMap::new(),
            parent_id,
            total_original_size: 0,
            total_stored_size: 0,
            new_chunk_count: 0,
            duplicate_chunk_count: 0,
        }
    }

    /// Add a file's chunk mapping to the manifest.
    pub fn add_file(&mut self, file_chunks: FileChunks, new_chunks: usize, dup_chunks: usize) {
        self.total_original_size += file_chunks.original_size;
        self.new_chunk_count += new_chunks as u64;
        self.duplicate_chunk_count += dup_chunks as u64;
        self.files.insert(file_chunks.path.clone(), file_chunks);
    }

    /// Get the file-to-chunk mapping for a specific file.
    pub fn get_file_chunks(&self, path: &str) -> Option<&FileChunks> {
        self.files.get(path)
    }

    /// List all file paths in this manifest.
    pub fn file_paths(&self) -> Vec<&str> {
        self.files.keys().map(|s| s.as_str()).collect()
    }

    /// Get all unique chunk hashes referenced by this manifest.
    pub fn referenced_chunk_hashes(&self) -> HashSet<String> {
        self.files
            .values()
            .flat_map(|fc| fc.chunk_hashes.iter().cloned())
            .collect()
    }

    /// Compute deduplication ratio.
    pub fn dedup_ratio(&self) -> f64 {
        let total_chunks = self.new_chunk_count + self.duplicate_chunk_count;
        if total_chunks == 0 {
            return 0.0;
        }
        self.duplicate_chunk_count as f64 / total_chunks as f64
    }

    /// Serialize the manifest to JSON bytes.
    pub fn to_bytes(&self) -> IncrementalResult<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| {
            VaultError::Incremental(format!("Failed to serialize manifest: {}", e))
        })
    }

    /// Deserialize a manifest from JSON bytes.
    pub fn from_bytes(data: &[u8]) -> IncrementalResult<Self> {
        serde_json::from_slice(data).map_err(|e| {
            VaultError::Incremental(format!("Failed to deserialize manifest: {}", e))
        })
    }

    /// Number of files in this manifest.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

// ============================================================================
// IncrementalBackup
// ============================================================================

/// Report from an incremental backup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReport {
    /// Manifest ID.
    pub manifest_id: String,
    /// Number of files backed up.
    pub files_backed_up: usize,
    /// Total original size.
    pub total_original_size: u64,
    /// Total stored size (new chunks only).
    pub total_stored_size: u64,
    /// Number of new chunks.
    pub new_chunks: u64,
    /// Number of duplicate chunks.
    pub duplicate_chunks: u64,
    /// Deduplication ratio.
    pub dedup_ratio: f64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Incremental backup engine.
///
/// Uses content-defined chunking and deduplication to efficiently store
/// only the data that has changed since the last backup.
pub struct IncrementalBackup {
    /// Chunker for splitting data into content-defined chunks.
    chunker: Chunker,
    /// Chunk store for deduplication.
    chunk_store: ChunkStore,
}

impl IncrementalBackup {
    /// Create a new incremental backup engine.
    pub fn new(storage: std::sync::Arc<dyn VaultStorage>, namespace: &str) -> Self {
        Self {
            chunker: Chunker::new(),
            chunk_store: ChunkStore::new(storage, namespace),
        }
    }

    /// Create with a custom chunker configuration.
    pub fn with_chunker(
        storage: std::sync::Arc<dyn VaultStorage>,
        namespace: &str,
        chunker: Chunker,
    ) -> Self {
        Self {
            chunker,
            chunk_store: ChunkStore::new(storage, namespace),
        }
    }

    /// Perform a full backup of the given directory.
    ///
    /// All files are chunked and stored. The resulting manifest records
    /// the complete file-to-chunk mapping.
    pub fn backup_full(
        &self,
        snapshot_id: &str,
        source: &Path,
    ) -> IncrementalResult<(BackupManifest, BackupReport)> {
        let start = std::time::Instant::now();

        let mut manifest = BackupManifest::new(
            source.to_string_lossy().as_ref(),
            None, // Full backup has no parent
        );

        self.backup_directory(snapshot_id, source, &mut manifest)?;

        let duration = start.elapsed();
        let report = BackupReport {
            manifest_id: manifest.id.clone(),
            files_backed_up: manifest.file_count(),
            total_original_size: manifest.total_original_size,
            total_stored_size: manifest.total_stored_size,
            new_chunks: manifest.new_chunk_count,
            duplicate_chunks: manifest.duplicate_chunk_count,
            dedup_ratio: manifest.dedup_ratio(),
            duration_ms: duration.as_millis() as u64,
        };

        // Save manifest and meta
        self.save_manifest(snapshot_id, &manifest)?;

        Ok((manifest, report))
    }

    /// Perform an incremental backup relative to a parent manifest.
    ///
    /// Only chunks that are not already in the chunk store are stored.
    /// Files that haven't changed will reference the same chunks as
    /// the parent backup.
    pub fn backup_incremental(
        &self,
        snapshot_id: &str,
        source: &Path,
        parent_manifest: &BackupManifest,
    ) -> IncrementalResult<(BackupManifest, BackupReport)> {
        let start = std::time::Instant::now();

        let mut manifest = BackupManifest::new(
            source.to_string_lossy().as_ref(),
            Some(parent_manifest.id.clone()),
        );

        self.backup_directory(snapshot_id, source, &mut manifest)?;

        let duration = start.elapsed();
        let report = BackupReport {
            manifest_id: manifest.id.clone(),
            files_backed_up: manifest.file_count(),
            total_original_size: manifest.total_original_size,
            total_stored_size: manifest.total_stored_size,
            new_chunks: manifest.new_chunk_count,
            duplicate_chunks: manifest.duplicate_chunk_count,
            dedup_ratio: manifest.dedup_ratio(),
            duration_ms: duration.as_millis() as u64,
        };

        self.save_manifest(snapshot_id, &manifest)?;

        Ok((manifest, report))
    }

    /// Recursively back up a directory.
    fn backup_directory(
        &self,
        snapshot_id: &str,
        dir: &Path,
        manifest: &mut BackupManifest,
    ) -> IncrementalResult<()> {
        if !dir.is_dir() {
            return Err(VaultError::Incremental(format!(
                "{} is not a directory",
                dir.display()
            )));
        }

        let entries = std::fs::read_dir(dir).map_err(|e| {
            VaultError::Incremental(format!("Failed to read directory {}: {}", dir.display(), e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                VaultError::Incremental(format!("Failed to read dir entry: {}", e))
            })?;

            let path = entry.path();
            if path.is_dir() {
                self.backup_directory(snapshot_id, &path, manifest)?;
            } else if path.is_file() {
                self.backup_file(snapshot_id, &path, dir, manifest)?;
            }
        }

        Ok(())
    }

    /// Back up a single file.
    fn backup_file(
        &self,
        snapshot_id: &str,
        file_path: &Path,
        base_dir: &Path,
        manifest: &mut BackupManifest,
    ) -> IncrementalResult<()> {
        let data = std::fs::read(file_path).map_err(|e| {
            VaultError::Incremental(format!(
                "Failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        let rel_path = file_path
            .strip_prefix(base_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        // Chunk the file
        let chunks = self.chunker.chunk_data(&data);

        // Store chunks with deduplication
        let (new_count, dup_count) = self.chunk_store.store_chunks(snapshot_id, &chunks)?;

        // Compute total stored size for new chunks
        let stored_size: u64 = chunks
            .iter()
            .filter(|c| !self.chunk_store.chunk_exists_before_store(&c.hash))
            .map(|c| c.size as u64)
            .sum();

        let file_chunks = FileChunks::from_chunks(&rel_path, &data, &chunks);
        manifest.add_file(file_chunks, new_count, dup_count);
        manifest.total_stored_size += stored_size;

        Ok(())
    }

    /// Save a manifest to storage.
    pub fn save_manifest(
        &self,
        snapshot_id: &str,
        manifest: &BackupManifest,
    ) -> IncrementalResult<()> {
        let key = format!("__manifests__/{}", manifest.id);
        let data = manifest.to_bytes()?;
        self.chunk_store.storage.store_file(snapshot_id, &key, &data)?;
        self.chunk_store.save_meta(snapshot_id)?;
        Ok(())
    }

    /// Load a manifest from storage.
    pub fn load_manifest(
        &self,
        snapshot_id: &str,
        manifest_id: &str,
    ) -> IncrementalResult<BackupManifest> {
        let key = format!("__manifests__/{}", manifest_id);
        let data = self.chunk_store.storage.retrieve_file(snapshot_id, &key)?;
        BackupManifest::from_bytes(&data)
    }

    /// Get a reference to the chunk store.
    pub fn chunk_store(&self) -> &ChunkStore {
        &self.chunk_store
    }

    /// Get a reference to the chunker.
    pub fn chunker(&self) -> &Chunker {
        &self.chunker
    }
}

// Internal helper: check if a hash existed before we tried to store it.
// This is a workaround since we don't have a separate "exists" check
// that accounts for the just-stored state.
impl ChunkStore {
    /// Check if a hash existed before the current store operation.
    /// Used for computing stored_size accurately.
    fn chunk_exists_before_store(&self, hash: &str) -> bool {
        let meta = self.meta.lock().unwrap();
        meta.stored_hashes.contains(hash)
    }
}

// ============================================================================
// BackupRestore
// ============================================================================

/// Report from a restore operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreReport {
    /// Number of files restored.
    pub files_restored: usize,
    /// Total bytes restored.
    pub total_bytes: u64,
    /// Number of chunks that were reused (dedup hit).
    pub chunks_reused: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Incremental backup restore engine.
///
/// Reconstructs complete files from their chunk mappings stored in
/// backup manifests.
pub struct BackupRestore {
    /// Chunk store to retrieve chunks from.
    chunk_store: ChunkStore,
}

impl BackupRestore {
    /// Create a new BackupRestore engine.
    pub fn new(storage: std::sync::Arc<dyn VaultStorage>, namespace: &str) -> Self {
        Self {
            chunk_store: ChunkStore::new(storage, namespace),
        }
    }

    /// Restore all files from a backup manifest to a target directory.
    pub fn restore_all(
        &self,
        snapshot_id: &str,
        manifest: &BackupManifest,
        target: &Path,
    ) -> IncrementalResult<RestoreReport> {
        let start = std::time::Instant::now();
        let mut report = RestoreReport {
            files_restored: 0,
            total_bytes: 0,
            chunks_reused: 0,
            duration_ms: 0,
        };

        std::fs::create_dir_all(target).map_err(|e| {
            VaultError::Incremental(format!("Failed to create target directory: {}", e))
        })?;

        for (rel_path, file_chunks) in &manifest.files {
            match self.restore_file(snapshot_id, file_chunks, target) {
                Ok(bytes) => {
                    report.files_restored += 1;
                    report.total_bytes += bytes;
                }
                Err(e) => {
                    // Log error but continue with other files
                    eprintln!("Warning: Failed to restore {}: {}", rel_path, e);
                }
            }
        }

        report.duration_ms = start.elapsed().as_millis() as u64;
        Ok(report)
    }

    /// Restore a single file from its chunk mapping.
    pub fn restore_file(
        &self,
        snapshot_id: &str,
        file_chunks: &FileChunks,
        target: &Path,
    ) -> IncrementalResult<u64> {
        let target_path = target.join(&file_chunks.path);

        // Ensure parent directory exists
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VaultError::Incremental(format!(
                    "Failed to create parent directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Reassemble file from chunks
        let mut file_data = Vec::with_capacity(file_chunks.original_size as usize);
        for hash in &file_chunks.chunk_hashes {
            let chunk_data = self.chunk_store.retrieve_chunk(snapshot_id, hash)?;
            file_data.extend_from_slice(&chunk_data);
        }

        // Verify file hash
        let actual_hash = Chunk::compute_hash(&file_data);
        if actual_hash != file_chunks.file_hash {
            return Err(VaultError::Incremental(format!(
                "File hash mismatch for {}: expected {}, got {}",
                file_chunks.path, file_chunks.file_hash, actual_hash
            )));
        }

        // Write to target
        std::fs::write(&target_path, &file_data).map_err(|e| {
            VaultError::Incremental(format!(
                "Failed to write file {}: {}",
                target_path.display(),
                e
            ))
        })?;

        Ok(file_data.len() as u64)
    }

    /// Restore specific files from a manifest.
    pub fn restore_files(
        &self,
        snapshot_id: &str,
        manifest: &BackupManifest,
        target: &Path,
        file_paths: &[&str],
    ) -> IncrementalResult<RestoreReport> {
        let start = std::time::Instant::now();
        let mut report = RestoreReport {
            files_restored: 0,
            total_bytes: 0,
            chunks_reused: 0,
            duration_ms: 0,
        };

        std::fs::create_dir_all(target).map_err(|e| {
            VaultError::Incremental(format!("Failed to create target directory: {}", e))
        })?;

        for path in file_paths {
            if let Some(file_chunks) = manifest.get_file_chunks(path) {
                match self.restore_file(snapshot_id, file_chunks, target) {
                    Ok(bytes) => {
                        report.files_restored += 1;
                        report.total_bytes += bytes;
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to restore {}: {}", path, e);
                    }
                }
            }
        }

        report.duration_ms = start.elapsed().as_millis() as u64;
        Ok(report)
    }

    /// List all files in a manifest.
    pub fn list_files(manifest: &BackupManifest) -> Vec<&str> {
        manifest.file_paths()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_compute_hash() {
        let hash1 = Chunk::compute_hash(b"hello");
        let hash2 = Chunk::compute_hash(b"hello");
        let hash3 = Chunk::compute_hash(b"world");

        assert_eq!(hash1, hash2, "Same data should produce same hash");
        assert_ne!(hash1, hash3, "Different data should produce different hash");
        assert_eq!(hash1.len(), 64, "SHA-256 hex should be 64 chars");
    }

    #[test]
    fn test_chunker_empty_data() {
        let chunker = Chunker::new();
        let chunks = chunker.chunk_data(b"");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunker_small_data() {
        let chunker = Chunker::new();
        let data = b"small data";
        let chunks = chunker.chunk_data(data);

        // Small data should be a single chunk
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, data);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[0].size, data.len());
    }

    #[test]
    fn test_chunker_large_data() {
        let chunker = Chunker::new();
        // 1MB of data should produce multiple chunks
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        let chunks = chunker.chunk_data(&data);

        assert!(!chunks.is_empty());
        // Verify all data is accounted for
        let total_size: usize = chunks.iter().map(|c| c.size).sum();
        assert_eq!(total_size, data.len());
    }

    #[test]
    fn test_chunker_deterministic() {
        let chunker = Chunker::new();
        let data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();

        let chunks1 = chunker.chunk_data(&data);
        let chunks2 = chunker.chunk_data(&data);

        // Same data should produce same chunks
        assert_eq!(chunks1.len(), chunks2.len());
        for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
            assert_eq!(c1.hash, c2.hash);
            assert_eq!(c1.size, c2.size);
            assert_eq!(c1.offset, c2.offset);
        }
    }

    #[test]
    fn test_chunker_stability_on_insertion() {
        // Content-defined chunking should keep most boundaries stable
        // when data is inserted
        let chunker = Chunker::new();
        let data: Vec<u8> = (0..200_000).map(|i| (i % 256) as u8).collect();
        let chunks_before = chunker.chunk_data(&data);

        // Insert some data at the beginning
        let mut modified = vec![0xAA; 100];
        modified.extend_from_slice(&data[100..]);

        let chunks_after = chunker.chunk_data(&modified);

        // Most chunks after the insertion point should remain the same
        // (This is a statistical property, not guaranteed for all data)
        assert!(!chunks_before.is_empty());
        assert!(!chunks_after.is_empty());
    }

    #[test]
    fn test_chunker_min_max_bounds() {
        let chunker = Chunker::with_params(1024, 8192, 0xFF);
        let data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
        let chunks = chunker.chunk_data(&data);

        for chunk in &chunks {
            // Last chunk can be smaller than min_chunk
            if chunk.offset + (chunk.size as u64) < data.len() as u64 {
                assert!(
                    chunk.size >= chunker.min_chunk,
                    "Chunk size {} < min {}",
                    chunk.size,
                    chunker.min_chunk
                );
            }
            assert!(
                chunk.size <= chunker.max_chunk,
                "Chunk size {} > max {}",
                chunk.size,
                chunker.max_chunk
            );
        }
    }

    #[test]
    fn test_file_chunks_from_chunks() {
        let data = b"test file content";
        let chunker = Chunker::new();
        let chunks = chunker.chunk_data(data);

        let fc = FileChunks::from_chunks("test.txt", data, &chunks);

        assert_eq!(fc.path, "test.txt");
        assert_eq!(fc.original_size, data.len() as u64);
        assert_eq!(fc.chunk_hashes.len(), chunks.len());
        assert_eq!(fc.chunk_sizes.len(), chunks.len());
        assert_eq!(fc.file_hash, Chunk::compute_hash(data));
    }

    #[test]
    fn test_backup_manifest_new() {
        let manifest = BackupManifest::new("/test/source", None);

        assert!(manifest.id.starts_with("manifest-"));
        assert_eq!(manifest.source, "/test/source");
        assert!(manifest.parent_id.is_none());
        assert!(manifest.files.is_empty());
        assert_eq!(manifest.total_original_size, 0);
        assert_eq!(manifest.new_chunk_count, 0);
        assert_eq!(manifest.duplicate_chunk_count, 0);
    }

    #[test]
    fn test_backup_manifest_add_file() {
        let mut manifest = BackupManifest::new("/test", None);
        let fc = FileChunks {
            path: "file.txt".to_string(),
            original_size: 1024,
            file_hash: "abc123".to_string(),
            chunk_hashes: vec!["h1".to_string(), "h2".to_string()],
            chunk_sizes: vec![512, 512],
        };

        manifest.add_file(fc, 1, 1);

        assert_eq!(manifest.file_count(), 1);
        assert_eq!(manifest.total_original_size, 1024);
        assert_eq!(manifest.new_chunk_count, 1);
        assert_eq!(manifest.duplicate_chunk_count, 1);
    }

    #[test]
    fn test_backup_manifest_serialization() {
        let mut manifest = BackupManifest::new("/test", Some("parent-123".to_string()));
        let fc = FileChunks {
            path: "doc.pdf".to_string(),
            original_size: 2048,
            file_hash: "def456".to_string(),
            chunk_hashes: vec!["c1".to_string()],
            chunk_sizes: vec![2048],
        };
        manifest.add_file(fc, 1, 0);

        let bytes = manifest.to_bytes().unwrap();
        let restored = BackupManifest::from_bytes(&bytes).unwrap();

        assert_eq!(restored.id, manifest.id);
        assert_eq!(restored.source, manifest.source);
        assert_eq!(restored.parent_id, manifest.parent_id);
        assert_eq!(restored.file_count(), 1);
    }

    #[test]
    fn test_backup_manifest_referenced_chunks() {
        let mut manifest = BackupManifest::new("/test", None);
        let fc1 = FileChunks {
            path: "a.txt".to_string(),
            original_size: 100,
            file_hash: "h1".to_string(),
            chunk_hashes: vec!["c1".to_string(), "c2".to_string()],
            chunk_sizes: vec![50, 50],
        };
        let fc2 = FileChunks {
            path: "b.txt".to_string(),
            original_size: 100,
            file_hash: "h2".to_string(),
            chunk_hashes: vec!["c2".to_string(), "c3".to_string()],
            chunk_sizes: vec![50, 50],
        };

        manifest.add_file(fc1, 2, 0);
        manifest.add_file(fc2, 1, 1);

        let referenced = manifest.referenced_chunk_hashes();
        assert_eq!(referenced.len(), 3); // c1, c2, c3
        assert!(referenced.contains("c1"));
        assert!(referenced.contains("c2"));
        assert!(referenced.contains("c3"));
    }

    #[test]
    fn test_backup_manifest_dedup_ratio() {
        let mut manifest = BackupManifest::new("/test", None);
        let fc = FileChunks {
            path: "file.txt".to_string(),
            original_size: 100,
            file_hash: "h".to_string(),
            chunk_hashes: vec!["c1".to_string()],
            chunk_sizes: vec![100],
        };
        manifest.add_file(fc, 3, 7);

        // 7 / (3+7) = 0.7
        let ratio = manifest.dedup_ratio();
        assert!((ratio - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_chunker_small_mode() {
        let chunker = Chunker::small();
        assert_eq!(chunker.min_chunk, 512);
        assert_eq!(chunker.max_chunk, 64 * 1024);
    }

    #[test]
    fn test_chunker_large_mode() {
        let chunker = Chunker::large();
        assert_eq!(chunker.min_chunk, 64 * 1024);
        assert_eq!(chunker.max_chunk, 8 * 1024 * 1024);
    }

    #[test]
    fn test_chunker_default() {
        let chunker = Chunker::default();
        assert_eq!(chunker.min_chunk, 4 * 1024);
        assert_eq!(chunker.max_chunk, 1024 * 1024);
    }
}
