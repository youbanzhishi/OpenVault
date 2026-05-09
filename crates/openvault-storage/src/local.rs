use std::path::{Path, PathBuf};

use openvault_core::error::{VaultError, VaultResult};
use openvault_core::snapshot::{BackupStrategy, Snapshot};
use openvault_core::storage::VaultStorage;

/// Local filesystem implementation of `VaultStorage`.
///
/// Directory layout:
/// ```text
/// <root>/
/// ├── snapshots/
/// │   ├── snap-20260509120000-0000.json    # snapshot metadata
/// │   └── ...
/// └── data/
///     ├── snap-20260509120000-0000/
///     │   ├── path/to/file.txt            # actual file data
///     │   └── ...
///     └── ...
/// ```
pub struct LocalVaultStorage {
    root: PathBuf,
}

impl LocalVaultStorage {
    /// Create a new local storage backed by `root` directory.
    /// The directory will be created if it doesn't exist.
    pub fn new(root: impl Into<PathBuf>) -> VaultResult<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|e| {
            VaultError::Storage(format!("Failed to create storage root {}: {}", root.display(), e))
        })?;
        std::fs::create_dir_all(root.join("snapshots")).map_err(|e| {
            VaultError::Storage(format!("Failed to create snapshots dir: {}", e))
        })?;
        std::fs::create_dir_all(root.join("data")).map_err(|e| {
            VaultError::Storage(format!("Failed to create data dir: {}", e))
        })?;
        Ok(Self { root })
    }

    fn snapshot_path(&self, id: &str) -> PathBuf {
        self.root.join("snapshots").join(format!("{}.json", id))
    }

    fn data_dir(&self, snapshot_id: &str) -> PathBuf {
        self.root.join("data").join(snapshot_id)
    }
}

impl VaultStorage for LocalVaultStorage {
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> VaultResult<()> {
        let dir = self.data_dir(snapshot_id);
        let file_path = dir.join(rel_path);

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VaultError::Storage(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        std::fs::write(&file_path, data).map_err(|e| {
            VaultError::Storage(format!(
                "Failed to write file {}: {}",
                file_path.display(),
                e
            ))
        })
    }

    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> VaultResult<Vec<u8>> {
        let file_path = self.data_dir(snapshot_id).join(rel_path);
        std::fs::read(&file_path).map_err(|e| {
            VaultError::Storage(format!(
                "Failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })
    }

    fn store_snapshot(&self, snapshot: &Snapshot) -> VaultResult<()> {
        let path = self.snapshot_path(&snapshot.id);
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| {
            VaultError::Storage(format!("Failed to serialize snapshot: {}", e))
        })?;
        std::fs::write(&path, json).map_err(|e| {
            VaultError::Storage(format!(
                "Failed to write snapshot {}: {}",
                path.display(),
                e
            ))
        })
    }

    fn load_snapshot(&self, id: &str) -> VaultResult<Snapshot> {
        let path = self.snapshot_path(id);
        if !path.exists() {
            return Err(VaultError::SnapshotNotFound(id.to_string()));
        }
        let json = std::fs::read_to_string(&path).map_err(|e| {
            VaultError::Storage(format!("Failed to read snapshot {}: {}", path.display(), e))
        })?;
        serde_json::from_str(&json).map_err(|e| {
            VaultError::Storage(format!("Failed to parse snapshot {}: {}", id, e))
        })
    }

    fn list_snapshots(&self) -> VaultResult<Vec<Snapshot>> {
        let snapshots_dir = self.root.join("snapshots");
        if !snapshots_dir.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();
        for entry in std::fs::read_dir(&snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let json = std::fs::read_to_string(&path)?;
                if let Ok(snapshot) = serde_json::from_str::<Snapshot>(&json) {
                    snapshots.push(snapshot);
                }
            }
        }

        // Sort by creation time, newest first
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        Ok(snapshots)
    }

    fn delete_snapshot(&self, id: &str) -> VaultResult<()> {
        let meta_path = self.snapshot_path(id);
        if !meta_path.exists() {
            return Err(VaultError::SnapshotNotFound(id.to_string()));
        }

        // Remove snapshot metadata
        std::fs::remove_file(&meta_path)?;

        // Remove snapshot data directory
        let data_dir = self.data_dir(id);
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir)?;
        }

        Ok(())
    }

    fn latest_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots
            .into_iter()
            .filter(|s| s.source == source)
            .max_by_key(|s| s.created_at))
    }

    fn latest_full_snapshot(&self, source: String) -> VaultResult<Option<Snapshot>> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots
            .into_iter()
            .filter(|s| s.source == source && s.strategy == BackupStrategy::Full)
            .max_by_key(|s| s.created_at))
    }

    fn backend_name(&self) -> &str {
        "local"
    }

    fn restore_snapshot(&self, snapshot: &Snapshot, target: &Path) -> VaultResult<()> {
        std::fs::create_dir_all(target).map_err(|e| {
            VaultError::RestoreFailed(format!(
                "Failed to create target directory {}: {}",
                target.display(),
                e
            ))
        })?;

        match snapshot.strategy {
            BackupStrategy::Full => {
                // Full backup: all files are in this snapshot
                for entry in &snapshot.entries {
                    let data = self.retrieve_file(&snapshot.id, &entry.path)?;
                    let file_path = target.join(&entry.path);

                    if let Some(parent) = file_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }

                    std::fs::write(&file_path, &data).map_err(|e| {
                        VaultError::RestoreFailed(format!(
                            "Failed to restore file {}: {}",
                            file_path.display(),
                            e
                        ))
                    })?;
                }
            }
            BackupStrategy::Incremental | BackupStrategy::Differential => {
                // Incremental/Differential: need to walk the chain to build
                // a complete file view
                let file_map = self.build_complete_file_map(snapshot)?;

                // For each file in the complete view, find which snapshot has it
                // and retrieve from there
                let mut visited = std::collections::HashSet::new();
                let mut current = Some(snapshot.clone());

                // Collect all snapshots in the chain (newest first)
                let mut chain: Vec<Snapshot> = Vec::new();
                while let Some(snap) = current {
                    chain.push(snap.clone());
                    current = match &snap.parent_id {
                        Some(pid) => Some(self.load_snapshot(pid)?),
                        None => None,
                    };
                }

                // For each file in the complete view, restore from the
                // newest snapshot that contains it
                for rel_path in file_map.keys() {
                    if visited.contains(rel_path) {
                        continue;
                    }
                    visited.insert(rel_path.clone());

                    // Find the first (newest) snapshot in the chain that has this file
                    for snap in &chain {
                        if snap.entries.iter().any(|e| e.path == *rel_path) {
                            let data = self.retrieve_file(&snap.id, rel_path)?;
                            let file_path = target.join(rel_path);

                            if let Some(parent) = file_path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }

                            std::fs::write(&file_path, &data).map_err(|e| {
                                VaultError::RestoreFailed(format!(
                                    "Failed to restore file {}: {}",
                                    file_path.display(),
                                    e
                                ))
                            })?;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openvault_core::snapshot::FileEntry;
    use tempfile::TempDir;

    fn make_snapshot(id: &str, strategy: BackupStrategy, entries: Vec<FileEntry>) -> Snapshot {
        let mut snap = Snapshot::new(strategy, "/tmp/source".into(), "local".into(), None);
        snap.id = id.to_string();
        for e in entries {
            snap.add_entry(e);
        }
        snap
    }

    #[test]
    fn test_store_and_load_snapshot() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        let snap = make_snapshot(
            "snap-test001",
            BackupStrategy::Full,
            vec![FileEntry {
                path: "hello.txt".into(),
                size: 5,
                mtime: 1000,
                checksum: "abc".into(),
            }],
        );

        storage.store_snapshot(&snap).unwrap();
        let loaded = storage.load_snapshot("snap-test001").unwrap();
        assert_eq!(loaded.id, "snap-test001");
        assert_eq!(loaded.entries.len(), 1);
    }

    #[test]
    fn test_list_snapshots() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        let snap1 = make_snapshot("snap-a", BackupStrategy::Full, vec![]);
        let snap2 = make_snapshot("snap-b", BackupStrategy::Incremental, vec![]);
        let snap3 = make_snapshot("snap-c", BackupStrategy::Differential, vec![]);

        storage.store_snapshot(&snap1).unwrap();
        storage.store_snapshot(&snap2).unwrap();
        storage.store_snapshot(&snap3).unwrap();

        let list = storage.list_snapshots().unwrap();
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_delete_snapshot() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        let snap = make_snapshot("snap-del", BackupStrategy::Full, vec![]);
        storage.store_snapshot(&snap).unwrap();
        storage.delete_snapshot("snap-del").unwrap();

        assert!(storage.load_snapshot("snap-del").is_err());
    }

    #[test]
    fn test_store_and_retrieve_file() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        storage
            .store_file("snap-001", "subdir/file.txt", b"hello world")
            .unwrap();

        let data = storage.retrieve_file("snap-001", "subdir/file.txt").unwrap();
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn test_restore_snapshot() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        let snap = make_snapshot(
            "snap-restore",
            BackupStrategy::Full,
            vec![
                FileEntry {
                    path: "a.txt".into(),
                    size: 5,
                    mtime: 1000,
                    checksum: "abc".into(),
                },
                FileEntry {
                    path: "sub/b.txt".into(),
                    size: 7,
                    mtime: 2000,
                    checksum: "def".into(),
                },
            ],
        );

        storage.store_file("snap-restore", "a.txt", b"aaa").unwrap();
        storage
            .store_file("snap-restore", "sub/b.txt", b"bbbbb")
            .unwrap();
        storage.store_snapshot(&snap).unwrap();

        let target = TempDir::new().unwrap();
        storage
            .restore_snapshot(&snap, target.path())
            .unwrap();

        assert!(target.path().join("a.txt").exists());
        assert!(target.path().join("sub/b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(target.path().join("a.txt")).unwrap(),
            "aaa"
        );
    }

    #[test]
    fn test_latest_full_snapshot() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        let full_snap = make_snapshot("snap-full", BackupStrategy::Full, vec![]);
        let inc_snap = make_snapshot("snap-inc", BackupStrategy::Incremental, vec![]);
        let diff_snap = make_snapshot("snap-diff", BackupStrategy::Differential, vec![]);

        storage.store_snapshot(&full_snap).unwrap();
        storage.store_snapshot(&inc_snap).unwrap();
        storage.store_snapshot(&diff_snap).unwrap();

        let latest_full = storage.latest_full_snapshot("/tmp/source".to_string()).unwrap();
        assert!(latest_full.is_some());
        assert_eq!(latest_full.unwrap().id, "snap-full");
    }

    #[test]
    fn test_restore_incremental_chain() {
        let dir = TempDir::new().unwrap();
        let storage = LocalVaultStorage::new(dir.path()).unwrap();

        // Full backup with a.txt
        let mut full_snap = Snapshot::new(
            BackupStrategy::Full,
            "/tmp/source".into(),
            "local".into(),
            None,
        );
        full_snap.id = "snap-chain-full".to_string();
        full_snap.add_entry(FileEntry {
            path: "a.txt".into(),
            size: 5,
            mtime: 1000,
            checksum: "abc".into(),
        });
        full_snap.add_entry(FileEntry {
            path: "b.txt".into(),
            size: 5,
            mtime: 1000,
            checksum: "def".into(),
        });

        storage.store_file("snap-chain-full", "a.txt", b"aaa").unwrap();
        storage.store_file("snap-chain-full", "b.txt", b"bbb").unwrap();
        storage.store_snapshot(&full_snap).unwrap();

        // Incremental with modified a.txt (no b.txt entry = unchanged)
        let mut inc_snap = Snapshot::new(
            BackupStrategy::Incremental,
            "/tmp/source".into(),
            "local".into(),
            Some("snap-chain-full".to_string()),
        );
        inc_snap.id = "snap-chain-inc".to_string();
        inc_snap.add_entry(FileEntry {
            path: "a.txt".into(),
            size: 8,
            mtime: 2000,
            checksum: "xyz".into(),
        });

        storage.store_file("snap-chain-inc", "a.txt", b"aaa_new").unwrap();
        storage.store_snapshot(&inc_snap).unwrap();

        // Restore from incremental → should get both a.txt (new) and b.txt (from full)
        let target = TempDir::new().unwrap();
        storage.restore_snapshot(&inc_snap, target.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.path().join("a.txt")).unwrap(),
            "aaa_new"
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join("b.txt")).unwrap(),
            "bbb"
        );
    }
}
