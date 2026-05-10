use std::path::Path;

use sha2::{Digest, Sha256};

use crate::config::BackupConfig;
use crate::engine::BackupEngine;
use crate::error::{VaultError, VaultResult};
use crate::snapshot::{BackupStrategy, FileEntry, Snapshot};
use crate::storage::VaultStorage;

// ---------------------------------------------------------------------------
// Full Backup
// ---------------------------------------------------------------------------

/// Full backup strategy: copies every file from source to storage.
pub struct FullBackup;

impl BackupEngine for FullBackup {
    fn execute(&self, config: &BackupConfig, storage: &dyn VaultStorage) -> VaultResult<Snapshot> {
        let source = &config.source;
        let mut snapshot = Snapshot::new(
            BackupStrategy::Full,
            source.to_string_lossy().to_string(),
            storage.backend_name().to_string(),
            None,
        );

        let files = collect_files(source, &config.exclude)?;
        for file_path in &files {
            let rel = relative_path(source, file_path)?;
            let metadata = std::fs::metadata(file_path)?;
            let checksum = sha256_file(file_path)?;
            let entry = FileEntry {
                path: rel,
                size: metadata.len(),
                mtime: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                checksum,
            };

            let data = std::fs::read(file_path)?;
            storage.store_file(&snapshot.id, &entry.path, &data)?;

            snapshot.add_entry(entry);
        }

        storage.store_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn name(&self) -> &str {
        "full"
    }
}

// ---------------------------------------------------------------------------
// Incremental Backup
// ---------------------------------------------------------------------------

/// Incremental backup strategy: only copies files that changed since the last
/// snapshot. Change detection uses (mtime, size); SHA-256 is used for
/// verification when mtime+size are ambiguous.
///
/// The incremental engine builds a **complete file view** by walking the
/// snapshot chain (via `parent_id`) so that it can correctly detect changes
/// even when the immediately-preceding snapshot was itself an incremental
/// with only a subset of entries.
pub struct IncrementalBackup;

impl IncrementalBackup {
    /// Build a complete file map by walking the snapshot chain.
    /// Later entries (from newer snapshots) override earlier ones.
    fn build_complete_file_map(
        storage: &dyn VaultStorage,
        latest: &Snapshot,
    ) -> VaultResult<std::collections::HashMap<String, FileEntry>> {
        let mut map = std::collections::HashMap::new();
        let mut current = Some(latest.clone());
        while let Some(snap) = current {
            for entry in snap.entries {
                map.insert(entry.path.clone(), entry);
            }
            current = match &snap.parent_id {
                Some(pid) => Some(storage.load_snapshot(pid)?),
                None => None,
            };
        }
        Ok(map)
    }
}

impl BackupEngine for IncrementalBackup {
    fn execute(&self, config: &BackupConfig, storage: &dyn VaultStorage) -> VaultResult<Snapshot> {
        let source = &config.source;
        let source_key = source.to_string_lossy().to_string();

        let parent = storage.latest_snapshot(source_key.clone())?;

        let mut snapshot = Snapshot::new(
            BackupStrategy::Incremental,
            source_key,
            storage.backend_name().to_string(),
            parent.as_ref().map(|p| p.id.clone()),
        );

        let files = collect_files(source, &config.exclude)?;

        let previous_files: std::collections::HashMap<String, FileEntry> = match &parent {
            Some(p) => Self::build_complete_file_map(storage, p)?,
            None => std::collections::HashMap::new(),
        };

        for file_path in &files {
            let rel = relative_path(source, file_path)?;
            let metadata = std::fs::metadata(file_path)?;
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let size = metadata.len();

            let changed = match previous_files.get(&rel) {
                Some(existing) => existing.mtime != mtime || existing.size != size,
                None => true,
            };

            if changed {
                let checksum = sha256_file(file_path)?;
                let entry = FileEntry {
                    path: rel,
                    size,
                    mtime,
                    checksum,
                };
                let data = std::fs::read(file_path)?;
                storage.store_file(&snapshot.id, &entry.path, &data)?;
                snapshot.add_entry(entry);
            }
        }

        storage.store_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn name(&self) -> &str {
        "incremental"
    }
}

// ---------------------------------------------------------------------------
// Differential Backup
// ---------------------------------------------------------------------------

/// Differential backup strategy: copies all files that changed since the
/// last **full** backup. Unlike incremental (which compares against the
/// immediately-previous snapshot), differential always compares against the
/// most recent full backup, so each differential snapshot is self-contained
/// relative to the full base.
pub struct DifferentialBackup;

impl BackupEngine for DifferentialBackup {
    fn execute(&self, config: &BackupConfig, storage: &dyn VaultStorage) -> VaultResult<Snapshot> {
        let source = &config.source;
        let source_key = source.to_string_lossy().to_string();

        // Find the latest FULL snapshot as the reference base
        let base_full = storage.latest_full_snapshot(source_key.clone())?;

        let parent_id = base_full.as_ref().map(|p| p.id.clone());

        let mut snapshot = Snapshot::new(
            BackupStrategy::Differential,
            source_key,
            storage.backend_name().to_string(),
            parent_id,
        );

        let files = collect_files(source, &config.exclude)?;

        // Build file map from the full base snapshot
        let base_files: std::collections::HashMap<String, FileEntry> = match &base_full {
            Some(base) => base
                .entries
                .iter()
                .map(|e| (e.path.clone(), e.clone()))
                .collect(),
            None => std::collections::HashMap::new(),
        };

        for file_path in &files {
            let rel = relative_path(source, file_path)?;
            let metadata = std::fs::metadata(file_path)?;
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let size = metadata.len();

            let changed = match base_files.get(&rel) {
                Some(existing) => existing.mtime != mtime || existing.size != size,
                None => true,
            };

            if changed {
                let checksum = sha256_file(file_path)?;
                let entry = FileEntry {
                    path: rel,
                    size,
                    mtime,
                    checksum,
                };
                let data = std::fs::read(file_path)?;
                storage.store_file(&snapshot.id, &entry.path, &data)?;
                snapshot.add_entry(entry);
            }
        }

        storage.store_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn name(&self) -> &str {
        "differential"
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_files(root: &Path, exclude: &[String]) -> VaultResult<Vec<std::path::PathBuf>> {
    let mut result = Vec::new();
    walk_dir(root, root, exclude, &mut result)?;
    Ok(result)
}

fn walk_dir(
    dir: &Path,
    root: &Path,
    exclude: &[String],
    result: &mut Vec<std::path::PathBuf>,
) -> VaultResult<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = relative_path(root, &path).unwrap_or_default();
        if exclude.iter().any(|pat| glob_match(pat, &rel)) {
            continue;
        }
        if path.is_dir() {
            walk_dir(&path, root, exclude, result)?;
        } else if path.is_file() {
            result.push(path);
        }
    }
    Ok(())
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern.contains("**") {
        let parts: Vec<&str> = pattern.split("**").collect();
        if parts.len() == 2 {
            return text.starts_with(parts[0]) && text.ends_with(parts[1]);
        }
    }
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return text.starts_with(parts[0]) && text.ends_with(parts[1]);
        }
    }
    text.contains(pattern)
}

fn relative_path(base: &Path, path: &Path) -> VaultResult<String> {
    path.strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|_| {
            VaultError::BackupFailed(format!(
                "Cannot compute relative path of {} from {}",
                path.display(),
                base.display()
            ))
        })
}

fn sha256_file(path: &Path) -> VaultResult<String> {
    let data = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_path() {
        let base = Path::new("/tmp/source");
        let path = Path::new("/tmp/source/subdir/file.txt");
        assert_eq!(relative_path(base, path).unwrap(), "subdir/file.txt");
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("*.tmp", "file.tmp"));
        assert!(!glob_match("*.tmp", "file.txt"));
    }

    #[test]
    fn test_glob_match_prefix_star() {
        assert!(glob_match(".git", ".git/config"));
        assert!(glob_match(".git", "some/.git/refs"));
    }

    #[test]
    fn test_differential_engine_name() {
        let diff = DifferentialBackup;
        assert_eq!(diff.name(), "differential");
    }
}
