# Deployment Guide

Production deployment guide for OpenVault, covering Docker deployment, configuration, monitoring, troubleshooting, and multi-device management.

## Table of Contents

1. [Docker Deployment](#docker-deployment)
2. [Configuration](#configuration)
3. [Monitoring](#monitoring)
4. [Troubleshooting](#troubleshooting)
5. [Multi-Device Management](#multi-device-management)
6. [Security Hardening](#security-hardening)

---

## Docker Deployment

### Quick Start (Single Node)

```bash
# Clone the repository
git clone https://github.com/your-org/openvault.git
cd openvault

# Start the server
docker compose -f docker/docker-compose.yml up -d

# Verify
curl http://localhost:8090/api/v1/health
```

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OPENVAULT_JWT_SECRET` | ⚠️ Yes (prod) | (auto-generated) | JWT signing secret |
| `OPENVAULT_BIND` | No | `0.0.0.0:8090` | Bind address |
| `OPENVAULT_DB_PATH` | No | `/data/db/openvault.db` | SQLite database path |
| `OPENVAULT_BACKUP_PATH` | No | `/data/backups` | Backup storage path |
| `RUST_LOG` | No | `openvault_server=info` | Log level filter |

### Data Persistence

The Docker Compose configuration uses named volumes:

- `openvault-db` — SQLite database
- `openvault-backups` — Backup data
- `openvault-logs` — Application logs

To back up the volumes:

```bash
docker run --rm -v openvault-db:/data -v $(pwd):/backup alpine \
  tar czf /backup/openvault-db-backup.tar.gz -C /data .
```

### Health Check

The container includes a built-in health check:

```bash
curl -f http://localhost:8090/api/v1/health || exit 1
```

Docker will report the container as unhealthy after 3 consecutive failures (30-second intervals).

---

## Production Deployment

### Multi-Replica with Nginx

```bash
# Start production stack
docker compose -f docker/docker-compose.prod.yml up -d
```

The production configuration includes:

- **2 OpenVault server replicas** behind Nginx load balancer
- **PostgreSQL** database (replacing SQLite for multi-replica support)
- **Prometheus** metrics collection
- **Grafana** dashboards

### Nginx Configuration

Create `docker/nginx.conf`:

```nginx
upstream openvault {
    server openvault-server:8090;
}

server {
    listen 80;
    server_name openvault.example.com;

    location / {
        proxy_pass http://openvault;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # WebSocket support for real-time updates
    location /ws/ {
        proxy_pass http://openvault;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

### Resource Limits

The production Compose file sets resource limits per container:

| Service | CPU Limit | Memory Limit | CPU Reserved | Memory Reserved |
|---------|-----------|--------------|--------------|-----------------|
| openvault-server | 2.0 | 1 GB | 0.5 | 256 MB |
| postgres | 2.0 | 2 GB | 0.5 | 512 MB |
| nginx | 0.5 | 128 MB | 0.1 | 32 MB |
| prometheus | 1.0 | 512 MB | 0.25 | 128 MB |
| grafana | 0.5 | 256 MB | 0.1 | 64 MB |

### Scaling Replicas

```bash
# Scale to 4 replicas
docker compose -f docker/docker-compose.prod.yml up -d --scale openvault-server=4
```

---

## Configuration

### SQLite vs PostgreSQL

| Feature | SQLite | PostgreSQL |
|---------|--------|------------|
| **Setup** | Zero config | External service |
| **Scalability** | Single writer | Multi-writer |
| **Replicas** | Not supported | Required for multi-replica |
| **Backup** | File copy | `pg_dump` |
| **Best For** | Single-node, dev | Production, multi-replica |

### Logging

OpenVault uses the `tracing` crate with `RUST_LOG` for log level control:

```bash
# Production
RUST_LOG=openvault_server=info

# Debugging
RUST_LOG=openvault_server=debug

# Verbose (includes dependencies)
RUST_LOG=debug
```

Log output is structured JSON when `RUST_LOG_FORMAT=json` is set.

---

## Monitoring

### Prometheus Integration

Create `docker/prometheus.yml`:

```yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'openvault'
    static_configs:
      - targets: ['openvault-server:8090']
    metrics_path: /metrics
```

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `openvault_backup_duration_seconds` | Backup operation duration | > 3600s |
| `openvault_backup_failures_total` | Failed backup count | > 0 |
| `openvault_restore_duration_seconds` | Restore operation duration | > 1800s |
| `openvault_storage_bytes_total` | Total storage used | > 80% quota |
| `openvault_devices_offline` | Number of offline devices | > 50% |
| `openvault_compliance_score` | 3-2-1 compliance score | < 80% |

### Grafana Dashboard

Import the OpenVault Grafana dashboard (included in the Docker setup at `http://localhost:3000`).

Default credentials: `admin` / `admin` (change on first login).

---

## Troubleshooting

### Common Issues

#### Server Won't Start

**Symptom**: Container exits immediately.

```bash
# Check logs
docker logs openvault-server

# Common causes:
# 1. Port already in use
# 2. Invalid JWT secret
# 3. Database permission issues
```

**Solution**:

```bash
# Check port availability
ss -tlnp | grep 8090

# Fix permissions
docker exec openvault-server chown -R openvault:openvault /data
```

#### Backup Fails

**Symptom**: Backup operation shows `failed` status.

```bash
# Check backup status
curl http://localhost:8090/api/v1/backup/<backup_id>

# Common causes:
# 1. Source directory not readable
# 2. Storage backend unreachable
# 3. Disk space exhausted
# 4. Encryption key mismatch
```

#### High Memory Usage

**Symptom**: Server using more memory than expected.

```bash
# Check current memory
docker stats openvault-server

# Solutions:
# 1. Reduce CARGO_BUILD_JOBS
# 2. Add resource limits in docker-compose
# 3. Check for memory leaks in long-running operations
```

#### Database Locked (SQLite)

**Symptom**: `database is locked` errors.

**Solution**: Switch to PostgreSQL for multi-replica deployments, or ensure only one writer at a time.

### Diagnostic Commands

```bash
# Server health
curl http://localhost:8090/api/v1/health

# System status
curl http://localhost:8090/api/v1/status

# Audit log
curl http://localhost:8090/api/v1/audit?limit=50

# Compliance report
curl http://localhost:8090/api/v1/compliance/report
```

---

## Multi-Device Management

### Registering Devices

```bash
# Register a new device
curl -X POST http://localhost:8090/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "office-laptop",
    "os": "linux",
    "hostname": "thinkpad-x1"
  }'
```

### Device Heartbeat

Devices should send periodic heartbeats (recommended: every 60 seconds):

```bash
curl -X POST http://localhost:8090/api/v1/devices/dev-abc123/heartbeat
```

### Monitoring Device Health

```bash
# List all devices and their status
curl http://localhost:8090/api/v1/devices

# Check specific device
curl http://localhost:8090/api/v1/devices/dev-abc123

# Device backups
curl http://localhost:8090/api/v1/devices/dev-abc123/backups
```

### Cross-Device Restore

To restore files from one device's backup to another:

```bash
curl -X POST http://localhost:8090/api/v1/restore \
  -H "Content-Type: application/json" \
  -d '{
    "snapshot_id": "snap-20240101120000-0000",
    "target": "/tmp/restored",
    "source_device": "dev-abc123"
  }'
```

---

## Security Hardening

### JWT Secret

**Always** set a strong JWT secret in production:

```bash
# Generate a secure secret
openssl rand -hex 32

# Set via environment
export OPENVAULT_JWT_SECRET=<generated-secret>
```

### TLS/SSL

Use Nginx as a TLS termination proxy:

```nginx
server {
    listen 443 ssl http2;
    ssl_certificate /etc/nginx/ssl/cert.pem;
    ssl_certificate_key /etc/nginx/ssl/key.pem;
    # ... proxy config
}
```

### Encryption at Rest

Enable backup encryption:

```yaml
encryption:
  algorithm: "aes-256-gcm"
  key_derivation: "argon2"
  password: "${VAULT_ENCRYPTION_PASSWORD}"
```

### Network Isolation

```yaml
networks:
  openvault-net:
    driver: bridge
    internal: false  # Set to true for backend-only
```

---

*OpenVault v1.0.0 — Phase 10 Documentation*
---

## 非 Docker 部署（二进制直接部署）

如果你不想使用 Docker，可以直接下载预编译二进制或从源码编译。

### 方式一：下载预编译二进制

从 [GitHub Releases](https://github.com/youbanzhishi/OpenVault/releases) 下载对应平台的二进制：

```bash
# Linux x86_64
curl -L https://github.com/youbanzhishi/OpenVault/releases/latest/download/vault-linux-amd64.tar.gz | tar xz
chmod +x openvault-cli
sudo mv openvault-cli /usr/local/bin/openvault

# macOS (Apple Silicon)
curl -L https://github.com/youbanzhishi/OpenVault/releases/latest/download/vault-macos-arm64.tar.gz | tar xz
chmod +x openvault-cli
sudo mv openvault-cli /usr/local/bin/openvault

# Windows
# 下载 vault-windows-amd64.exe.zip，解压后使用
```

#### 创建 systemd 服务（Linux）

```bash
# 创建配置目录
sudo mkdir -p /etc/openvault /var/lib/openvault /var/lib/openvault/backups

# 创建 systemd 服务
sudo tee /etc/systemd/system/openvault.service << 'EOF'
[Unit]
Description=OpenVault Backup Server
After=network.target

[Service]
Type=simple
User=openvault
Group=openvault
WorkingDirectory=/var/lib/openvault
Environment=RUST_LOG=openvault_server=info
Environment=OPENVAULT_BIND=0.0.0.0:8090
Environment=OPENVAULT_DB_PATH=/var/lib/openvault/db/openvault.db
Environment=OPENVAULT_BACKUP_PATH=/var/lib/openvault/backups
Environment=OPENVAULT_JWT_SECRET=your-secure-jwt-secret
ExecStart=/usr/local/bin/openvault serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# 创建用户和数据目录
sudo useradd -r -s /bin/false openvault
sudo mkdir -p /var/lib/openvault/db /var/lib/openvault/backups
sudo chown -R openvault:openvault /var/lib/openvault

# 启动服务
sudo systemctl daemon-reload
sudo systemctl enable openvault
sudo systemctl start openvault
sudo systemctl status openvault
```

#### 生成 JWT 密钥

```bash
# 生产环境务必设置强密钥
export OPENVAULT_JWT_SECRET=$(openssl rand -hex 32)
echo "OPENVAULT_JWT_SECRET=$OPENVAULT_JWT_SECRET" | sudo tee -a /etc/openvault/env
```

#### 连接 PostgreSQL（生产环境）

```bash
# 安装 PostgreSQL
sudo apt-get install postgresql postgresql-contrib

# 创建数据库和用户
sudo -u postgres createuser openvault
sudo -u postgres createdb openvault -O openvault
sudo -u postgres psql -c "ALTER USER openvault PASSWORD 'your_secure_password';"

# 设置环境变量
export OPENVAULT_DB_PATH="postgres://openvault:your_secure_password@localhost:5432/openvault"
```

### 方式二：从源码编译

```bash
# 安装 Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 克隆仓库
git clone https://github.com/youbanzhishi/OpenVault.git
cd OpenVault

# 编译 release 版本
cargo build --release -p openvault-cli

# 二进制位于
./target/release/openvault-cli

# 安装到系统路径
sudo cp target/release/openvault-cli /usr/local/bin/openvault
```

#### 编译依赖（Linux）

```bash
sudo apt-get install build-essential pkg-config libssl-dev
```

#### 编译依赖（macOS）

```bash
xcode-select --install
```

### 常用命令

```bash
# 启动服务
openvault serve

# 检查健康状态
openvault status

# 列出所有设备
openvault devices list

# 手动触发备份
openvault backup run --device <device-id>

# 恢复数据
openvault restore --snapshot <snapshot-id> --target /path/to/restore

# 查看合规报告
openvault compliance report
```

### Nginx 反向代理（推荐生产环境）

```nginx
server {
    listen 80;
    server_name openvault.example.com;

    location / {
        proxy_pass http://127.0.0.1:8090;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # WebSocket 支持（实时更新）
    location /ws/ {
        proxy_pass http://127.0.0.1:8090;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```
