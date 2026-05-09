use crate::error::VaultResult;
use crate::snapshot::{FileEntry, Snapshot};

/// Abstraction for backup storage backends.
///
/// This trait is the storage spine of OpenVault. It is intentionally aligned
/// with OpenLink's `open-storage::StorageBackend` interface but adds
/// backup-specific operations (snapshot persistence, metadata queries).
///
/// Implementations: `LocalVaultStorage`, `S3VaultStorage`, etc.
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

    /// Find the latest full snapshot for a given source path.
    fn latest_full_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>>;

    /// Human-readable backend name (e.g., "local", "s3").
    fn backend_name(&self) -> &str;

    /// Restore all files from a snapshot to the given target directory.
    /// For incremental/differential snapshots, this walks the chain to build
    /// a complete file view before restoring.
    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> VaultResult<()>;

    /// Build a complete file map by walking the snapshot chain.
    /// Newer entries take precedence over older ones.
    /// We walk newest→oldest, and only insert if the key doesn't already exist,
    /// so the first (newest) entry wins.
    fn build_complete_file_map(
        &self,
        snapshot: &Snapshot,
    ) -> VaultResult<std::collections::HashMap<String, FileEntry>> {
        let mut map = std::collections::HashMap::new();
        let mut current = Some(snapshot.clone());
        while let Some(snap) = current {
            for entry in snap.entries {
                // Only insert if not already present (newer snapshot wins)
                map.entry(entry.path.clone()).or_insert(entry);
            }
            current = match &snap.parent_id {
                Some(pid) => Some(self.load_snapshot(pid)?),
                None => None,
            };
        }
        Ok(map)
    }
}
