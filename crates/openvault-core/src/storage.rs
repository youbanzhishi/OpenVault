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

    /// Check if a specific file exists in a snapshot.
    /// Default implementation tries retrieve_file and checks for error.
    fn file_exists(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<bool> {
        match self.retrieve_file(snapshot_id, rel_path) {
            Ok(_) => Ok(true),
            Err(crate::error::VaultError::SnapshotNotFound(_)) => Ok(false),
            Err(_) => Ok(false),
        }
    }

    /// Count the number of snapshots for a given source.
    fn snapshot_count(&self, source: String) -> VaultResult<u32> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots.into_iter().filter(|s| s.source == source).count() as u32)
    }

    /// Restore all files from a snapshot to the given target directory.
    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> VaultResult<()>;
}

// Blanket impl: allow &dyn VaultStorage where VaultStorage is expected
// This is needed because iterators over Vec<Box<dyn VaultStorage>> yield &Box<dyn VaultStorage>
// which auto-derefs to &dyn VaultStorage, but functions expect impl VaultStorage

// Blanket impl: allow &dyn VaultStorage where VaultStorage is expected
// This is needed because iterators over Vec<Box<dyn VaultStorage>> yield &Box<dyn VaultStorage>
// which auto-derefs to &dyn VaultStorage, but functions expect impl VaultStorage
impl VaultStorage for &dyn VaultStorage {
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> VaultResult<()> {
        (**self).store_file(snapshot_id, rel_path, data)
    }
    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<Vec<u8>> {
        (**self).retrieve_file(snapshot_id, rel_path)
    }
    fn store_snapshot(&self, snapshot: &Snapshot) -> VaultResult<()> {
        (**self).store_snapshot(snapshot)
    }
    fn load_snapshot(&self, id: &str) -> VaultResult<Snapshot> {
        (**self).load_snapshot(id)
    }
    fn list_snapshots(&self) -> VaultResult<Vec<Snapshot>> {
        (**self).list_snapshots()
    }
    fn delete_snapshot(&self, id: &str) -> VaultResult<()> {
        (**self).delete_snapshot(id)
    }
    fn latest_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        (**self).latest_snapshot(source)
    }
    fn latest_full_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        (**self).latest_full_snapshot(source)
    }
    fn backend_name(&self) -> &str {
        (**self).backend_name()
    }
    fn file_exists(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<bool> {
        (**self).file_exists(snapshot_id, rel_path)
    }
    fn snapshot_count(&self, source: String) -> VaultResult<u32> {
        (**self).snapshot_count(source)
    }
    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> VaultResult<()> {
        (**self).restore_snapshot(snapshot, target)
    }
}
