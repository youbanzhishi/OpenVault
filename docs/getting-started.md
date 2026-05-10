# Getting Started with OpenVault

> 狡兔三窟，AI守护，永不丢失

OpenVault is a secure, intelligent file backup and disaster recovery system written in Rust. This guide will walk you through installation, configuration, and your first backup/restore cycle.

## Table of Contents

1. [Installation](#installation)
2. [Configuration](#configuration)
3. [Your First Backup Strategy](#your-first-backup-strategy)
4. [Restore Operations](#restore-operations)
5. [CLI Command Reference](#cli-command-reference)
6. [Next Steps](#next-steps)

---

## Installation

### From Binary (Linux x86_64)

Download the latest release from [GitHub Releases](https://github.com/your-org/openvault/releases):

```bash
# Download and extract
curl -L https://github.com/your-org/openvault/releases/latest/download/openvault-server-linux-x86_64.tar.gz | tar xz
sudo mv openvault-server /usr/local/bin/

# Verify
openvault-server --version
```

### Using Docker

```bash
docker pull ghcr.io/your-org/openvault:latest
docker run -d \
  -p 8090:8090 \
  -v openvault-data:/data \
  --name openvault \
  ghcr.io/your-org/openvault:latest
```

### Building from Source

```bash
git clone https://github.com/your-org/openvault.git
cd openvault
cargo build --release -p openvault-server

# The binary is at target/release/openvault-server
```

### Installing the CLI

```bash
cargo build --release -p openvault-cli
# The CLI binary is at target/release/vault
cp target/release/vault /usr/local/bin/
```

---

## Configuration

OpenVault uses a YAML configuration file. Create one with `vault init`:

```bash
vault init
```

This creates a `vault.yaml` in the current directory. Here's a minimal example:

```yaml
name: "my-backup"
source: "/data/important"
strategy: "full"
storage:
  type: "local"
  path: "/backup/vault"
exclude:
  - "*.tmp"
  - ".git"
  - "node_modules"
schedule: "0 2 * * *"  # Daily at 2 AM
```

### Storage Backend Options

| Backend | `type` | Required Fields |
|---------|--------|----------------|
| Local filesystem | `local` | `path` |
| AWS S3 | `s3` | `bucket`, `region`, `access_key_id`, `secret_access_key` |
| Cloudflare R2 | `r2` | `account_id`, `bucket`, `access_key_id`, `secret_access_key` |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `OPENVAULT_BIND` | Server bind address | `0.0.0.0:8090` |
| `OPENVAULT_DB_PATH` | SQLite database path | `/data/db/openvault.db` |
| `OPENVAULT_BACKUP_PATH` | Backup storage path | `/data/backups` |
| `OPENVAULT_JWT_SECRET` | JWT signing secret | (auto-generated) |
| `RUST_LOG` | Log level filter | `openvault_server=info` |

---

## Your First Backup Strategy

### 1. Initialize a Vault

```bash
vault init
```

### 2. Perform a Full Backup

```bash
vault backup /path/to/data --strategy full
```

This creates a complete snapshot of all files under `/path/to/data`, computing SHA-256 checksums for integrity verification.

### 3. Set Up Incremental Backups

Incremental backups only store files that have changed since the last snapshot:

```bash
vault backup /path/to/data --strategy incremental
```

### 4. Verify Integrity

```bash
vault verify
```

This checks that all file checksums match their recorded values.

### 5. Check 3-2-1 Policy Compliance

```bash
vault status --source /path/to/data
```

This evaluates your backup configuration against the 3-2-1 rule:
- **3** copies of your data
- **2** different media types
- **1** offsite copy

---

## Restore Operations

### Restore from a Specific Snapshot

```bash
vault restore snap-20240101120000-0000
```

### Restore to a Custom Directory

```bash
vault restore snap-20240101120000-0000 --target /tmp/restored
```

### Restore with Conflict Handling

```bash
# Skip existing files
vault restore snap-20240101120000-0000 --conflict skip

# Overwrite existing files (default)
vault restore snap-20240101120000-0000 --conflict overwrite

# Rename conflicting files
vault restore snap-20240101120000-0000 --conflict rename
```

### Restore Specific Files

```bash
vault restore snap-20240101120000-0000 --filter "docs/.*" --filter "images/.*"
```

### AI-Powered Restore (Natural Language)

```bash
# Via the API
curl -X POST http://localhost:8090/api/v1/restore/ai \
  -H "Content-Type: application/json" \
  -d '{"query": "restore my tax documents from last year"}'
```

---

## CLI Command Reference

| Command | Description |
|---------|-------------|
| `vault init` | Initialize a new vault |
| `vault backup <path>` | Execute a backup (full/incremental/differential) |
| `vault put <file>` | Store a single file |
| `vault get <path>` | Retrieve a single file |
| `vault list` | List snapshots or files |
| `vault restore <id>` | Restore from a snapshot |
| `vault verify` | Verify integrity of snapshots |
| `vault status` | Show 3-2-1 policy health |
| `vault replicate` | Replicate snapshots to additional backends |
| `vault maintain` | Full 3-2-1 maintenance (check + heal + remediate) |
| `vault heal scan` | Scan for corruption |
| `vault heal repair` | Heal corrupt files from healthy replica |

### Backup Strategy Options

- `--strategy full` — Complete snapshot of all files
- `--strategy incremental` — Only changed files since last snapshot
- `--strategy differential` — All changes since last full backup

### Global Flags

- `--config <path>` — Path to config file (default: `vault.yaml`)
- `--verbose` / `-v` — Enable verbose output
- `--quiet` / `-q` — Suppress non-error output

---

## Next Steps

- **[API Reference](./api-reference.md)** — Full HTTP API documentation
- **[Backup Strategies](./backup-strategies.md)** — Advanced strategy configuration
- **[Deployment Guide](./deployment.md)** — Production deployment with Docker

---

*OpenVault v1.0.0 — Phase 10 Documentation*
