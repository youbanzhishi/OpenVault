use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use openvault_core::config::BackupConfig;
use openvault_core::engine::engine_for_strategy;
use openvault_core::storage::VaultStorage;
use openvault_storage::LocalVaultStorage;

#[derive(Parser)]
#[command(name = "vault", version, about = "OpenVault — intelligent file backup & disaster recovery")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a backup using the given config file
    Backup {
        /// Path to YAML configuration file
        config: PathBuf,
    },
    /// Restore files from a snapshot
    Restore {
        /// Snapshot ID to restore from
        snapshot_id: String,
        /// Target directory to restore into
        #[arg(short, long)]
        target: PathBuf,
        /// Path to YAML configuration file (to locate storage)
        #[arg(short, long)]
        config: PathBuf,
    },
    /// Manage snapshots
    #[command(subcommand)]
    Snapshots(SnapshotCommands),
}

#[derive(Subcommand)]
enum SnapshotCommands {
    /// List all snapshots
    List {
        /// Path to YAML configuration file (to locate storage)
        #[arg(short, long)]
        config: PathBuf,
    },
    /// Delete a snapshot
    Delete {
        /// Snapshot ID to delete
        id: String,
        /// Path to YAML configuration file (to locate storage)
        #[arg(short, long)]
        config: PathBuf,
    },
}

fn build_storage(config: &BackupConfig) -> Result<Box<dyn VaultStorage>> {
    match &config.storage {
        openvault_core::config::StorageConfig::Local { path } => {
            let storage = LocalVaultStorage::new(path)
                .context("Failed to initialize local storage")?;
            Ok(Box::new(storage))
        }
        openvault_core::config::StorageConfig::S3 { .. } => {
            anyhow::bail!("S3 storage backend is not yet implemented")
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Backup { config: config_path } => {
            let config = BackupConfig::load_from_file(&config_path)
                .context("Failed to load backup config")?;
            config.validate().context("Invalid configuration")?;

            let storage = build_storage(&config)?;
            let engine = engine_for_strategy(&config.strategy);

            eprintln!(
                "🚀 Running {} backup: {} → {}",
                engine.name(),
                config.source.display(),
                config.name,
            );

            let snapshot = engine
                .execute(&config, storage.as_ref())
                .context("Backup failed")?;

            println!(
                "✅ Backup complete: snapshot {} ({} files, {} bytes)",
                snapshot.id,
                snapshot.file_count(),
                snapshot.total_size,
            );
        }

        Commands::Restore {
            snapshot_id,
            target,
            config: config_path,
        } => {
            let config = BackupConfig::load_from_file(&config_path)
                .context("Failed to load backup config")?;
            let storage = build_storage(&config)?;

            let snapshot = storage
                .load_snapshot(&snapshot_id)
                .context("Snapshot not found")?;

            eprintln!(
                "📦 Restoring snapshot {} → {}",
                snapshot_id,
                target.display(),
            );

            storage
                .restore_snapshot(&snapshot, &target)
                .context("Restore failed")?;

            println!(
                "✅ Restored {} files from snapshot {}",
                snapshot.file_count(),
                snapshot_id,
            );
        }

        Commands::Snapshots(SnapshotCommands::List { config: config_path }) => {
            let config = BackupConfig::load_from_file(&config_path)
                .context("Failed to load backup config")?;
            let storage = build_storage(&config)?;

            let snapshots = storage.list_snapshots()?;

            if snapshots.is_empty() {
                println!("No snapshots found.");
            } else {
                println!("{:<25} {:<15} {:<8} {:<12} Created", "ID", "Strategy", "Files", "Size");
                println!("{}", "-".repeat(80));
                for snap in &snapshots {
                    println!(
                        "{:<25} {:<15} {:<8} {:<12} {}",
                        snap.id,
                        match snap.strategy {
                            openvault_core::snapshot::BackupStrategy::Full => "full",
                            openvault_core::snapshot::BackupStrategy::Incremental => "incremental",
                        },
                        snap.file_count(),
                        format_bytes(snap.total_size),
                        snap.created_at.format("%Y-%m-%d %H:%M:%S"),
                    );
                }
            }
        }

        Commands::Snapshots(SnapshotCommands::Delete { id, config: config_path }) => {
            let config = BackupConfig::load_from_file(&config_path)
                .context("Failed to load backup config")?;
            let storage = build_storage(&config)?;

            storage.delete_snapshot(&id)?;
            println!("🗑️  Deleted snapshot {}", id);
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
