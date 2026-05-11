# OpenVault AI 智能体内置指南

> 供 AI Agent 快速理解与接入 OpenVault 的结构化参考

---

## 一句话定位

OpenVault 是安全智能的文件备份与容灾恢复系统，实现 3-2-1 备份策略，带 AI 自愈能力。口号：「狡兔三窟，AI守护，永不丢失」。

---

## 核心概念

| 概念 | 说明 |
|------|------|
| **3-2-1 策略** | 3 份数据副本、2 种存储介质、1 份异地备份 |
| **Snapshot（快照）** | 备份的时间点快照，支持全量/增量/差异 |
| **Self-Healing（自愈）** | 自动检测并修复损坏文件，从健康副本恢复 |
| **Compliance（合规）** | 3-2-1 策略合规检查与报告生成 |
| **AI Restore** | 自然语言恢复，如"恢复上周的文档" |
| **Policy（策略）** | 备份策略定义：调度、保留、存储后端 |
| **Device（设备）** | 注册的备份设备，支持心跳和状态管理 |
| **Tenant（租户）** | 多租户隔离，含配额管理 |

---

## API 速查表

### 健康与状态

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| GET | `/api/v1/health` | 健康检查 | 否 |
| GET | `/api/v1/status` | 系统状态 | 否 |

### 设备管理

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| POST | `/api/v1/devices` | 注册设备 | 是 |
| GET | `/api/v1/devices` | 列出设备 | 否 |
| GET | `/api/v1/devices/:id` | 设备详情 | 否 |
| DELETE | `/api/v1/devices/:id` | 注销设备 | 是 |
| PUT | `/api/v1/devices/:id/status` | 更新设备状态 | 是 |
| POST | `/api/v1/devices/:id/heartbeat` | 设备心跳 | 否 |
| GET | `/api/v1/devices/:id/backups` | 设备备份列表 | 否 |

### 策略管理

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| POST | `/api/v1/policies` | 创建策略 | 是 |
| GET | `/api/v1/policies` | 列出策略 | 否 |
| GET | `/api/v1/policies/:id` | 策略详情 | 否 |
| PUT | `/api/v1/policies/:id` | 更新策略 | 是 |
| DELETE | `/api/v1/policies/:id` | 删除策略 | 是 |

### 备份操作

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| POST | `/api/v1/backup` | 触发备份 | 是 |
| GET | `/api/v1/backup/:id` | 备份状态 | 否 |
| POST | `/api/v1/backup/:id/cancel` | 取消备份 | 是 |

### 恢复操作

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| POST | `/api/v1/restore` | 触发恢复 | 是 |
| GET | `/api/v1/restore/:snapshot_id` | 恢复状态 | 否 |
| POST | `/api/v1/restore/ai` | AI 自然语言恢复 | 是 |

### 快照管理

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| GET | `/api/v1/snapshots` | 列出快照 | 否 |
| GET | `/api/v1/snapshots/:id` | 快照详情 | 否 |
| DELETE | `/api/v1/snapshots/:id` | 删除快照 | 是 |

### 搜索 & AI

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| POST | `/api/v1/search` | 搜索备份文件 | 否 |
| GET | `/api/v1/intel/suggestions` | AI 智能建议 | 否 |

### 审计

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| GET | `/api/v1/audit` | 查询审计日志 | 否 |

### 租户

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| POST | `/api/v1/tenants` | 创建租户 | 是 |
| GET | `/api/v1/tenants/:id/usage` | 租户用量 | 否 |

### 合规

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| GET | `/api/v1/compliance/check` | 合规检查 | 否 |
| GET | `/api/v1/compliance/report` | 合规报告 | 否 |

### 通知

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| GET | `/api/v1/notifications/config` | 获取通知配置 | 否 |
| PUT | `/api/v1/notifications/config` | 更新通知配置 | 是 |
| GET | `/api/v1/notifications` | 列出通知 | 否 |
| POST | `/api/v1/notifications/rules` | 创建通知规则 | 是 |

### Agent 发现

| 方法 | 路径 | 说明 | 认证 |
|------|------|------|------|
| GET | `/.well-known/agent.json` | Agent 清单（发现协议） | 否 |

---

## AI 能力重点说明

### 1. AI 自然语言恢复（核心能力）

**端点**：`POST /api/v1/restore/ai`

将自然语言查询解析为结构化恢复操作。支持解析：

- **时间范围**：上周、去年、2024年1月
- **文件类型**：文档、图片、代码
- **操作类型**：新建、修改、删除
- **路径模式**：通配符匹配

### 2. 智能建议

**端点**：`GET /api/v1/intel/suggestions`

返回三类建议：
- `classification`：文件分类与备份优先级
- `scheduling`：调度时间优化
- `risk`：风险评估与改进建议

### 3. 文件搜索

**端点**：`POST /api/v1/search`

支持关键词搜索和语义搜索，跨快照查询。

---

## Agent 接入步骤

### 1. 发现服务

```bash
curl http://localhost:8090/.well-known/agent.json
```

获取服务能力清单、API 端点、认证方式。

### 2. 获取 JWT Token

写操作需要 Bearer Token 认证。Token 通过设备注册或管理接口获取。

### 3. 调用 API

所有请求添加 `Authorization: Bearer <jwt>` 头（写操作必需）。

---

## curl 示例

### 健康检查

```bash
curl http://localhost:8090/api/v1/health
```

### 系统状态

```bash
curl http://localhost:8090/api/v1/status
```

### 触发备份

```bash
curl -X POST http://localhost:8090/api/v1/backup \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"device_id": "dev-001", "strategy": "incremental"}'
```

### 触发恢复

```bash
curl -X POST http://localhost:8090/api/v1/restore \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"snapshot_id": "snap-20240101120000-0000", "target": "/tmp/restored"}'
```

### AI 自然语言恢复

```bash
curl -X POST http://localhost:8090/api/v1/restore/ai \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"query": "恢复上周修改的所有文档"}'
```

### 搜索文件

```bash
curl -X POST http://localhost:8090/api/v1/search \
  -H "Content-Type: application/json" \
  -d '{"query": "季度报告", "mode": "keyword", "limit": 20}'
```

### 获取 AI 建议

```bash
curl http://localhost:8090/api/v1/intel/suggestions
```

### 合规检查

```bash
curl "http://localhost:8090/api/v1/compliance/check?path=/data&region=EU"
```

### 审计日志

```bash
curl "http://localhost:8090/api/v1/audit?limit=50"
```

### 注册设备

```bash
curl -X POST http://localhost:8090/api/v1/devices \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{"name": "office-laptop", "os": "linux", "hostname": "thinkpad-x1"}'
```

### 创建策略

```bash
curl -X POST http://localhost:8090/api/v1/policies \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "daily-incremental",
    "source": "/data/important",
    "strategy": "incremental",
    "schedule": "0 2 * * *",
    "retention_days": 30,
    "storage_backends": ["local", "s3"]
  }'
```

### 创建通知规则

```bash
curl -X POST http://localhost:8090/api/v1/notifications/rules \
  -H "Authorization: Bearer <jwt>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "备份失败告警",
    "notification_types": ["backup_failed"],
    "min_severity": "error",
    "channels": ["webhook"],
    "webhook_url": "https://hooks.example.com/alerts"
  }'
```

---

## 错误码参考

| 错误码 | HTTP 状态 | 说明 |
|--------|-----------|------|
| `unauthorized` | 401 | JWT 缺失或无效 |
| `forbidden` | 403 | 权限不足 |
| `device_not_found` | 404 | 设备不存在 |
| `policy_not_found` | 404 | 策略不存在 |
| `snapshot_not_found` | 404 | 快照不存在 |
| `backup_not_found` | 404 | 备份不存在 |
| `conflict` | 409 | 资源已存在 |
| `validation_error` | 422 | 请求体无效 |
| `internal_error` | 500 | 服务器内部错误 |
| `quota_exceeded` | 429 | 租户配额超限 |

---

*OpenVault v1.0.1 — Agent 接入指南*
