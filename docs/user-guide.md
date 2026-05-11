# OpenVault 用户使用文档

> 狡兔三窟，AI守护，永不丢失

OpenVault 是安全智能的文件备份与容灾恢复系统，用 Rust 编写，内置 3-2-1 备份策略执行与自愈能力。

---

## 目录

- [安装部署](#安装部署)
- [快速上手](#快速上手)
- [CLI 命令完整参考](#cli-命令完整参考)
- [备份策略详解](#备份策略详解)
- [3-2-1 策略与合规检查](#3-2-1-策略与合规检查)
- [存储后端配置](#存储后端配置)
- [设备管理](#设备管理)
- [自愈机制](#自愈机制)
- [AI 功能](#ai-功能)
- [审计与合规](#审计与合规)
- [通知配置](#通知配置)
- [环境变量参考](#环境变量参考)
- [常见问题 FAQ](#常见问题-faq)

---

## 安装部署

### 方式一：Docker（推荐）

```bash
docker pull ghcr.io/youbanzhishi/openvault/openvault:latest

docker run -d \
  -p 8090:8090 \
  -v openvault-data:/data \
  -e OPENVAULT_JWT_SECRET=$(openssl rand -hex 32) \
  --name openvault \
  ghcr.io/youbanzhishi/openvault/openvault:latest
```

验证服务是否启动：

```bash
curl http://localhost:8090/api/v1/health
# 返回 OK
```

### 方式二：下载预编译二进制

从 [GitHub Releases](https://github.com/youbanzhishi/OpenVault/releases) 下载：

```bash
# Linux x86_64
curl -L https://github.com/youbanzhishi/OpenVault/releases/latest/download/vault-linux-amd64.tar.gz | tar xz
chmod +x vault
sudo mv vault /usr/local/bin/

# macOS (Apple Silicon)
curl -L https://github.com/youbanzhishi/OpenVault/releases/latest/download/vault-macos-arm64.tar.gz | tar xz
chmod +x vault
sudo mv vault /usr/local/bin/
```

创建 systemd 服务（Linux）：

```bash
sudo tee /etc/systemd/system/openvault.service << 'EOF'
[Unit]
Description=OpenVault Backup Server
After=network.target

[Service]
Type=simple
User=openvault
Group=openvault
Environment=OPENVAULT_BIND=0.0.0.0:8090
Environment=OPENVAULT_DB_PATH=/var/lib/openvault/db/openvault.db
Environment=OPENVAULT_BACKUP_PATH=/var/lib/openvault/backups
Environment=OPENVAULT_JWT_SECRET=your-secure-jwt-secret
ExecStart=/usr/local/bin/vault serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now openvault
```

### 方式三：从源码编译

前置条件：Rust 1.86+

```bash
git clone https://github.com/youbanzhishi/OpenVault.git
cd OpenVault

# 编译服务器
cargo build --release -p openvault-server
# 二进制位于 target/release/openvault-server

# 编译 CLI 工具
cargo build --release -p openvault-cli
# 二进制位于 target/release/vault
sudo cp target/release/vault /usr/local/bin/
```

---

## 快速上手

### 第一步：初始化

```bash
vault init
```

这会在当前目录生成 `vault.yaml` 配置文件和 `.openvault-vault` 数据目录。

### 第二步：第一次备份

```bash
vault backup /data/important --strategy full
```

全量备份会为所有文件计算 SHA-256 校验和，创建完整快照。

### 第三步：查看备份状态

```bash
vault status --source /data/important
```

输出包含 3-2-1 策略合规评分和各副本健康状态。

### 第四步：恢复数据

```bash
# 查看可用快照
vault list

# 从快照恢复
vault restore snap-20240101120000-0000 --target /tmp/restored
```

### 第五步：完整性校验

```bash
vault verify
```

---

## CLI 命令完整参考

### 基础命令

| 命令 | 说明 |
|------|------|
| `vault init` | 初始化备份仓库 |
| `vault backup <path> --strategy <策略>` | 执行备份 |
| `vault put <file>` | 存入单个文件 |
| `vault get <path>` | 取出单个文件 |
| `vault list` | 列出快照或文件 |
| `vault restore <id> --target <path>` | 从快照恢复 |
| `vault verify` | 完整性校验 |
| `vault status --source <path>` | 3-2-1 策略合规检查 |
| `vault replicate` | 复制快照到额外后端 |
| `vault maintain` | 完整 3-2-1 维护（check+heal+remediate） |
| `vault heal scan` | 扫描损坏 |
| `vault heal repair` | 修复损坏 |

### backup 命令详解

```bash
vault backup <源路径> [选项]
```

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `-s, --strategy` | 备份策略：`full` / `incremental` / `differential` | `full` |
| `--storage <路径>` | 备份存储路径 | 配置文件中的值 |
| `-c, --config <文件>` | 指定配置文件 | `vault.yaml` |
| `-e, --exclude <模式>` | 排除模式（可重复） | 配置文件中的值 |

### restore 命令详解

```bash
vault restore <快照ID> [选项]
```

| 选项 | 说明 | 默认值 |
|------|------|--------|
| `-t, --target <路径>` | 恢复目标目录 | 必填 |
| `--conflict <策略>` | 冲突处理：`skip` / `overwrite` / `rename` / `fail` | `overwrite` |
| `--verify` | 恢复后校验校验和 | `true` |
| `--storage <路径>` | 备份存储路径 | 配置文件中的值 |

### 子命令

#### snapshots 子命令

```bash
vault snapshots list          # 列出所有快照
vault snapshots info <id>     # 查看快照详情
vault snapshots delete <id>   # 删除快照
```

#### device 子命令

```bash
vault device list             # 列出已注册设备
vault device register --name <名称>  # 注册当前设备
```

#### heal 子命令

```bash
vault heal scan [snapshot_id]               # 扫描损坏
vault heal repair <snapshot_id> \
  --source-storage <健康副本路径> \
  --target-storage <损坏副本路径>           # 修复损坏
```

#### audit 子命令

```bash
vault audit list [--user <ID>] [--operation <类型>] [--limit 20]  # 查看审计日志
vault audit verify                                                  # 验证审计链完整性
vault audit export [--format json|csv] [--output <文件>]           # 导出审计日志
```

#### compliance 子命令

```bash
vault compliance check [--path /] [--region EU] [--retention-days 365]  # 合规检查
vault compliance classify <path>                                         # 数据分类
```

#### tenant 子命令

```bash
vault tenant create --name <名称> [--max-storage-gb 0] [--max-files 0]  # 创建租户
vault tenant list                                                        # 列出租户
vault tenant usage <tenant_id>                                           # 查看租户用量
```

#### notify 子命令

```bash
vault notify list [--limit 20]                               # 列出通知
vault notify rules                                           # 列出通知规则
vault notify test [--notification-type backup_completed]     # 发送测试通知
```

### 全局标志

| 标志 | 说明 |
|------|------|
| `-c, --config <路径>` | 指定配置文件路径（默认 `vault.yaml`） |
| `-v, --verbose` | 详细输出 |
| `-q, --quiet` | 静默模式，仅输出错误 |

---

## 备份策略详解

### 全量备份（Full）

完整复制源目录下所有文件，生成独立快照。

```bash
vault backup /data --strategy full
```

- ✅ 恢复最简单——单个快照包含全部数据
- ✅ 无依赖链，任意快照可独立恢复
- ❌ 存储空间占用最大
- ❌ 大数据集备份耗时较长

**适用场景**：首次备份、小数据集（<10GB）、合规要求的全量快照

### 增量备份（Incremental）

仅备份自上次任意类型快照以来变更的文件。

```bash
vault backup /data --strategy incremental
```

- ✅ 存储占用最小——仅存储变更文件
- ✅ 备份速度最快
- ❌ 恢复需要完整链（全量 + 所有增量）
- ❌ 链中任一快照损坏影响后续增量

**适用场景**：大型数据集的日常备份

### 差异备份（Differential）

备份自上次全量备份以来所有变更的文件。

```bash
vault backup /data --strategy differential
```

- ✅ 恢复只需全量 + 最新差异
- ✅ 比增量更独立
- ❌ 存储占用随时间增长
- ❌ 比增量备份大

**适用场景**：需要比增量更简单恢复链的场景

### 推荐备份计划

| 时间 | 策略 | 说明 |
|------|------|------|
| 每周日 | 全量 | 建立完整基线 |
| 周一至周六 | 增量 | 仅备份变更 |
| 每月1日 | 全量 | 月度长期保留 |

配置示例：

```yaml
name: "production"
source: "/var/data"
strategy: "incremental"
storage:
  type: "local"
  path: "/backup/vault"
schedule: "0 2 * * *"  # 每天凌晨2点
```

---

## 3-2-1 策略与合规检查

### 什么是 3-2-1 规则

| 规则 | 含义 | OpenVault 实现 |
|------|------|----------------|
| **3** 份数据副本 | 原始 + 2 份备份 | 策略引擎监控副本数量 |
| **2** 种存储介质 | 如本地磁盘 + 云端 | 多存储后端支持 |
| **1** 份异地备份 | 地理位置分离 | S3/R2 云端后端 |

### 合规检查

```bash
vault status --source /data/important
```

返回合规评分（0-100%）和详细违规项。

### 策略配置

```yaml
# 严格模式（默认）
policy: strict
# 要求：3+ 副本、2+ 介质类型、1+ 异地

# 宽松模式
policy: relaxed
# 要求：1+ 副本、任意介质、异地可选

# 自定义
policy: custom
min_copies: 2
min_media_types: 2
require_offsite: true
```

### API 合规检查

```bash
# 运行合规检查
curl http://localhost:8090/api/v1/compliance/check?path=/data&region=EU

# 获取合规报告
curl http://localhost:8090/api/v1/compliance/report
```

### 自动修复

当 3-2-1 策略违规时，系统可自动复制到额外后端：

```bash
vault replicate --source /data \
  --replicas /mnt/backup,s3://bucket,r2://bucket \
  --auto-remediate
```

### 完整维护周期

```bash
vault maintain
```

该命令依次执行：合规检查 → 损坏扫描 → 自动修复 → 策略违规自动补救。

---

## 存储后端配置

### Local（本地文件系统）

```yaml
storage:
  type: "local"
  path: "/mnt/backup/vault"
```

- 最快访问速度
- 无网络依赖
- 建议使用独立物理磁盘

### S3（AWS S3 及兼容）

```yaml
storage:
  type: "s3"
  bucket: "my-backups"
  prefix: "vault/"
  region: "us-east-1"
  endpoint: "https://s3.amazonaws.com"  # 可选，MinIO 等兼容服务
  access_key_id: "${AWS_ACCESS_KEY_ID}"
  secret_access_key: "${AWS_SECRET_ACCESS_KEY}"
```

兼容服务：AWS S3、MinIO、DigitalOcean Spaces、Wasabi

### R2（Cloudflare R2）

```yaml
storage:
  type: "r2"
  account_id: "your-account-id"
  bucket: "my-backups"
  prefix: "vault/"
  access_key_id: "${R2_ACCESS_KEY_ID}"
  secret_access_key: "${R2_SECRET_ACCESS_KEY}"
```

零出站流量费，适合大量恢复场景

### 后端对比

| 特性 | Local | S3 | R2 |
|------|-------|----|----|
| 访问速度 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ |
| 成本 | 免费（硬件） | 按 GB + 出站流量 | 按 GB，无出站费 |
| 持久性 | 单磁盘 | 99.999999999% | 99.999999999% |
| 异地 | ❌ | ✅ | ✅ |
| 配置难度 | 低 | 中 | 中 |

### 多后端复制（3-2-1）

```bash
# 复制到多个后端
vault replicate --source /data \
  --replicas /mnt/backup,s3://my-bucket,r2://my-bucket
```

---

## 设备管理

### 注册设备

```bash
# CLI 方式
vault device register --name "office-laptop"

# API 方式
curl -X POST http://localhost:8090/api/v1/devices \
  -H "Content-Type: application/json" \
  -d '{
    "name": "office-laptop",
    "os": "linux",
    "hostname": "thinkpad-x1"
  }'
```

### 设备心跳

设备应定期发送心跳（建议 60 秒间隔）：

```bash
curl -X POST http://localhost:8090/api/v1/devices/dev-abc123/heartbeat
```

### 查看设备状态

```bash
# 列出所有设备
vault device list

# API 方式
curl http://localhost:8090/api/v1/devices

# 查看特定设备
curl http://localhost:8090/api/v1/devices/dev-abc123
```

### 跨设备恢复

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

## 自愈机制

OpenVault 的自愈引擎可自动检测并修复损坏的备份文件。

### 扫描损坏

```bash
vault heal scan
# 或扫描特定快照
vault heal scan snap-20240101120000-0000
```

扫描过程会逐文件校验 SHA-256，输出损坏文件列表。

### 修复损坏

从健康副本修复损坏数据：

```bash
vault heal repair snap-20240101120000-0000 \
  --source-storage /healthy-vault \
  --target-storage /corrupt-vault
```

### 自动维护

```bash
vault maintain
```

自动执行完整维护周期：

1. **Check**：3-2-1 策略合规检查
2. **Heal**：扫描并修复损坏文件
3. **Remediate**：自动补足缺失副本

---

## AI 功能

### AI 自然语言恢复

用自然语言描述需要恢复的文件，AI 自动定位并恢复：

```bash
curl -X POST http://localhost:8090/api/v1/restore/ai \
  -H "Content-Type: application/json" \
  -d '{"query": "恢复去年的税务文件"}'
```

支持的自然语言描述示例：
- "恢复上周修改的文档"
- "找回12月份的报表"
- "恢复所有 .pdf 文件"

AI 会解析时间范围、文件类型、操作类型等语义，返回匹配文件列表。

### 智能建议

获取 AI 对备份策略的优化建议：

```bash
curl http://localhost:8090/api/v1/intel/suggestions
```

返回内容包含：
- **文件分类建议**：按文件类型推荐备份频率和优先级
- **调度建议**：最佳备份时间窗口
- **风险评估**：当前备份方案的风险等级和改进建议

### 文件搜索

跨快照搜索备份文件：

```bash
curl -X POST http://localhost:8090/api/v1/search \
  -H "Content-Type: application/json" \
  -d '{"query": "季度报告", "mode": "keyword", "limit": 20}'
```

搜索模式：
- `keyword`：关键词匹配
- `semantic`：语义搜索

---

## 审计与合规

### 查看审计日志

```bash
# CLI
vault audit list --limit 50
vault audit list --user admin --operation backup

# API
curl "http://localhost:8090/api/v1/audit?limit=50"
```

### 验证审计链完整性

```bash
vault audit verify
```

确认审计日志未被篡改。

### 导出审计日志

```bash
vault audit export --format json --output audit-2024.json
vault audit export --format csv --output audit-2024.csv
```

### 合规检查

```bash
# CLI
vault compliance check --path /data --region EU --retention-days 365

# API
curl "http://localhost:8090/api/v1/compliance/check?path=/data&region=EU"
curl http://localhost:8090/api/v1/compliance/report
```

### 数据分类

```bash
vault compliance classify /data/important
```

按敏感度对数据进行分类标记。

---

## 通知配置

### 查看通知配置

```bash
curl http://localhost:8090/api/v1/notifications/config
```

### 更新通知配置

```bash
curl -X PUT http://localhost:8090/api/v1/notifications/config \
  -H "Content-Type: application/json" \
  -d '{
    "smtp_host": "smtp.example.com",
    "smtp_port": 587,
    "from_address": "openvault@example.com"
  }'
```

### 创建通知规则

```bash
curl -X POST http://localhost:8090/api/v1/notifications/rules \
  -H "Content-Type: application/json" \
  -d '{
    "name": "备份失败告警",
    "notification_types": ["backup_failed"],
    "min_severity": "error",
    "channels": ["webhook", "email"],
    "webhook_url": "https://hooks.example.com/backup-alerts",
    "dedup_minutes": 5
  }'
```

### 支持的通知类型

| 类型 | 说明 |
|------|------|
| `backup_completed` | 备份完成 |
| `backup_failed` | 备份失败 |
| `compliance_violation` | 合规违规 |
| `quota_warning` | 配额警告 |
| `risk_warning` | 风险警告 |

### 支持的通道

| 通道 | 说明 |
|------|------|
| `webhook` | HTTP Webhook |
| `email` | 邮件通知 |
| `in_app` | 应用内通知 |

### 严重级别

`info` → `warning` → `error` → `critical`

---

## 环境变量参考

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `OPENVAULT_BIND` | 服务器绑定地址 | `0.0.0.0:8090` |
| `OPENVAULT_DB_PATH` | 数据库路径 | `/data/db/openvault.db` |
| `OPENVAULT_BACKUP_PATH` | 备份存储路径 | `/data/backups` |
| `OPENVAULT_JWT_SECRET` | JWT 签名密钥（**生产环境必填**） | 自动生成 |
| `RUST_LOG` | 日志级别 | `openvault_server=info` |

### 日志级别配置

```bash
# 生产环境
RUST_LOG=openvault_server=info

# 调试模式
RUST_LOG=openvault_server=debug

# 详细（含依赖库）
RUST_LOG=debug
```

---

## 常见问题 FAQ

### Q: 服务器启动后无法访问？

1. 检查端口是否被占用：`ss -tlnp | grep 8090`
2. 检查防火墙设置
3. 查看日志：`docker logs openvault` 或 `journalctl -u openvault`

### Q: 备份失败怎么办？

1. 查看备份状态：`curl http://localhost:8090/api/v1/backup/<backup_id>`
2. 常见原因：
   - 源目录不可读
   - 存储后端不可达
   - 磁盘空间不足
   - 加密密钥不匹配

### Q: 增量备份链断裂如何恢复？

运行自愈修复：

```bash
vault heal scan           # 先扫描确认损坏位置
vault heal repair <id> \
  --source-storage /healthy \
  --target-storage /corrupt
```

或直接创建新的全量备份重建基线：

```bash
vault backup /data --strategy full
```

### Q: 如何满足 3-2-1 合规？

1. 至少配置 2 个存储后端（如 Local + S3）
2. 确保至少 1 个是异地（S3 或 R2）
3. 使用 `vault replicate` 或 `vault maintain` 自动维护

### Q: AI 自然语言恢复支持哪些描述？

支持以下语义元素：
- **时间范围**：上周、去年、12月份、2024年1月
- **文件类型**：文档、图片、PDF、代码
- **操作类型**：新建、修改、删除
- **路径模式**：包含特定目录或文件名

### Q: 如何在生产环境保障安全？

1. 设置强 JWT 密钥：`OPENVAULT_JWT_SECRET=$(openssl rand -hex 32)`
2. 启用 TLS（通过 Nginx 反向代理）
3. 启用加密：配置 `encryption` 字段
4. 网络隔离：将备份服务放在内网

### Q: 如何监控备份健康状况？

```bash
# 系统状态
curl http://localhost:8090/api/v1/status

# 合规检查
vault status --source /data

# 查看审计日志
vault audit list --limit 20
```

### Q: SQLite 和 PostgreSQL 如何选择？

| 场景 | 推荐 |
|------|------|
| 单机部署 / 开发 | SQLite |
| 多副本 / 生产 | PostgreSQL |

---

*OpenVault v1.0.1 — 用户文档*
