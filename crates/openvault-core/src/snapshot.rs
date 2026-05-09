use serde::{Deserialize, Serialize};

/// Metadata for a single file within a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path from source root.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Last modification time (seconds since epoch).
    pub mtime: i64,
    /// SHA-256 checksum of the file contents.
    pub checksum: String,
}

/// Unique identifier for a snapshot.
pub type SnapshotId = String;

/// A snapshot captures the state of a backup at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unique identifier (timestamp-based by default).
    pub id: SnapshotId,
    /// Timestamp when this snapshot was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The backup strategy that produced this snapshot.
    pub strategy: BackupStrategy,
    /// Source directory that was backed up.
    pub source: String,
    /// Storage backend identifier.
    pub storage_backend: String,
    /// Files contained in this snapshot.
    pub entries: Vec<FileEntry>,
    /// Parent snapshot ID (None for full backups, Some for incremental).
    pub parent_id: Option<SnapshotId>,
    /// Total size of all files in bytes.
    pub total_size: u64,
}

/// The strategy used to create a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackupStrategy {
    Full,
    Incremental,
}

/// Counter for generating unique snapshot IDs within the same second.
static SNAPSHOT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Snapshot {
    /// Create a new snapshot with an auto-generated unique ID.
    pub fn new(
        strategy: BackupStrategy,
        source: String,
        storage_backend: String,
        parent_id: Option<SnapshotId>,
    ) -> Self {
        let created_at = chrono::Utc::now();
        let seq = SNAPSHOT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!(
            "snap-{}-{:04}",
            created_at.format("%Y%m%d%H%M%S"),
            seq
        );
        Self {
            id,
            created_at,
            strategy,
            source,
            storage_backend,
            entries: Vec::new(),
            parent_id,
            total_size: 0,
        }
    }

    /// Add a file entry and update total_size.
    pub fn add_entry(&mut self, entry: FileEntry) {
        self.total_size += entry.size;
        self.entries.push(entry);
    }

    /// Number of files in this snapshot.
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }
}

/// A specific version of a file from a backup snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupEntry {
    /// The snapshot ID this entry belongs to.
    pub snapshot_id: SnapshotId,
    /// The file entry metadata.
    pub file_entry: FileEntry,
    /// When the snapshot was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The backup strategy used.
    pub strategy: BackupStrategy,
}
