use crate::config::BackupConfig;
use crate::error::VaultResult;
use crate::snapshot::Snapshot;

/// Core backup engine trait.
///
/// This is the central abstraction of OpenVault. A BackupEngine knows how to
/// execute a backup given a config and a storage backend. Concrete strategies
/// (Full, Incremental, etc.) implement this trait; the core never dispatches
/// on "local" vs "remote" — that is the storage layer's job.
pub trait BackupEngine: Send + Sync {
    /// Execute a backup and return the resulting snapshot.
    fn execute(&self, config: &BackupConfig, storage: &dyn crate::storage::VaultStorage) -> VaultResult<Snapshot>;

    /// Human-readable name of this strategy.
    fn name(&self) -> &str;
}

/// Factory: select a backup engine by strategy name.
pub fn engine_for_strategy(
    strategy: &crate::snapshot::BackupStrategy,
) -> Box<dyn BackupEngine> {
    use crate::strategy::{FullBackup, IncrementalBackup};
    match strategy {
        crate::snapshot::BackupStrategy::Full => Box::new(FullBackup),
        crate::snapshot::BackupStrategy::Incremental => Box::new(IncrementalBackup),
    }
}
