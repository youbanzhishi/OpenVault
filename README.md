# OpenVault

> 狡兔三窟，AI守护，永不丢失

**OpenVault** is a secure, intelligent file backup and disaster recovery system written in Rust. It implements the **3-2-1 backup strategy** (3 copies, 2 media types, 1 offsite) with self-healing capabilities.

[![CI](https://github.com/youbanzhishi/openvault/actions/workflows/ci.yml/badge.svg)](https://github.com/youbanzhishi/openvault/actions/workflows/ci.yml)
[![Release](https://github.com/youbanzhishi/openvault/actions/workflows/release.yml/badge.svg)](https://github.com/youbanzhishi/openvault/actions/workflows/release.yml)
[![Docker](https://img.shields.io/docker/v/ghcr.io/youbanzhishi/openvault?label=docker)](https://github.com/youbanzhishi/openvault/pkgs/container/openvault)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       OpenVault v1.0.1                       │
├──────────┬──────────┬───────────┬──────────┬───────────────┤
│   CLI    │  Server  │  Intel    │ Transport│   Storage     │
│  (vault) │ (axum)   │  (AI)     │ (OpenLink│  Backends     │
│          │          │           │   RPC)   │               │
├──────────┴──────────┴───────────┴──────────┴───────────────┤
│                    openvault-core                           │
│  ┌─────────┐ ┌────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │ Engine  │ │Crypto  │ │ Integrity│ │  3-2-1 Policy    │ │
│  └─────────┘ └────────┘ └──────────┘ └──────────────────┘ │
│  ┌─────────┐ ┌────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │Restore  │ │Search  │ │  Audit   │ │  Self-Healing    │ │
│  └─────────┘ └────────┘ └──────────┘ └──────────────────┘ │
│  ┌─────────┐ ┌────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │Compress │ │Incr.   │ │ Pipeline │ │  Compliance      │ │
│  └─────────┘ └────────┘ └──────────┘ └──────────────────┘ │
│  ┌─────────┐ ┌────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │Tenant   │ │Notif.  │ │  Bench   │ │  Replicator      │ │
│  └─────────┘ └────────┘ └──────────┘ └──────────────────┘ │
└─────────────────────────────────────────────────────────────┘
         │              │              │              │
    ┌────┴────┐   ┌─────┴─────┐  ┌────┴────┐  ┌─────┴─────┐
    │  Local  │   │  AWS S3   │  │   R2    │  │  Custom   │
    │ Storage │   │  Storage  │  │ Storage │  │  Backend  │
    └─────────┘   └───────────┘  └─────────┘  └───────────┘
```

## Crate Layout

| Crate | Description | Lines of Code |
|-------|-------------|---------------|
| `openvault-core` | Core abstractions, backup engine, crypto, search, compliance | ~9,000 |
| `openvault-server` | HTTP API server (axum), device management, web panel | ~4,500 |
| `openvault-intel` | AI intelligence: file classification, anomaly, scheduling | ~1,500 |
| `openvault-storage` | Storage backends: Local, S3, Cloudflare R2 | ~1,500 |
| `openvault-transport` | OpenLink transport for remote management | ~1,500 |
| `openvault-cli` | Command-line interface | ~500 |

## Build Requirements

- **Rust** 1.86+ (因 icu 依赖需要 edition 2024，推荐使用 rustup 安装最新稳定版)
- **C/C++ compiler** (gcc or clang, for SQLite compilation)
- **pkg-config** (optional, for OpenSSL linking)

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Quick Start

### Download Pre-built Binary

```bash
# 下载预编译二进制
curl -L https://github.com/youbanzhishi/OpenVault/releases/latest/download/vault-linux-amd64.tar.gz | tar xz
./openvault-cli serve
```

### Docker Deployment

```bash
# 使用预构建镜像（推荐）
docker run -d -p 8090:8090 ghcr.io/youbanzhishi/openvault/openvault:latest

# 或从 docker-compose 启动
docker compose -f docker/docker-compose.yml up -d
curl http://localhost:8090/api/v1/health
```

### Build from Source

```bash
# Build
cargo build --release -p openvault-server -p openvault-cli

# Start server
./target/release/openvault-server --bind 0.0.0.0:8090

# Initialize a vault
./target/release/vault init

# Full backup
./target/release/vault backup /path/to/data --strategy full

# Verify integrity
./target/release/vault verify

# Check 3-2-1 policy compliance
./target/release/vault status --source /path/to/data
```

📖 For full deployment options, see [部署指南](docs/deployment.md) (Docker, binary, source build, systemd, production config).

### systemd Service Deployment

```ini
# /etc/systemd/system/openvault.service
[Unit]
Description=OpenVault Backup Server
After=network.target

[Service]
Type=simple
User=openvault
ExecStart=/usr/local/bin/openvault-server --bind 0.0.0.0:8090
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable openvault
sudo systemctl start openvault
```

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

### Encryption & Compression
- **AES-256-GCM**: Authenticated encryption for backup data
- **Argon2 / PBKDF2**: Password-based key derivation
- **Hierarchical Key Management**: Master key + per-file data keys
- **Zstd / LZ4 Compression**: Transparent compression pipeline

### Search & Intelligence
- **File Indexing**: Metadata index with keyword search
- **Semantic Search**: AI-powered document search (keyword fallback)
- **Natural Language Restore**: "Restore my tax documents from last year"
- **File Classification**: Automatic file type classification
- **Anomaly Detection**: Unusual backup pattern detection
- **Smart Scheduling**: AI-optimized backup scheduling

### Enterprise Features
- **Multi-Tenant**: Tenant isolation with quotas and RBAC
- **Audit Logging**: Tamper-proof audit trail with rotation
- **Compliance Checking**: Automated compliance reports
- **Notification Service**: Webhook, email, and in-app notifications

### HTTP API Server
- RESTful API for remote backup management
- JWT authentication with scoped access
- Device registration and heartbeat
- Web management dashboard with WebSocket updates
- Physical agent / robot API

## CLI Reference

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

## Performance Benchmarks

OpenVault includes a comprehensive benchmark suite (`openvault-core::bench`):

| Benchmark | Description |
|-----------|-------------|
| `small_file_batch_backup` | 1,000 files (< 1KB each) backup throughput |
| `large_file_backup_checksum` | 100MB file SHA-256 checksum throughput |
| `full_backup_500_files` | Full snapshot creation (500 files) |
| `incremental_backup_50_files` | Incremental snapshot creation (50 changed files) |
| `single_file_restore_latency` | Single file restore setup latency |
| `batch_restore_100_files` | Batch restore setup throughput |
| `cross_device_restore_latency` | Cross-device restore latency estimation |
| `aes256gcm_encrypt/decrypt` | AES-256-GCM encryption/decryption throughput |
| `aes256gcm_encrypt_{4kb,64kb,1mb}` | Block size impact on encryption |
| `index_build_10k_entries` | File index construction speed |
| `keyword_search_10k_index` | Keyword search across 10K entries |
| `semantic_search_1k_index` | Semantic search across 1K entries |

## Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](docs/getting-started.md) | Installation, configuration, first backup |
| [API Reference](docs/api-reference.md) | Complete HTTP API documentation |
| [Backup Strategies](docs/backup-strategies.md) | Strategy guide, templates, encryption |
| [Deployment Guide](docs/deployment.md) | Docker, binary, source build, systemd, production config, troubleshooting |

## Development

### Building

```bash
# Check core crates
cargo check -p openvault-core -p openvault-server -p openvault-intel

# Run tests
cargo test -p openvault-core

# Lint
cargo clippy -p openvault-core -p openvault-server -p openvault-intel
cargo fmt --all -- --check
```

### CI/CD

- **CI** (`.github/workflows/ci.yml`): Runs on every push — check, test, clippy, fmt
- **Release** (`.github/workflows/release.yml`): Triggered by `v*` tags — build binary, Docker image, GitHub Release

## License

MIT
