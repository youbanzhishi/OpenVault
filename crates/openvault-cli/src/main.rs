use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use openvault_core::config::BackupConfig;
use openvault_core::engine::engine_for_strategy;
use openvault_core::healing::{HealingConfig, HealingEngine};
use openvault_core::policy::{Policy321, PolicyEngine};
use openvault_core::restore::{ConflictStrategy, RestoreEngine, RestoreOptions};
use openvault_core::snapshot::BackupStrategy;
use openvault_core::storage::VaultStorage;
use openvault_storage::LocalVaultStorage;

#[derive(Parser)]
#[command(name = "vault", version, about = "OpenVault — intelligent file backup & disaster recovery\n\n狡兔三窟，AI守护，永不丢失")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a backup
    Backup {
        /// Source path to back up
        path: PathBuf,
        /// Backup strategy: full, incremental, differential
        #[arg(short, long, default_value = "full")]
        strategy: String,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
        /// Path to YAML configuration file (alternative to inline args)
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Patterns to exclude (can be repeated)
        #[arg(short = 'e', long = "exclude")]
        excludes: Vec<String>,
    },
    /// Restore files from a snapshot
    Restore {
        /// Snapshot ID to restore from
        snapshot_id: String,
        /// Target directory to restore into
        #[arg(short, long)]
        target: PathBuf,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
        /// Conflict resolution: skip, overwrite, rename, fail
        #[arg(short, long, default_value = "overwrite")]
        conflict: String,
        /// Verify checksums after restore
        #[arg(long, default_value_t = true)]
        verify: bool,
    },
    /// Verify integrity of a snapshot
    Verify {
        /// Snapshot ID to verify
        snapshot_id: String,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// Show backup policy health status
    Status {
        /// Source path to check
        #[arg(short, long)]
        source: Option<String>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Manage snapshots
    #[command(subcommand)]
    Snapshots(SnapshotCommands),
    /// Manage registered devices
    #[command(subcommand)]
    Device(DeviceCommands),
    /// Self-healing operations
    #[command(subcommand)]
    Heal(HealCommands),
}

#[derive(Subcommand)]
enum SnapshotCommands {
    /// List all snapshots
    List {
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// Delete a snapshot
    Delete {
        /// Snapshot ID to delete
        id: String,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// Show snapshot details
    Info {
        /// Snapshot ID to inspect
        id: String,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum DeviceCommands {
    /// List registered devices
    List,
    /// Register current device
    Register {
        /// Device name
        #[arg(short, long)]
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum HealCommands {
    /// Scan snapshots for corruption
    Scan {
        /// Snapshot ID to scan (omit for all)
        snapshot_id: Option<String>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// Heal corrupt files from a healthy replica
    Repair {
        /// Snapshot ID to repair
        snapshot_id: String,
        /// Source storage path (healthy replica)
        #[arg(long)]
        source_storage: PathBuf,
        /// Target storage path (corrupt replica)
        #[arg(long)]
        target_storage: PathBuf,
    },
}

/// Resolve storage from CLI args or config file.
fn resolve_storage(
    config_path: Option<&PathBuf>,
    storage_path: Option<&PathBuf>,
) -> Result<Box<dyn VaultStorage>> {
    match (config_path, storage_path) {
        (Some(cfg), _) => {
            let config = BackupConfig::load_from_file(cfg)
                .context("Failed to load backup config")?;
            match &config.storage {
                openvault_core::config::StorageConfig::Local { path } => {
                    let storage = LocalVaultStorage::new(path)
                        .context("Failed to initialize local storage")?;
                    Ok(Box::new(storage))
                }
                openvault_core::config::StorageConfig::S3 { bucket, prefix, endpoint, region, access_key_id, secret_access_key } => {
                    let storage = openvault_storage::S3VaultStorage::new(
                        bucket.clone(),
                        prefix.clone(),
                        endpoint.clone(),
                        region.clone(),
                        access_key_id.clone(),
                        secret_access_key.clone(),
                    );
                    Ok(Box::new(storage))
                }
                openvault_core::config::StorageConfig::R2 { bucket, prefix, account_id, access_key_id, secret_access_key } => {
                    let storage = openvault_storage::R2VaultStorage::new(
                        account_id.clone(),
                        bucket.clone(),
                        prefix.clone(),
                        access_key_id.clone(),
                        secret_access_key.clone(),
                    );
                    Ok(Box::new(storage))
                }
            }
        }
        (_, Some(path)) => {
            let storage = LocalVaultStorage::new(path)
                .context("Failed to initialize local storage")?;
            Ok(Box::new(storage))
        }
        (None, None) => {
            anyhow::bail!("Either --config or --storage must be specified")
        }
    }
}

fn parse_strategy(s: &str) -> Result<BackupStrategy> {
    match s.to_lowercase().as_str() {
        "full" => Ok(BackupStrategy::Full),
        "incremental" | "inc" => Ok(BackupStrategy::Incremental),
        "differential" | "diff" => Ok(BackupStrategy::Differential),
        _ => anyhow::bail!("Unknown strategy: {}. Use full, incremental, or differential", s),
    }
}

fn parse_conflict(s: &str) -> Result<ConflictStrategy> {
    match s.to_lowercase().as_str() {
        "skip" => Ok(ConflictStrategy::Skip),
        "overwrite" | "over" => Ok(ConflictStrategy::Overwrite),
        "rename" => Ok(ConflictStrategy::Rename),
        "fail" => Ok(ConflictStrategy::Fail),
        _ => anyhow::bail!("Unknown conflict strategy: {}. Use skip, overwrite, rename, or fail", s),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // ── Backup ──────────────────────────────────────────────────────
        Commands::Backup { path, strategy, storage, config, excludes } => {
            let strategy = parse_strategy(&strategy)?;

            let (storage_box, config_obj) = if let Some(cfg_path) = &config {
                let cfg = BackupConfig::load_from_file(cfg_path)
                    .context("Failed to load config")?;
                let stor = resolve_storage(Some(cfg_path), None)?;
                (stor, cfg)
            } else {
                let storage_path = storage
                    .unwrap_or_else(|| path.join("../.openvault-vault"));
                let stor = LocalVaultStorage::new(&storage_path)
                    .context("Failed to initialize storage")?;
                let cfg = BackupConfig {
                    name: format!("backup-{}", path.display()),
                    source: path.clone(),
                    storage: openvault_core::config::StorageConfig::Local {
                        path: storage_path,
                    },
                    strategy: strategy.clone(),
                    exclude: excludes,
                    schedule: None,
                };
                (Box::new(stor) as Box<dyn VaultStorage>, cfg)
            };

            let engine = engine_for_strategy(&strategy);

            eprintln!(
                "🚀 Running {} backup: {} → {}",
                engine.name(),
                config_obj.source.display(),
                config_obj.name,
            );

            let snapshot = engine
                .execute(&config_obj, &*storage_box)
                .context("Backup failed")?;

            println!(
                "✅ Backup complete: snapshot {} ({} files, {} bytes)",
                snapshot.id,
                snapshot.file_count(),
                snapshot.total_size,
            );
        }

        // ── Restore ─────────────────────────────────────────────────────
        Commands::Restore { snapshot_id, target, config, storage, conflict, verify } => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            let conflict_strategy = parse_conflict(&conflict)?;

            let snapshot = storage_box
                .load_snapshot(&snapshot_id)
                .context("Snapshot not found")?;

            eprintln!(
                "📦 Restoring snapshot {} → {}",
                snapshot_id,
                target.display(),
            );

            let restore_engine = RestoreEngine::new(Arc::from(storage_box));
            let options = RestoreOptions {
                target: target.clone(),
                conflict: conflict_strategy,
                verify_checksums: verify,
                ..Default::default()
            };

            let report = restore_engine
                .restore(&snapshot, options)
                .await
                .context("Restore failed")?;

            if report.is_success() {
                println!(
                    "✅ Restored {} files from snapshot {} ({})",
                    report.files_restored,
                    snapshot_id,
                    report.summary(),
                );
            } else {
                eprintln!(
                    "⚠️  Restore completed with issues: {}",
                    report.summary()
                );
            }
        }

        // ── Verify ──────────────────────────────────────────────────────
        Commands::Verify { snapshot_id, config, storage } => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;

            let snapshot = storage_box
                .load_snapshot(&snapshot_id)
                .context("Snapshot not found")?;

            eprintln!("🔍 Verifying snapshot {}...", snapshot_id);

            let restore_engine = RestoreEngine::new(Arc::from(storage_box));
            let report = restore_engine
                .verify(&snapshot)
                .await
                .context("Verification failed")?;

            if report.is_ok() {
                println!(
                    "✅ Snapshot {} verified: {}",
                    snapshot_id,
                    report.summary()
                );
            } else {
                eprintln!(
                    "❌ Snapshot {} has issues: {}",
                    snapshot_id,
                    report.summary()
                );
                for err in &report.errors {
                    eprintln!("  - {}: {}", err.path, err.message);
                }
                std::process::exit(1);
            }
        }

        // ── Status (3-2-1 policy health) ───────────────────────────────
        Commands::Status { source, storage, config } => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            let policy = Policy321::default();
            let engine = PolicyEngine::new(policy);

            let source = source.unwrap_or_else(|| "/".to_string());
            let health = engine.check_health(&source, &[&*storage_box])?;

            println!("{}", health.summary());

            if !health.violations.is_empty() {
                eprintln!("\n⚠️  Violations:");
                for v in &health.violations {
                    eprintln!("  - {}", v.message);
                }
            }

            if !health.remediation.is_empty() {
                eprintln!("\n💡 Suggested actions:");
                for r in &health.remediation {
                    eprintln!("  - {}", r.description);
                }
            }

            println!("\n📊 Backends: {}", health.backend_names.join(", "));
        }

        // ── Snapshots ──────────────────────────────────────────────────
        Commands::Snapshots(SnapshotCommands::List { config, storage }) => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            let snapshots = storage_box.list_snapshots()?;

            if snapshots.is_empty() {
                println!("No snapshots found.");
            } else {
                println!("{:<30} {:<15} {:<8} {:<12} Created", "ID", "Strategy", "Files", "Size");
                println!("{}", "-".repeat(85));
                for snap in &snapshots {
                    println!(
                        "{:<30} {:<15} {:<8} {:<12} {}",
                        snap.id,
                        match snap.strategy {
                            BackupStrategy::Full => "full",
                            BackupStrategy::Incremental => "incremental",
                            BackupStrategy::Differential => "differential",
                        },
                        snap.file_count(),
                        format_bytes(snap.total_size),
                        snap.created_at.format("%Y-%m-%d %H:%M:%S"),
                    );
                }
                println!("\nTotal: {} snapshots", snapshots.len());
            }
        }

        Commands::Snapshots(SnapshotCommands::Delete { id, config, storage }) => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            storage_box.delete_snapshot(&id)?;
            println!("🗑️  Deleted snapshot {}", id);
        }

        Commands::Snapshots(SnapshotCommands::Info { id, config, storage }) => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            let snap = storage_box.load_snapshot(&id)
                .context("Snapshot not found")?;

            println!("Snapshot: {}", snap.id);
            println!("  Strategy:  {:?}", snap.strategy);
            println!("  Source:    {}", snap.source);
            println!("  Backend:   {}", snap.storage_backend);
            println!("  Created:   {}", snap.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
            println!("  Files:     {}", snap.file_count());
            println!("  Total size: {}", format_bytes(snap.total_size));
            if let Some(parent) = &snap.parent_id {
                println!("  Parent:    {}", parent);
            }
            println!("  Entries:");
            for entry in &snap.entries {
                println!("    {} ({} bytes, checksum: {}..)",
                    entry.path,
                    entry.size,
                    &entry.checksum.chars().take(8).collect::<String>(),
                );
            }
        }

        // ── Device ─────────────────────────────────────────────────────
        Commands::Device(DeviceCommands::List) => {
            println!("📱 Device management requires OpenLink server connection.");
            println!("   Use 'openvault-server' for multi-device management.");
        }

        Commands::Device(DeviceCommands::Register { name }) => {
            let device_name = name.unwrap_or_else(|| {
                hostname::get()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "unknown".to_string())
            });
            println!("📱 Device '{}' registered locally.", device_name);
            println!("   For multi-device sync, connect to OpenLink server.");
        }

        // ── Heal ───────────────────────────────────────────────────────
        Commands::Heal(HealCommands::Scan { snapshot_id, storage }) => {
            let default_storage = PathBuf::from(".openvault-vault");
            let storage_path = storage.as_ref().unwrap_or(&default_storage);
            let storage_box: Box<dyn VaultStorage> = Box::new(LocalVaultStorage::new(storage_path)
                .context("Failed to initialize storage")?);

            if let Some(sid) = snapshot_id {
                let snapshot = storage_box.load_snapshot(&sid)
                    .context("Snapshot not found")?;
                let result = HealingEngine::scan(&*storage_box, &snapshot)?;
                println!("{}", result.summary());

                for check in &result.checks {
                    if !check.healthy {
                        eprintln!("  ❌ {} - {}", check.path,
                            check.error.as_deref().unwrap_or("corrupt"));
                    }
                }
            } else {
                let results = HealingEngine::scan_all(&*storage_box)?;
                for result in &results {
                    println!("{}", result.summary());
                }
                let total_corrupt: u32 = results.iter().map(|r| r.files_corrupt).sum();
                if total_corrupt > 0 {
                    eprintln!("\n⚠️  Found {} corrupt files across all snapshots", total_corrupt);
                } else {
                    println!("\n✅ All snapshots are healthy");
                }
            }
        }

        Commands::Heal(HealCommands::Repair { snapshot_id, source_storage, target_storage }) => {
            let source: Box<dyn VaultStorage> = Box::new(LocalVaultStorage::new(&source_storage)
                .context("Failed to initialize source storage")?);
            let target: Box<dyn VaultStorage> = Box::new(LocalVaultStorage::new(&target_storage)
                .context("Failed to initialize target storage")?);

            let snapshot = target.load_snapshot(&snapshot_id)
                .or_else(|_| source.load_snapshot(&snapshot_id))
                .context("Snapshot not found in either storage")?;

            let config = HealingConfig::default();
            let result = HealingEngine::heal(&*target, &*source, &snapshot, &config)?;

            if result.files_healed > 0 {
                println!("🔧 {}", result.summary());
            } else {
                println!("✅ No healing needed for snapshot {}", snapshot_id);
            }
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
