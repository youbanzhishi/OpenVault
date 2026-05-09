# OpenVault 项目索引

## 项目概述
OpenVault 是智能文件备份容灾系统，支持增量备份、差异备份、加密存储、多后端存储。

## 项目结构

```
OpenVault/
├── Cargo.toml           # 工作区配置
├── crates/
│   ├── openvault-core/  # 核心库
│   │   └── src/
│   │       ├── lib.rs   # 导出公共接口
│   │       ├── config.rs
│   │       ├── crypto.rs
│   │       ├── engine.rs
│   │       ├── error.rs
│   │       ├── integrity.rs
│   │       ├── restore.rs
│   │       ├── snapshot.rs
│   │       ├── storage.rs
│   │       └── strategy.rs
│   ├── openvault-storage/  # 存储后端
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── local.rs   # 本地存储实现
│   │       └── s3.rs      # S3存储实现(预留)
│   ├── openvault-cli/   # 命令行工具
│   │   └── src/main.rs
│   ├── openvault-transport/  # Phase 4: OpenLink传输
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs
│   │       ├── error.rs
│   │       ├── router.rs
│   │       └── transport.rs
│   └── openvault-server/  # Phase 4: API服务器
│       └── src/
│           ├── main.rs
│           ├── lib.rs
│           ├── api.rs
│           ├── auth.rs
│           ├── error.rs
│           ├── handlers.rs
│           ├── models.rs
│           └── services.rs
└── 项目文档/
    └── OpenVault/
        ├── INDEX.md
        └── 规划/
            └── hot-rules.md
```

## 开发阶段

### Phase 1: 核心备份引擎
- 备份引擎 trait 和策略模式
- 完整备份实现
- 文件扫描和元数据管理
- 状态: ✅ 完成 (16测试)

### Phase 2: 增量+差异+多后端
- 增量备份策略
- 差异备份策略
- S3存储后端
- 状态: ✅ 完成 (31测试)

### Phase 3: 加密+校验+恢复
- AES-GCM 加密
- SHA-256 校验
- 恢复引擎
- 冲突解决策略
- 状态: ✅ 完成 (54测试)

### Phase 4: OpenLink集成 + 远程管理
- **openvault-transport**: OpenLink API集成
  - Transport trait 抽象
  - OpenLinkTransport 实现
  - StorageRouter 存储路由
  - TransferRouter 传输路由
- **openvault-server**: Axum HTTP API服务
  - 设备管理 (注册/心跳/状态)
  - 策略管理 (创建/更新/删除)
  - 备份操作 (触发/状态/取消)
  - 恢复操作
  - 快照管理
  - 通知系统 (Webhook)
  - JWT认证
- **多设备备份管理**
- 状态: ✅ 完成 (46测试)

## API 端点

### 健康与状态
- `GET /api/v1/health` - 健康检查
- `GET /api/v1/status` - 系统状态

### 设备管理
- `POST /api/v1/devices` - 注册设备
- `GET /api/v1/devices` - 列出设备
- `GET /api/v1/devices/:id` - 获取设备详情
- `PUT /api/v1/devices/:id/status` - 更新状态
- `POST /api/v1/devices/:id/heartbeat` - 心跳
- `DELETE /api/v1/devices/:id` - 注销设备

### 策略管理
- `POST /api/v1/policies` - 创建策略
- `GET /api/v1/policies` - 列出策略
- `GET /api/v1/policies/:id` - 获取策略详情
- `PUT /api/v1/policies/:id` - 更新策略
- `DELETE /api/v1/policies/:id` - 删除策略

### 备份操作
- `POST /api/v1/backup` - 触发备份
- `GET /api/v1/backup/:id` - 获取备份状态
- `POST /api/v1/backup/:id/cancel` - 取消备份

### 恢复操作
- `POST /api/v1/restore` - 触发恢复
- `GET /api/v1/restore/:snapshot_id` - 获取恢复状态

### 快照管理
- `GET /api/v1/snapshots` - 列出快照
- `GET /api/v1/snapshots/:id` - 获取快照详情
- `DELETE /api/v1/snapshots/:id` - 删除快照

### 通知配置
- `GET /api/v1/notifications/config` - 获取配置
- `PUT /api/v1/notifications/config` - 更新配置

## 配置说明

### OpenLinkConfig (openvault-transport)
```json
{
  "endpoint": "http://localhost:8080",
  "token": "your-token",
  "device_id": "unique-device-id",
  "device_name": "my-device",
  "timeout_secs": 30,
  "max_retries": 3,
  "chunk_size": 10485760,
  "compression": true,
  "storage": {
    "primary": "openlink",
    "backup": null,
    "region": null
  }
}
```

### BackupPolicy (openvault-server)
```json
{
  "name": "Daily Backup",
  "enabled": true,
  "strategy": "incremental",
  "schedule": "0 2 * * *",
  "retention_days": 30,
  "compression": true,
  "encryption": true,
  "exclude_patterns": ["*.tmp", ".git"],
  "include_patterns": ["*"]
}
```

## 测试结果

| Phase | 测试数 | 状态 |
|-------|--------|------|
| Phase 1-3 | 54 | ✅ 全部通过 |
| Phase 4 | 46 | ✅ 全部通过 |

## GitHub
https://github.com/youbanzhishi/OpenVault

## 最后更新
2024-05-10
