# Backup Strategies Guide

This guide covers backup strategy best practices, templates, storage backend selection, and encryption configuration for OpenVault.

## Table of Contents

1. [The 3-2-1 Rule](#the-3-2-1-rule)
2. [Strategy Types](#strategy-types)
3. [Strategy Templates](#strategy-templates)
4. [Storage Backend Selection](#storage-backend-selection)
5. [Encryption Configuration](#encryption-configuration)
6. [Advanced Patterns](#advanced-patterns)

---

## The 3-2-1 Rule

The 3-2-1 backup rule is the gold standard for data protection:

| Rule | Meaning | OpenVault Implementation |
|------|---------|------------------------|
| **3** copies of data | Original + 2 backups | Policy engine monitors copy count |
| **2** different media types | e.g., local disk + cloud | Multiple storage backends |
| **1** offsite copy | Geographically separated | S3/R2 cloud backends |

### How OpenVault Enforces 3-2-1

OpenVault's policy engine continuously evaluates your backup configuration:

- **Compliance Check**: `vault status` evaluates copy count, media diversity, and offsite presence
- **Auto-Remediation**: When policy is violated, the system automatically replicates to additional backends
- **Health Scoring**: Each backup source gets a compliance score (0-100%)

### Policy Profiles

```yaml
# Strict 3-2-1 (default)
policy: strict
# Requires: 3+ copies, 2+ media types, 1+ offsite

# Relaxed (1-1-0)
policy: relaxed
# Requires: 1+ copy, any media, offsite optional

# Custom
policy: custom
min_copies: 2
min_media_types: 2
require_offsite: true
```

---

## Strategy Types

### Full Backup

A complete snapshot of all files in the source directory.

**Pros**:
- Simple to restore (single snapshot contains everything)
- No dependency chain
- Fastest single-file restore

**Cons**:
- Highest storage consumption
- Slowest to create for large datasets
- Most network bandwidth for remote backends

**Best for**: Initial backups, small datasets (<10 GB), compliance-required full snapshots

```bash
vault backup /data --strategy full
```

### Incremental Backup

Only files that have changed since the **last** snapshot (of any type).

**Pros**:
- Minimal storage — only changed files
- Fastest backup time
- Lowest bandwidth usage

**Cons**:
- Restore requires full chain (full + all incrementals)
- Chain dependency — if one snapshot is corrupt, later incrementals may be affected
- Longer restore times

**Best for**: Daily backups of large, slowly-changing datasets

```bash
vault backup /data --strategy incremental
```

### Differential Backup

All files that have changed since the **last full** backup.

**Pros**:
- Compromise between full and incremental
- Only need last full + latest differential to restore
- More independent than incremental chain

**Cons**:
- Storage grows over time (each differential includes all changes since full)
- Larger than incremental backups

**Best for**: Weekly backups where you want simpler restore chains than incremental

```bash
vault backup /data --strategy differential
```

### Recommended Schedule

| Day | Strategy | Why |
|-----|----------|-----|
| Sunday | Full | Start the week with a complete baseline |
| Mon-Sat | Incremental | Fast daily backups of only changes |
| Monthly | Full | Monthly baseline for long-term retention |

---

## Strategy Templates

### Personal Documents

```yaml
name: "personal-docs"
source: "/home/user/Documents"
strategy: "incremental"
storage:
  - type: "local"
    path: "/mnt/backup/docs"
  - type: "s3"
    bucket: "my-backups"
    prefix: "personal/docs/"
    region: "us-east-1"
schedule: "0 2 * * *"  # Daily at 2 AM
retention_days: 90
exclude:
  - "*.tmp"
  - "~*"
  - ".cache/"
```

### Enterprise Server

```yaml
name: "production-server"
source: "/var/data"
strategy: "incremental"
storage:
  - type: "local"
    path: "/mnt/nas/production"
  - type: "s3"
    bucket: "company-backups"
    prefix: "production/"
    region: "us-east-1"
  - type: "r2"
    account_id: "abc123"
    bucket: "disaster-recovery"
    prefix: "production/"
schedule: "0 */4 * * *"  # Every 4 hours
retention_days: 365
encryption:
  algorithm: "aes-256-gcm"
  key_derivation: "argon2"
exclude:
  - "*.log"
  - "/tmp/"
  - "*.cache"
```

### Development Project

```yaml
name: "dev-project"
source: "/home/dev/project"
strategy: "full"
storage:
  - type: "local"
    path: "/backup/project"
  - type: "r2"
    account_id: "dev-account"
    bucket: "dev-backups"
    prefix: "project/"
schedule: "0 0 * * 1"  # Weekly on Monday
retention_days: 30
exclude:
  - "target/"
  - "node_modules/"
  - ".git/"
  - "*.pyc"
```

---

## Storage Backend Selection

### Comparison Matrix

| Feature | Local | S3 | R2 |
|---------|-------|----|----|
| **Speed** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ |
| **Cost** | Free (hardware) | Pay per GB + egress | Pay per GB, no egress |
| **Durability** | Single disk | 99.999999999% | 99.999999999% |
| **Offsite** | ❌ | ✅ | ✅ |
| **Media Type** | Local disk | Cloud object | Cloud object |
| **Setup Complexity** | Low | Medium | Medium |
| **Best For** | Fast local copies | AWS ecosystem | Cost-effective cloud |

### Local Backend

```yaml
storage:
  type: "local"
  path: "/mnt/backup/vault"
```

**When to use**: First backup copy, fast access, no network dependency.

**Tips**:
- Use a separate physical disk from the source
- Consider ZFS or Btrfs for additional integrity
- Mount with `noatime` for better performance

### S3 Backend

```yaml
storage:
  type: "s3"
  bucket: "my-backups"
  prefix: "vault/"
  region: "us-east-1"
  endpoint: "https://s3.amazonaws.com"  # Optional for non-AWS
  access_key_id: "${AWS_ACCESS_KEY_ID}"
  secret_access_key: "${AWS_SECRET_ACCESS_KEY}"
```

**When to use**: Offsite backup, AWS ecosystem, compliance requirements.

**Compatible services**: AWS S3, MinIO, DigitalOcean Spaces, Wasabi

### R2 Backend

```yaml
storage:
  type: "r2"
  account_id: "your-account-id"
  bucket: "my-backups"
  prefix: "vault/"
  access_key_id: "${R2_ACCESS_KEY_ID}"
  secret_access_key: "${R2_SECRET_ACCESS_KEY}"
```

**When to use**: Cloudflare ecosystem, zero egress fees, cost optimization.

---

## Encryption Configuration

OpenVault uses AES-256-GCM authenticated encryption for backup data at rest.

### Key Derivation Methods

| Method | Use Case | Security |
|--------|----------|----------|
| **Argon2** | Password-based keys | High (memory-hard) |
| **PBKDF2** | Legacy compatibility | Medium (CPU-hard only) |

### Enabling Encryption

```yaml
encryption:
  algorithm: "aes-256-gcm"
  key_derivation: "argon2"
  password: "${VAULT_ENCRYPTION_PASSWORD}"  # From environment variable
```

### Key Management

OpenVault supports hierarchical key management:

- **Master Key**: Derived from password, never stored
- **Data Keys**: Per-file encryption keys, encrypted with master key
- **Key Rotation**: Re-encrypt data keys without re-encrypting all data

### Pipeline Configuration

The storage pipeline applies operations in order:

```
Source File → Compress → Encrypt → Store
```

```yaml
pipeline:
  - compress:
      algorithm: "zstd"
      level: 3
  - encrypt:
      algorithm: "aes-256-gcm"
      key_derivation: "argon2"
```

### Compression Options

| Algorithm | Ratio | Speed | Best For |
|-----------|-------|-------|----------|
| **zstd** (level 3) | Good | Fast | General purpose |
| **lz4** | Moderate | Very fast | Low-latency backups |

---

## Advanced Patterns

### Multi-Backend Replication

```bash
# Replicate to additional backends for 3-2-1 compliance
vault replicate --source /data --replicas /mnt/backup,s3://bucket,r2://bucket
```

### Self-Healing

```bash
# Scan for corruption
vault heal scan

# Repair from healthy replicas
vault heal repair snap-20240101120000-0000 \
  --source-storage /healthy-vault \
  --target-storage /corrupt-vault
```

### Full Maintenance Cycle

```bash
# Check compliance, heal corruption, remediate policy violations
vault maintain
```

---

*OpenVault v1.0.0 — Phase 10 Documentation*
