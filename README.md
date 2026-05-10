# OpenVault

> 狡兔三窟，AI守护，永不丢失

**OpenVault** is a secure, intelligent file backup and disaster recovery system written in Rust. It implements the **3-2-1 backup strategy** (3 copies, 2 media types, 1 offsite) with self-healing capabilities.

## Features

### Core Backup Engine
- **Full Backup**: Complete snapshot of all files
- **Incremental Backup**: Only changed files since last snapshot (mtime + size detection)
- **Differential Backup**: All changes since last full backup
- **SHA-256 Integrity**: Every file checksummed and verified

### Storage Backends
- **Local Filesystem**: Fast local storage with directory-based layout
- **S3-Compatible**: AWS S3, MinIO, and any S3-compatible service (SigV4 signing)
- **Cloudflare R2**: S3-compatible with automatic endpoint configuration

### 3-2-1 Backup Strategy
- **Policy Engine**: Evaluate backup compliance against 3-2-1 rules
- **Auto-Remediation**: Automatically replicate to additional backends when policy is violated
- **Replication Coordinator**: Manage snapshot distribution across multiple backends
- **Policy Profiles**: Strict (3-2-1), Relaxed (1-1-0), and Custom

### Self-Healing
- **Corruption Detection**: Scan snapshots for checksum mismatches
- **Automatic Recovery**: Heal corrupt files from healthy replicas on other backends
- **Multi-Source Healing**: Try multiple source backends in priority order
- **Post-Heal Verification**: Verify data integrity after healing

### Encryption
- **AES-256-GCM**: Authenticated encryption for backup data
- **Argon2 Key Derivation**: Password-based key derivation
- **Per-File Nonces**: Random 12-byte nonce for each encryption operation

### CLI
```
vault init           Initialize a new vault
vault backup <path>  Execute a backup (full/incremental/differential)
vault put <file>     Store a single file
vault get <path>     Retrieve a single file
vault list           List snapshots or files
vault restore <id>   Restore from a snapshot
vault verify         Verify integrity of snapshots
vault status         Show 3-2-1 policy health
vault replicate      Replicate snapshots to additional backends
vault maintain       Full 3-2-1 maintenance (check + heal + remediate)
vault heal scan      Scan for corruption
vault heal repair    Heal corrupt files from healthy replica
```

### HTTP API Server
- RESTful API for remote backup management
- JWT authentication with scoped access
- Device registration and heartbeat
- Policy management
- Webhook notifications

## Architecture

```
OpenVault
├── openvault-core       # Core abstractions, types, engine
├── openvault-storage    # Storage backends (Local, S3, R2)
├── openvault-cli        # Command-line interface
├── openvault-transport  # OpenLink transport for remote management
└── openvault-server     # HTTP API server
```

## Quick Start

```bash
# Initialize a local vault
vault init

# Full backup
vault backup /path/to/data --strategy full

# Incremental backup
vault backup /path/to/data --strategy incremental

# Verify integrity
vault verify

# Check 3-2-1 policy compliance
vault status --source /path/to/data

# Replicate to additional storage
vault replicate --source /path/to/data --replicas /backup1,/backup2

# Self-heal corrupt files
vault heal repair snap-001 --source-storage /healthy-vault --target-storage /corrupt-vault
```

## Configuration (YAML)

```yaml
name: "my-backup"
source: "/data/important"
strategy: "full"
storage:
  type: "s3"
  bucket: "my-backups"
  prefix: "vault/"
  endpoint: "https://s3.amazonaws.com"
  region: "us-east-1"
  access_key_id: "AKIAIOSFODNN7EXAMPLE"
  secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
exclude:
  - "*.tmp"
  - ".git"
  - "node_modules"
schedule: "0 2 * * *"  # Daily at 2 AM
```

## License

MIT
