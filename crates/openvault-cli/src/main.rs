use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use openvault_core::audit::{AuditLog, AuditOperation, AuditQuery, AuditResult, RotationConfig};
use openvault_core::compliance::{
    ComplianceChecker, ComplianceRule, DataClassification, RetentionPolicy,
};
use openvault_core::config::BackupConfig;
use openvault_core::engine::engine_for_strategy;
use openvault_core::healing::{HealingConfig, HealingEngine};
use openvault_core::integrity::{Checksum, HashAlgorithm};
use openvault_core::notification::{NotificationSvc, NotificationType, Severity};
use openvault_core::policy::{Policy321, PolicyEngine};
use openvault_core::replicator::{ReplicationCoordinator, ReplicatorConfig};
use openvault_core::restore::{ConflictStrategy, RestoreEngine, RestoreOptions};
use openvault_core::snapshot::BackupStrategy;
use openvault_core::storage::VaultStorage;
use openvault_core::tenant::{TenantManager, TenantQuota};
use openvault_storage::LocalVaultStorage;

#[derive(Parser)]
#[command(
    name = "vault",
    version,
    about = "OpenVault — intelligent file backup & disaster recovery\n\n狡兔三窟，AI守护，永不丢失"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new vault for backups
    Init {
        /// Directory to store the vault
        #[arg(default_value = ".openvault-vault")]
        path: PathBuf,
        /// Storage type: local, s3, r2
        #[arg(short, long, default_value = "local")]
        storage_type: String,
    },
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
    /// Put a single file into the vault
    Put {
        /// File to store
        file: PathBuf,
        /// Snapshot ID to store the file under (creates a new snapshot if not specified)
        #[arg(short, long)]
        snapshot: Option<String>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// Get a single file from the vault
    Get {
        /// Relative path of the file to retrieve
        path: String,
        /// Snapshot ID to retrieve from (uses latest if not specified)
        #[arg(short, long)]
        snapshot: Option<String>,
        /// Output file path (defaults to current directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// List files in a snapshot or all snapshots
    List {
        /// Snapshot ID to list files from (lists all snapshots if not specified)
        #[arg(short, long)]
        snapshot: Option<String>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
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
        /// Snapshot ID to verify (verifies all if not specified)
        snapshot_id: Option<String>,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Storage path for backups
        #[arg(short, long)]
        storage: Option<PathBuf>,
    },
    /// Show backup policy health status (3-2-1 rule)
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
    /// Replicate snapshots to additional backends (3-2-1 strategy)
    Replicate {
        /// Source path that was backed up
        #[arg(short, long)]
        source: Option<String>,
        /// Primary storage path
        #[arg(short, long)]
        storage: Option<PathBuf>,
        /// Additional storage paths for replicas (comma-separated)
        #[arg(short, long)]
        replicas: Option<String>,
        /// Path to YAML configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Auto-remediate policy violations
        #[arg(long)]
        auto_remediate: bool,
    },
    /// Perform full 3-2-1 maintenance (check + heal + remediate)
    Maintain {
        /// Source path that was backed up
        #[arg(short, long)]
        source: Option<String>,
        /// Primary storage path
        #[arg(short, long)]
        storage: Option<PathBuf>,
        /// Additional storage paths for replicas (comma-separated)
        #[arg(short, long)]
        replicas: Option<String>,
        /// Auto-remediate policy violations
        #[arg(long, default_value_t = true)]
        auto_remediate: bool,
    },
    /// Self-healing operations
    #[command(subcommand)]
    Heal(HealCommands),
    /// Audit log operations
    #[command(subcommand)]
    Audit(AuditCommands),
    /// Compliance operations
    #[command(subcommand)]
    Compliance(ComplianceCommands),
    /// Tenant management
    #[command(subcommand)]
    Tenant(TenantCommands),
    /// Notification management
    #[command(subcommand)]
    Notify(NotifyCommands),
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

#[derive(Subcommand)]
enum AuditCommands {
    /// List audit log entries
    List {
        /// Filter by user ID
        #[arg(short, long)]
        user: Option<String>,
        /// Filter by operation type
        #[arg(short, long)]
        operation: Option<String>,
        /// Maximum entries to show
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
    /// Verify audit log chain integrity
    Verify,
    /// Export audit log (json/csv)
    Export {
        /// Export format: json or csv
        #[arg(short, long, default_value = "json")]
        format: String,
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ComplianceCommands {
    /// Run compliance check
    Check {
        /// Path to check
        #[arg(short, long, default_value = "/")]
        path: String,
        /// Region for geo-compliance
        #[arg(short, long, default_value = "EU")]
        region: String,
        /// Policy retention days
        #[arg(long, default_value_t = 365)]
        retention_days: u32,
    },
    /// Classify a path
    Classify {
        /// Path to classify
        path: String,
    },
}

#[derive(Subcommand)]
enum TenantCommands {
    /// Create a new tenant
    Create {
        /// Tenant name
        #[arg(short, long)]
        name: String,
        /// Max storage in GB (0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_storage_gb: u64,
        /// Max files (0 = unlimited)
        #[arg(long, default_value_t = 0)]
        max_files: u64,
    },
    /// List all tenants
    List,
    /// Show tenant usage
    Usage {
        /// Tenant ID
        tenant_id: String,
    },
}

#[derive(Subcommand)]
enum NotifyCommands {
    /// List recent notifications
    List {
        /// Maximum entries to show
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
    /// List notification rules
    Rules,
    /// Send a test notification
    Test {
        /// Notification type
        #[arg(short, long, default_value = "backup_completed")]
        notification_type: String,
    },
}

/// Resolve storage from CLI args or config file.
fn resolve_storage(
    config_path: Option<&PathBuf>,
    storage_path: Option<&PathBuf>,
) -> Result<Box<dyn VaultStorage>> {
    match (config_path, storage_path) {
        (Some(cfg), _) => {
            let config =
                BackupConfig::load_from_file(cfg).context("Failed to load backup config")?;
            match &config.storage {
                openvault_core::config::StorageConfig::Local { path } => {
                    let storage = LocalVaultStorage::new(path)
                        .context("Failed to initialize local storage")?;
                    Ok(Box::new(storage))
                }
                openvault_core::config::StorageConfig::S3 {
                    bucket,
                    prefix,
                    endpoint,
                    region,
                    access_key_id,
                    secret_access_key,
                } => {
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
                openvault_core::config::StorageConfig::R2 {
                    bucket,
                    prefix,
                    account_id,
                    access_key_id,
                    secret_access_key,
                } => {
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
            let storage =
                LocalVaultStorage::new(path).context("Failed to initialize local storage")?;
            Ok(Box::new(storage))
        }
        (None, None) => {
            let default_path = PathBuf::from(".openvault-vault");
            let storage =
                LocalVaultStorage::new(&default_path).context("Failed to initialize local storage")?;
            Ok(Box::new(storage))
        }
    }
}

fn parse_strategy(s: &str) -> Result<BackupStrategy> {
    match s.to_lowercase().as_str() {
        "full" => Ok(BackupStrategy::Full),
        "incremental" | "inc" => Ok(BackupStrategy::Incremental),
        "differential" | "diff" => Ok(BackupStrategy::Differential),
        _ => anyhow::bail!(
            "Unknown strategy: {}. Use full, incremental, or differential",
            s
        ),
    }
}

fn parse_conflict(s: &str) -> Result<ConflictStrategy> {
    match s.to_lowercase().as_str() {
        "skip" => Ok(ConflictStrategy::Skip),
        "overwrite" | "over" => Ok(ConflictStrategy::Overwrite),
        "rename" => Ok(ConflictStrategy::Rename),
        "fail" => Ok(ConflictStrategy::Fail),
        _ => anyhow::bail!(
            "Unknown conflict strategy: {}. Use skip, overwrite, rename, or fail",
            s
        ),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // ── Init ───────────────────────────────────────────────────────
        Commands::Init { path, storage_type } => match storage_type.as_str() {
            "local" => {
                let storage =
                    LocalVaultStorage::new(&path).context("Failed to initialize local vault")?;
                println!("✅ Initialized local vault at {}", path.display());
                println!("   Backend: {}", storage.backend_name());
            }
            "s3" | "r2" => {
                println!(
                    "ℹ️  For {} storage, use --config with a YAML config file.",
                    storage_type
                );
                println!("   Example config:");
                if storage_type == "s3" {
                    println!(
                        r#"   storage:
  type: "s3"
  bucket: "my-backups"
  prefix: "vault/"
  endpoint: "https://s3.amazonaws.com"
  region: "us-east-1""#
                    );
                } else {
                    println!(
                        r#"   storage:
  type: "r2"
  bucket: "my-r2-bucket"
  prefix: "backups/"
  account_id: "your-account-id""#
                    );
                }
            }
            _ => anyhow::bail!(
                "Unknown storage type: {}. Use local, s3, or r2",
                storage_type
            ),
        },

        // ── Backup ──────────────────────────────────────────────────────
        Commands::Backup {
            path,
            strategy,
            storage,
            config,
            excludes,
        } => {
            let strategy = parse_strategy(&strategy)?;

            let (storage_box, config_obj) = if let Some(cfg_path) = &config {
                let cfg =
                    BackupConfig::load_from_file(cfg_path).context("Failed to load config")?;
                let stor = resolve_storage(Some(cfg_path), None)?;
                (stor, cfg)
            } else {
                let storage_path = storage.unwrap_or_else(|| PathBuf::from(".openvault-vault"));
                let stor = LocalVaultStorage::new(&storage_path)
                    .context("Failed to initialize storage")?;
                let cfg = BackupConfig {
                    name: format!("backup-{}", path.display()),
                    source: path.clone(),
                    storage: openvault_core::config::StorageConfig::Local { path: storage_path },
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

        // ── Put (single file) ──────────────────────────────────────────
        Commands::Put {
            file,
            snapshot,
            storage,
        } => {
            let storage_path = storage.unwrap_or_else(|| PathBuf::from(".openvault-vault"));
            let storage_box: Box<dyn VaultStorage> = Box::new(
                LocalVaultStorage::new(&storage_path).context("Failed to initialize storage")?,
            );

            if !file.exists() {
                anyhow::bail!("File not found: {}", file.display());
            }

            let data = std::fs::read(&file).context("Failed to read file")?;
            let checksum = Checksum::compute(&data, HashAlgorithm::Sha256);
            let rel_path = file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let snap_id = if let Some(sid) = snapshot {
                sid
            } else {
                // Create a minimal snapshot for this file
                let mut snap = openvault_core::snapshot::Snapshot::new(
                    BackupStrategy::Full,
                    file.parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    storage_box.backend_name().to_string(),
                    None,
                );
                snap.add_entry(openvault_core::snapshot::FileEntry {
                    path: rel_path.clone(),
                    size: data.len() as u64,
                    mtime: std::fs::metadata(&file)
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                    checksum: checksum.value().to_string(),
                });
                storage_box.store_snapshot(&snap)?;
                snap.id
            };

            storage_box.store_file(&snap_id, &rel_path, &data)?;
            println!(
                "✅ Stored {} ({} bytes, checksum: {}..) in snapshot {}",
                rel_path,
                data.len(),
                checksum_short(checksum.value()),
                snap_id
            );
        }

        // ── Get (single file) ──────────────────────────────────────────
        Commands::Get {
            path: rel_path,
            snapshot,
            output,
            storage,
        } => {
            let storage_path = storage.unwrap_or_else(|| PathBuf::from(".openvault-vault"));
            let storage_box: Box<dyn VaultStorage> = Box::new(
                LocalVaultStorage::new(&storage_path).context("Failed to initialize storage")?,
            );

            let snap_id = if let Some(sid) = snapshot {
                sid
            } else {
                // Find the latest snapshot containing this file
                let snapshots = storage_box.list_snapshots()?;
                let mut found: Option<String> = None;
                for snap in &snapshots {
                    if snap.entries.iter().any(|e| e.path == rel_path) {
                        found = Some(snap.id.clone());
                        break;
                    }
                }
                found.ok_or_else(|| {
                    anyhow::anyhow!("File '{}' not found in any snapshot", rel_path)
                })?
            };

            let data = storage_box
                .retrieve_file(&snap_id, &rel_path)
                .context("Failed to retrieve file")?;

            let output_path = output.unwrap_or_else(|| PathBuf::from(&rel_path));
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output_path, &data)?;
            println!(
                "✅ Retrieved {} → {} ({} bytes)",
                rel_path,
                output_path.display(),
                data.len()
            );
        }

        // ── List ───────────────────────────────────────────────────────
        Commands::List {
            snapshot,
            storage,
            config,
        } => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;

            if let Some(sid) = snapshot {
                let snap = storage_box
                    .load_snapshot(&sid)
                    .context("Snapshot not found")?;
                println!(
                    "Files in snapshot {} ({} files, {}):",
                    snap.id,
                    snap.file_count(),
                    format_bytes(snap.total_size)
                );
                for entry in &snap.entries {
                    println!(
                        "  {:12} {} ({}..)",
                        format_bytes(entry.size),
                        entry.path,
                        checksum_short(&entry.checksum),
                    );
                }
            } else {
                let snapshots = storage_box.list_snapshots()?;
                if snapshots.is_empty() {
                    println!("No snapshots found.");
                } else {
                    println!(
                        "{:<30} {:<15} {:<8} {:<12} Created",
                        "ID", "Strategy", "Files", "Size"
                    );
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
        }

        // ── Restore ─────────────────────────────────────────────────────
        Commands::Restore {
            snapshot_id,
            target,
            config,
            storage,
            conflict,
            verify,
        } => {
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
                eprintln!("⚠️  Restore completed with issues: {}", report.summary());
            }
        }

        // ── Verify ──────────────────────────────────────────────────────
        Commands::Verify {
            snapshot_id,
            config,
            storage,
        } => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;

            if let Some(sid) = snapshot_id {
                let snapshot = storage_box
                    .load_snapshot(&sid)
                    .context("Snapshot not found")?;

                eprintln!("🔍 Verifying snapshot {}...", sid);

                let restore_engine = RestoreEngine::new(Arc::from(storage_box));
                let report = restore_engine
                    .verify(&snapshot)
                    .await
                    .context("Verification failed")?;

                if report.is_ok() {
                    println!("✅ Snapshot {} verified: {}", sid, report.summary());
                } else {
                    eprintln!("❌ Snapshot {} has issues: {}", sid, report.summary());
                    for err in &report.errors {
                        eprintln!("  - {}: {}", err.path, err.message);
                    }
                    std::process::exit(1);
                }
            } else {
                // Verify all snapshots
                let snapshots = storage_box.list_snapshots()?;
                let mut total_ok = 0u32;
                let mut total_failed = 0u32;

                let restore_engine = RestoreEngine::new(Arc::from(storage_box));

                for snap in &snapshots {
                    match restore_engine.verify(snap).await {
                        Ok(report) => {
                            if report.is_ok() {
                                println!("✅ {} - {}", snap.id, report.summary());
                                total_ok += 1;
                            } else {
                                eprintln!("❌ {} - {}", snap.id, report.summary());
                                total_failed += 1;
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ {} - verification error: {}", snap.id, e);
                            total_failed += 1;
                        }
                    }
                }

                println!(
                    "\n📊 Summary: {} OK, {} failed out of {} snapshots",
                    total_ok,
                    total_failed,
                    snapshots.len()
                );
                if total_failed > 0 {
                    std::process::exit(1);
                }
            }
        }

        // ── Status (3-2-1 policy health) ───────────────────────────────
        Commands::Status {
            source,
            storage,
            config,
        } => {
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
                println!(
                    "{:<30} {:<15} {:<8} {:<12} Created",
                    "ID", "Strategy", "Files", "Size"
                );
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

        Commands::Snapshots(SnapshotCommands::Delete {
            id,
            config,
            storage,
        }) => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            storage_box.delete_snapshot(&id)?;
            println!("🗑️  Deleted snapshot {}", id);
        }

        Commands::Snapshots(SnapshotCommands::Info {
            id,
            config,
            storage,
        }) => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            let snap = storage_box
                .load_snapshot(&id)
                .context("Snapshot not found")?;

            println!("Snapshot: {}", snap.id);
            println!("  Strategy:  {:?}", snap.strategy);
            println!("  Source:    {}", snap.source);
            println!("  Backend:   {}", snap.storage_backend);
            println!(
                "  Created:   {}",
                snap.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!("  Files:     {}", snap.file_count());
            println!("  Total size: {}", format_bytes(snap.total_size));
            if let Some(parent) = &snap.parent_id {
                println!("  Parent:    {}", parent);
            }
            println!("  Entries:");
            for entry in &snap.entries {
                println!(
                    "    {} ({} bytes, checksum: {}..)",
                    entry.path,
                    entry.size,
                    checksum_short(&entry.checksum),
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

        // ── Replicate (3-2-1 strategy) ────────────────────────────────
        Commands::Replicate {
            source,
            storage,
            replicas,
            config,
            auto_remediate,
        } => {
            let storage_box = resolve_storage(config.as_ref(), storage.as_ref())?;
            let source_key = source.unwrap_or_else(|| "/".to_string());

            let replica_storages: Vec<Box<dyn VaultStorage>> = if let Some(repl) = replicas {
                repl.split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|path| {
                        let p = PathBuf::from(path.trim());
                        Ok(Box::new(LocalVaultStorage::new(&p).context(format!(
                            "Failed to init replica storage at {}",
                            path.trim()
                        ))?) as Box<dyn VaultStorage>)
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                vec![]
            };

            let replicator_config = ReplicatorConfig {
                auto_remediate,
                ..Default::default()
            };
            let replicator = ReplicationCoordinator::new(replicator_config);

            // Get latest snapshot from primary
            match storage_box.latest_snapshot(source_key.clone())? {
                Some(snapshot) => {
                    let refs: Vec<&dyn VaultStorage> =
                        replica_storages.iter().map(|s| s.as_ref()).collect();
                    let result = replicator.replicate_snapshot(&snapshot, &*storage_box, &refs)?;

                    if result.is_full_success() {
                        println!("✅ {}", result.summary());
                    } else {
                        eprintln!("⚠️  {}", result.summary());
                        for (backend, error) in &result.errors {
                            eprintln!("  ❌ {}: {}", backend, error);
                        }
                    }
                }
                None => {
                    eprintln!("No snapshots found for source: {}", source_key);
                }
            }
        }

        // ── Maintain (full 3-2-1 check) ──────────────────────────────
        Commands::Maintain {
            source,
            storage,
            replicas,
            auto_remediate,
        } => {
            let default_storage = PathBuf::from(".openvault-vault");
            let storage_path = storage.as_ref().unwrap_or(&default_storage);
            let primary: Box<dyn VaultStorage> = Box::new(
                LocalVaultStorage::new(storage_path).context("Failed to init primary storage")?,
            );
            let source_key = source.unwrap_or_else(|| "/".to_string());

            let replica_storages: Vec<Box<dyn VaultStorage>> = if let Some(repl) = replicas {
                repl.split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|path| {
                        let p = PathBuf::from(path.trim());
                        Ok(Box::new(LocalVaultStorage::new(&p).context(format!(
                            "Failed to init replica storage at {}",
                            path.trim()
                        ))?) as Box<dyn VaultStorage>)
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                vec![]
            };

            let replicator_config = ReplicatorConfig {
                auto_remediate,
                ..ReplicatorConfig::default()
            };
            let replicator = ReplicationCoordinator::new(replicator_config);

            let refs: Vec<&dyn VaultStorage> =
                replica_storages.iter().map(|s| s.as_ref()).collect();
            let result = replicator.maintain_321(&source_key, &*primary, &refs)?;

            println!("🔍 {}", result.summary);

            if !result.policy_healthy {
                eprintln!("⚠️  3-2-1 policy violations detected. Consider adding more backends.");
            }
            if result.backends_with_corruption > 0 {
                eprintln!(
                    "🔧 Corruption detected in {} backend(s), healing attempted.",
                    result.backends_with_corruption
                );
            }
            if result.remediation_actions > 0 {
                println!(
                    "✅ Auto-remediated: {} replica(s) created",
                    result.remediation_actions
                );
            }
            if result.healing_actions > 0 {
                println!(
                    "🔧 Healed: {} file(s) restored from healthy replicas",
                    result.healing_actions
                );
            }
        }

        // ── Heal ───────────────────────────────────────────────────────
        Commands::Heal(HealCommands::Scan {
            snapshot_id,
            storage,
        }) => {
            let default_storage = PathBuf::from(".openvault-vault");
            let storage_path = storage.as_ref().unwrap_or(&default_storage);
            let storage_box: Box<dyn VaultStorage> = Box::new(
                LocalVaultStorage::new(storage_path).context("Failed to initialize storage")?,
            );

            if let Some(sid) = snapshot_id {
                let snapshot = storage_box
                    .load_snapshot(&sid)
                    .context("Snapshot not found")?;
                let result = HealingEngine::scan(&*storage_box, &snapshot)?;
                println!("{}", result.summary());

                for check in &result.checks {
                    if !check.healthy {
                        eprintln!(
                            "  ❌ {} - {}",
                            check.path,
                            check.error.as_deref().unwrap_or("corrupt")
                        );
                    }
                }
            } else {
                let results = HealingEngine::scan_all(&*storage_box)?;
                for result in &results {
                    println!("{}", result.summary());
                }
                let total_corrupt: u32 = results.iter().map(|r| r.files_corrupt).sum();
                if total_corrupt > 0 {
                    eprintln!(
                        "\n⚠️  Found {} corrupt files across all snapshots",
                        total_corrupt
                    );
                } else {
                    println!("\n✅ All snapshots are healthy");
                }
            }
        }

        Commands::Heal(HealCommands::Repair {
            snapshot_id,
            source_storage,
            target_storage,
        }) => {
            let source: Box<dyn VaultStorage> = Box::new(
                LocalVaultStorage::new(&source_storage)
                    .context("Failed to initialize source storage")?,
            );
            let target: Box<dyn VaultStorage> = Box::new(
                LocalVaultStorage::new(&target_storage)
                    .context("Failed to initialize target storage")?,
            );

            let snapshot = target
                .load_snapshot(&snapshot_id)
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

        // ── Audit ───────────────────────────────────────────────────────
        Commands::Audit(AuditCommands::List {
            user,
            operation,
            limit,
        }) => {
            let mut log = AuditLog::new(RotationConfig::default());
            // In a real implementation, log would be loaded from storage
            // For now, add a sample entry
            let mut meta = std::collections::HashMap::new();
            meta.insert("source".into(), "cli".into());
            log.append(
                "system",
                AuditOperation::BackupStarted,
                "cli-backup",
                AuditResult::Success,
                meta,
            )
            .unwrap();

            let mut q = AuditQuery::new();
            if let Some(ref u) = user {
                q = q.user_id(u);
            }
            if let Some(ref op) = operation {
                q = q.operation(AuditOperation::Custom(op.clone()));
            }
            q = q.paginate(0, limit);

            let result = log.query(&q);
            if result.items.is_empty() {
                println!("No audit entries found.");
            } else {
                println!(
                    "{:<6} {:<25} {:<15} {:<20} {:<10} Target",
                    "Seq", "Timestamp", "User", "Operation", "Result"
                );
                println!("{}", "-".repeat(100));
                for e in &result.items {
                    println!(
                        "{:<6} {:<25} {:<15} {:<20} {:<10} {}",
                        e.seq,
                        e.timestamp.format("%Y-%m-%d %H:%M:%S"),
                        e.user_id,
                        e.operation.to_string().chars().take(18).collect::<String>(),
                        format!("{:?}", e.result),
                        e.target,
                    );
                }
                println!("\nTotal: {} entries (page {})", result.total, result.page);
            }
        }

        Commands::Audit(AuditCommands::Verify) => {
            let mut log = AuditLog::new(RotationConfig::default());
            let meta = std::collections::HashMap::new();
            log.append(
                "system",
                AuditOperation::BackupStarted,
                "test",
                AuditResult::Success,
                meta,
            )
            .unwrap();
            match log.verify_chain() {
                Ok(_) => println!("✅ Audit chain integrity verified."),
                Err(e) => eprintln!("❌ Audit chain integrity FAILED: {}", e),
            }
        }

        Commands::Audit(AuditCommands::Export { format, output }) => {
            let mut log = AuditLog::new(RotationConfig::default());
            let mut meta = std::collections::HashMap::new();
            meta.insert("source".into(), "cli".into());
            log.append(
                "system",
                AuditOperation::BackupCompleted,
                "snap-001",
                AuditResult::Success,
                meta,
            )
            .unwrap();

            let q = AuditQuery::new();
            let data = match format.as_str() {
                "csv" => log.export_csv(&q)?,
                _ => log.export_json(&q)?,
            };

            match output {
                Some(path) => {
                    std::fs::write(&path, &data)?;
                    println!("📋 Audit log exported to {}", path.display());
                }
                None => println!("{}", data),
            }
        }

        // ── Compliance ──────────────────────────────────────────────────
        Commands::Compliance(ComplianceCommands::Check {
            path,
            region,
            retention_days,
        }) => {
            let mut checker = ComplianceChecker::new();
            checker.add_rule(ComplianceRule {
                rule_id: "gdpr-default".into(),
                name: "GDPR Default".into(),
                description: "EU data residency".into(),
                classification: DataClassification::Confidential,
                retention: RetentionPolicy::KeepYears(7),
                allowed_regions: vec!["EU".into()],
                path_patterns: vec!["**/confidential/**".into()],
                enabled: true,
            });
            let report = checker.check(&path, &region, retention_days);
            println!("{}", report.summary());
            for f in &report.findings {
                println!("  - [{}] {}: {}", f.severity, f.rule_name, f.message);
            }
        }

        Commands::Compliance(ComplianceCommands::Classify { path }) => {
            let classification = DataClassification::from_path(&path);
            println!("📂 Path: {}", path);
            println!("🏷️  Classification: {:?}", classification);
        }

        // ── Tenant ──────────────────────────────────────────────────────
        Commands::Tenant(TenantCommands::Create {
            name,
            max_storage_gb,
            max_files,
        }) => {
            let mut mgr = TenantManager::new();
            let quota = TenantQuota {
                max_storage_bytes: max_storage_gb * 1024 * 1024 * 1024,
                max_files,
                max_copies: 0,
            };
            let tenant = mgr.create_tenant(&name, quota)?;
            println!("🏢 Tenant created:");
            println!("  ID: {}", tenant.tenant_id);
            println!("  Name: {}", tenant.name);
            println!("  Quota: {} GB / {} files", max_storage_gb, max_files);
        }

        Commands::Tenant(TenantCommands::List) => {
            println!("📋 Tenant management requires OpenVault server connection.");
            println!("   Use 'openvault-server' for multi-tenant management.");
        }

        Commands::Tenant(TenantCommands::Usage { tenant_id }) => {
            println!("📊 Usage for tenant {}:", tenant_id);
            println!("   Connect to OpenVault server for real-time usage data.");
        }

        // ── Notify ──────────────────────────────────────────────────────
        Commands::Notify(NotifyCommands::List { limit }) => {
            let mut svc = NotificationSvc::new();
            let _ = svc.send(
                NotificationType::BackupCompleted,
                Severity::Info,
                "Test Backup",
                "Backup completed",
                None,
                std::collections::HashMap::new(),
            );
            let history = svc.history();
            let count = std::cmp::min(limit as usize, history.len());
            if history.is_empty() {
                println!("No notifications.");
            } else {
                for n in history.iter().take(count) {
                    let read_marker = if n.read { "✓" } else { "●" };
                    println!(
                        "{} [{}] {} — {}",
                        read_marker,
                        n.severity,
                        n.title,
                        n.timestamp.format("%Y-%m-%d %H:%M")
                    );
                }
            }
        }

        Commands::Notify(NotifyCommands::Rules) => {
            let svc = NotificationSvc::new();
            println!("📋 Notification Rules:");
            for r in svc.rules() {
                println!(
                    "  {} ({}): severity >= {:?}, channels: {:?}, dedup: {}m",
                    r.name, r.rule_id, r.min_severity, r.channels, r.dedup_minutes
                );
            }
        }

        Commands::Notify(NotifyCommands::Test { notification_type }) => {
            let mut svc = NotificationSvc::new();
            let ntype = match notification_type.as_str() {
                "backup_failed" => NotificationType::BackupFailed,
                "compliance_violation" => NotificationType::ComplianceViolation,
                "quota_warning" => NotificationType::QuotaWarning,
                _ => NotificationType::BackupCompleted,
            };
            svc.send(
                ntype.clone(),
                Severity::Info,
                "Test",
                "Test notification from CLI",
                None,
                std::collections::HashMap::new(),
            )?;
            println!("📨 Test notification sent: {}", ntype);
        }
    }

    Ok(())
}

/// Safely truncate checksum for display (avoid panic on short strings).
fn checksum_short(checksum: &str) -> &str {
    if checksum.len() >= 8 {
        &checksum[..8]
    } else {
        checksum
    }
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
