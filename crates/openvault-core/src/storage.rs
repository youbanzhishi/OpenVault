use crate::error::VaultResult;
use crate::snapshot::Snapshot;

/// Abstraction for backup storage backends.
///
/// This trait is the storage spine of OpenVault. It is intentionally aligned
/// with OpenLink's `open-storage::StorageBackend` interface but adds
/// backup-specific operations (snapshot persistence, metadata queries).
///
/// Implementations: `LocalVaultStorage`, `S3VaultStorage`, `R2VaultStorage`, etc.
pub trait VaultStorage: Send + Sync {
    /// Store a file's raw data under the given snapshot ID and relative path.
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> VaultResult<()>;

    /// Retrieve a file's raw data.
    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<Vec<u8>>;

    /// Persist snapshot metadata.
    fn store_snapshot(&self, snapshot: &Snapshot) -> VaultResult<()>;

    /// Load a snapshot by ID.
    fn load_snapshot(&self, id: &str) -> VaultResult<Snapshot>;

    /// List all snapshots.
    fn list_snapshots(&self) -> VaultResult<Vec<Snapshot>>;

    /// Delete a snapshot and its associated files.
    fn delete_snapshot(&self, id: &str) -> VaultResult<()>;

    /// Find the latest snapshot for a given source path.
    fn latest_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>>;

    /// Find the latest **full** snapshot for a given source path.
    fn latest_full_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>>;

    /// Human-readable backend name (e.g., "local", "s3", "r2").
    fn backend_name(&self) -> &str;

    /// Restore all files from a snapshot to the given target directory.
    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> VaultResult<()>;
}
