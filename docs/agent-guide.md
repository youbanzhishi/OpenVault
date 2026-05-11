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

---

## Agent Action Protocol v2 定义

> OpenVault 的 agent.json v2 能力声明，遵循 [Agent Action Protocol](https://github.com/youbanzhishi/open-knowledge-system/blob/main/共享知识/设计模式/Agent-Action-Protocol.md)。

### agent.json v2

```json
{
  "schema_version": "2.0",
  "name": "openvault",
  "description": "安全智能的文件备份与容灾恢复系统——3-2-1策略，AI自愈",
  "version": "1.0.1",
  "base_url": "http://localhost:8090",
  "auth": {
    "type": "bearer",
    "header": "Authorization"
  },
  "capabilities": [
    {
      "name": "retrieve",
      "description": "从备份快照中检索并恢复指定文件到目标路径",
      "category": "execute",
      "endpoint": "POST /api/v1/restore",
      "input": {
        "type": "object",
        "properties": {
          "snapshot_id": {
            "type": "string",
            "description": "快照UUID"
          },
          "target": {
            "type": "string",
            "description": "恢复目标路径"
          },
          "file_pattern": {
            "type": "string",
            "description": "文件匹配模式，支持通配符，如'*.wav'或'vocal_*'"
          }
        },
        "required": ["snapshot_id", "target"]
      },
      "output": {
        "type": "object",
        "properties": {
          "restore_id": { "type": "string", "description": "恢复任务ID" },
          "snapshot_id": { "type": "string" },
          "status": { "type": "string", "enum": ["pending", "running", "completed", "failed"] },
          "files_restored": {
            "type": "array",
            "items": { "type": "string" },
            "description": "已恢复的文件列表"
          }
        }
      },
      "examples": [
        {
          "input": { "snapshot_id": "snap-20240615120000-0000", "target": "/tmp/restored", "file_pattern": "*.wav" },
          "output": {
            "restore_id": "rst-a1b2c3",
            "snapshot_id": "snap-20240615120000-0000",
            "status": "completed",
            "files_restored": ["/tmp/restored/vocal_dry.wav", "/tmp/restored/accomp.wav"]
          }
        }
      ]
    },
    {
      "name": "backup",
      "description": "触发指定设备的备份操作，支持全量/增量/差异策略",
      "category": "execute",
      "endpoint": "POST /api/v1/backup",
      "input": {
        "type": "object",
        "properties": {
          "device_id": {
            "type": "string",
            "description": "设备ID"
          },
          "strategy": {
            "type": "string",
            "enum": ["full", "incremental", "differential"],
            "description": "备份策略，默认incremental"
          },
          "source_path": {
            "type": "string",
            "description": "备份源路径"
          },
          "storage_backends": {
            "type": "array",
            "items": { "type": "string" },
            "description": "存储后端列表，如['local', 's3']"
          }
        },
        "required": ["device_id"]
      },
      "output": {
        "type": "object",
        "properties": {
          "backup_id": { "type": "string", "description": "备份任务ID" },
          "device_id": { "type": "string" },
          "strategy": { "type": "string" },
          "status": { "type": "string", "enum": ["pending", "running", "completed", "failed"] },
          "snapshot_id": { "type": "string", "description": "完成后生成的快照ID" }
        }
      },
      "examples": [
        {
          "input": { "device_id": "dev-001", "strategy": "incremental", "storage_backends": ["local", "s3"] },
          "output": {
            "backup_id": "bak-d4e5f6",
            "device_id": "dev-001",
            "strategy": "incremental",
            "status": "pending",
            "snapshot_id": null
          }
        }
      ]
    },
    {
      "name": "search",
      "description": "在备份文件中搜索，支持关键词和语义搜索，跨快照查询",
      "category": "search",
      "endpoint": "POST /api/v1/search",
      "input": {
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "搜索关键词或语义查询"
          },
          "mode": {
            "type": "string",
            "enum": ["keyword", "semantic"],
            "description": "搜索模式，默认keyword"
          },
          "limit": {
            "type": "integer",
            "description": "返回结果数量，默认20"
          }
        },
        "required": ["query"]
      },
      "output": {
        "type": "object",
        "properties": {
          "results": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "file_path": { "type": "string" },
                "snapshot_id": { "type": "string" },
                "size": { "type": "integer" },
                "modified_at": { "type": "string" },
                "relevance_score": { "type": "number" }
              }
            }
          },
          "total": { "type": "integer" }
        }
      },
      "examples": [
        {
          "input": { "query": "混音工程文件", "mode": "semantic", "limit": 10 },
          "output": {
            "results": [
              {
                "file_path": "/data/projects/夏日之歌.opendaw",
                "snapshot_id": "snap-20240615120000-0000",
                "size": 524288,
                "modified_at": "2024-06-15T10:30:00Z",
                "relevance_score": 0.92
              }
            ],
            "total": 1
          }
        }
      ]
    },
    {
      "name": "verify",
      "description": "验证备份快照的完整性，检查数据校验和与3-2-1合规性",
      "category": "search",
      "endpoint": "GET /api/v1/compliance/check",
      "input": {
        "type": "object",
        "properties": {
          "path": {
            "type": "string",
            "description": "检查路径"
          },
          "region": {
            "type": "string",
            "description": "合规区域，如EU/CN"
          }
        },
        "required": []
      },
      "output": {
        "type": "object",
        "properties": {
          "compliant": { "type": "boolean", "description": "是否合规" },
          "checks": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "rule": { "type": "string" },
                "passed": { "type": "boolean" },
                "detail": { "type": "string" }
              }
            }
          },
          "score": { "type": "number", "description": "合规评分 0-100" }
        }
      },
      "examples": [
        {
          "input": { "path": "/data/important", "region": "CN" },
          "output": {
            "compliant": true,
            "checks": [
              { "rule": "3_copies", "passed": true, "detail": "3份数据副本确认" },
              { "rule": "2_media", "passed": true, "detail": "本地+S3两种存储介质" },
              { "rule": "1_offsite", "passed": true, "detail": "S3异地备份确认" }
            ],
            "score": 100
          }
        }
      ]
    },
    {
      "name": "get_status",
      "description": "获取系统整体状态，包含设备、备份、快照统计",
      "category": "search",
      "endpoint": "GET /api/v1/status",
      "input": {
        "type": "object",
        "properties": {},
        "required": []
      },
      "output": {
        "type": "object",
        "properties": {
          "devices": {
            "type": "object",
            "properties": {
              "total": { "type": "integer" },
              "online": { "type": "integer" }
            }
          },
          "backups": {
            "type": "object",
            "properties": {
              "total": { "type": "integer" },
              "last_success": { "type": "string" }
            }
          },
          "snapshots": {
            "type": "object",
            "properties": {
              "total": { "type": "integer" },
              "total_size_bytes": { "type": "integer" }
            }
          },
          "health": { "type": "string", "enum": ["ok", "degraded", "critical"] }
        }
      },
      "examples": [
        {
          "input": {},
          "output": {
            "devices": { "total": 3, "online": 2 },
            "backups": { "total": 42, "last_success": "2024-06-15T02:00:00Z" },
            "snapshots": { "total": 120, "total_size_bytes": 5368709120 },
            "health": "ok"
          }
        }
      ]
    }
  ],
  "workflows": [
    {
      "name": "knowledge_archive",
      "description": "知识归档流：OpenDAW导出→OpenMind入库→OpenVault备份",
      "steps": [
        { "project": "opendaw", "action": "export" },
        { "project": "openmind", "action": "ingest" },
        { "project": "openvault", "action": "backup" }
      ]
    },
    {
      "name": "disaster_recovery",
      "description": "容灾恢复流：OpenVault检索→验证→恢复文件",
      "steps": [
        { "project": "openvault", "action": "search" },
        { "project": "openvault", "action": "verify" },
        { "project": "openvault", "action": "retrieve" }
      ]
    }
  ],
  "events": {
    "subscribe": "POST /api/v1/events/subscribe",
    "types": ["backup.started", "backup.completed", "backup.failed", "restore.completed", "compliance.violation", "self_healing.triggered"]
  },
  "links": {
    "docs": "https://github.com/youbanzhishi/OpenVault/docs",
    "source": "https://github.com/youbanzhishi/OpenVault",
    "health": "http://localhost:8090/api/v1/health"
  }
}
```
