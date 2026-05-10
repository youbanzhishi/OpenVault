# OpenVault API Reference

Complete reference for the OpenVault HTTP API server. All endpoints are prefixed with `/api/v1/`.

## Base URL

```
http://<host>:8090/api/v1/
```

## Authentication

All write operations require a JWT token in the `Authorization` header:

```
Authorization: Bearer <token>
```

Tokens are obtained by registering a device or via the admin interface.

---

## Endpoints

### Health & Status

#### `GET /api/v1/health`

Health check endpoint. Returns `OK` if the server is running.

**Response**: `200 OK`

```
OK
```

---

#### `GET /api/v1/status`

System status overview.

**Response**: `200 OK`

```json
{
  "version": "1.0.0",
  "uptime_seconds": 86400,
  "connected_devices": 3,
  "total_snapshots": 42,
  "total_storage_bytes": 107374182400,
  "active_backups": 1,
  "health": "healthy"
}
```

**`health` values**: `healthy`, `degraded`, `critical`

---

### Device Management

#### `POST /api/v1/devices`

Register a new device.

**Request Body**:

```json
{
  "name": "laptop-office",
  "os": "linux",
  "hostname": "thinkpad-x1",
  "ip_address": "192.168.1.100"
}
```

**Response**: `201 Created`

```json
{
  "device_id": "dev-abc123",
  "name": "laptop-office",
  "status": "online",
  "registered_at": "2024-01-15T10:30:00Z"
}
```

---

#### `GET /api/v1/devices`

List all registered devices.

**Response**: `200 OK`

```json
[
  {
    "device_id": "dev-abc123",
    "name": "laptop-office",
    "status": "online",
    "last_heartbeat": "2024-01-15T10:35:00Z"
  }
]
```

---

#### `GET /api/v1/devices/:device_id`

Get details for a specific device.

**Path Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `device_id` | string | Device identifier |

**Response**: `200 OK`

```json
{
  "device_id": "dev-abc123",
  "name": "laptop-office",
  "os": "linux",
  "hostname": "thinkpad-x1",
  "status": "online",
  "registered_at": "2024-01-15T10:30:00Z",
  "last_heartbeat": "2024-01-15T10:35:00Z"
}
```

**Error**: `404 Not Found`

```json
{
  "error": "device_not_found",
  "message": "Device dev-xyz789 not found"
}
```

---

#### `DELETE /api/v1/devices/:device_id`

Unregister a device.

**Response**: `204 No Content`

---

#### `PUT /api/v1/devices/:device_id/status`

Update a device's status.

**Request Body**:

```json
{
  "status": "offline"
}
```

**Response**: `200 OK`

---

#### `POST /api/v1/devices/:device_id/heartbeat`

Send a device heartbeat.

**Response**: `200 OK`

```json
{
  "acknowledged": true,
  "server_time": "2024-01-15T10:36:00Z"
}
```

---

#### `GET /api/v1/devices/:device_id/backups`

List backups for a specific device.

**Response**: `200 OK`

```json
[
  {
    "backup_id": "bak-001",
    "snapshot_id": "snap-20240115103000-0000",
    "strategy": "full",
    "status": "completed",
    "started_at": "2024-01-15T02:00:00Z",
    "completed_at": "2024-01-15T02:15:00Z"
  }
]
```

---

### Policy Management

#### `POST /api/v1/policies`

Create a new backup policy.

**Request Body**:

```json
{
  "name": "daily-full",
  "source": "/data/important",
  "strategy": "full",
  "schedule": "0 2 * * *",
  "retention_days": 30,
  "storage_backends": ["local", "s3"]
}
```

**Response**: `201 Created`

---

#### `GET /api/v1/policies`

List all policies.

**Response**: `200 OK` — Array of policy objects.

---

#### `GET /api/v1/policies/:policy_id`

Get a specific policy.

---

#### `PUT /api/v1/policies/:policy_id`

Update a policy.

---

#### `DELETE /api/v1/policies/:policy_id`

Delete a policy.

**Response**: `204 No Content`

---

### Backup Operations

#### `POST /api/v1/backup`

Trigger a backup operation.

**Request Body**:

```json
{
  "policy_id": "pol-001",
  "strategy": "incremental",
  "source": "/data/important"
}
```

**Response**: `202 Accepted`

```json
{
  "backup_id": "bak-002",
  "status": "running",
  "started_at": "2024-01-15T10:30:00Z"
}
```

---

#### `GET /api/v1/backup/:backup_id`

Get the status of a backup operation.

**Response**: `200 OK`

```json
{
  "backup_id": "bak-002",
  "status": "completed",
  "progress_pct": 100,
  "files_processed": 1234,
  "bytes_processed": 5368709120,
  "started_at": "2024-01-15T10:30:00Z",
  "completed_at": "2024-01-15T10:45:00Z"
}
```

**`status` values**: `queued`, `running`, `completed`, `failed`, `cancelled`

---

#### `POST /api/v1/backup/:backup_id/cancel`

Cancel a running backup.

**Response**: `200 OK`

```json
{
  "backup_id": "bak-002",
  "status": "cancelled"
}
```

---

### Restore Operations

#### `POST /api/v1/restore`

Trigger a restore operation.

**Request Body**:

```json
{
  "snapshot_id": "snap-20240115103000-0000",
  "target": "/tmp/restored",
  "conflict_strategy": "rename",
  "verify_checksums": true,
  "filter_paths": ["docs/", "images/"]
}
```

**Response**: `202 Accepted`

---

#### `GET /api/v1/restore/:snapshot_id`

Get restore status.

---

#### `POST /api/v1/restore/ai`

AI-powered natural language restore.

**Request Body**:

```json
{
  "query": "restore my tax documents from December 2023"
}
```

**Response**: `200 OK`

```json
{
  "snapshot_id": "snap-20231201020000-0000",
  "files_matched": 5,
  "restore_started": true
}
```

---

### Snapshot Management

#### `GET /api/v1/snapshots`

List all snapshots.

**Query Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `strategy` | string | Filter by strategy (full/incremental/differential) |
| `limit` | integer | Max results (default: 50) |

---

#### `GET /api/v1/snapshots/:snapshot_id`

Get snapshot details.

---

#### `DELETE /api/v1/snapshots/:snapshot_id`

Delete a snapshot.

---

### Search

#### `POST /api/v1/search`

Search for files across all snapshots.

**Request Body**:

```json
{
  "query": "quarterly report",
  "mode": "keyword",
  "limit": 20
}
```

**`mode` values**: `keyword`, `semantic`

**Response**: `200 OK`

```json
{
  "results": [
    {
      "path": "/data/reports/Q4-2023.pdf",
      "relevance": 0.95,
      "size_bytes": 1048576,
      "snapshot_id": "snap-20240101120000-0000"
    }
  ],
  "total": 1
}
```

---

### Audit

#### `GET /api/v1/audit`

Query audit log entries.

**Query Parameters**:

| Name | Type | Description |
|------|------|-------------|
| `operation` | string | Filter by operation type |
| `from` | ISO8601 | Start time |
| `to` | ISO8601 | End time |
| `limit` | integer | Max results |

---

### Tenants (Multi-tenant)

#### `POST /api/v1/tenants`

Create a new tenant.

**Request Body**:

```json
{
  "name": "acme-corp",
  "quota": {
    "max_storage_bytes": 1099511627776,
    "max_devices": 50,
    "max_snapshots": 500
  }
}
```

---

#### `GET /api/v1/tenants/:id/usage`

Get tenant resource usage.

---

### Compliance

#### `GET /api/v1/compliance/check`

Run a compliance check.

---

#### `GET /api/v1/compliance/report`

Generate a compliance report.

---

### Notifications

#### `GET /api/v1/notifications`

List recent notifications.

#### `GET /api/v1/notifications/config`

Get notification configuration.

#### `PUT /api/v1/notifications/config`

Update notification configuration.

#### `POST /api/v1/notifications/rules`

Create a notification rule.

**Request Body**:

```json
{
  "event_type": "backup_failed",
  "channels": ["webhook", "email"],
  "webhook_url": "https://hooks.example.com/backup-alerts",
  "severity": "critical"
}
```

---

#### `GET /api/v1/intel/suggestions`

Get AI-powered backup suggestions.

---

## Error Codes

All error responses follow this format:

```json
{
  "error": "<error_code>",
  "message": "<human-readable description>"
}
```

| Error Code | HTTP Status | Description |
|------------|-------------|-------------|
| `unauthorized` | 401 | Missing or invalid JWT token |
| `forbidden` | 403 | Insufficient permissions |
| `device_not_found` | 404 | Device ID does not exist |
| `policy_not_found` | 404 | Policy ID does not exist |
| `snapshot_not_found` | 404 | Snapshot ID does not exist |
| `backup_not_found` | 404 | Backup ID does not exist |
| `conflict` | 409 | Resource already exists |
| `validation_error` | 422 | Invalid request body |
| `internal_error` | 500 | Unexpected server error |
| `quota_exceeded` | 429 | Tenant quota exceeded |

---

*OpenVault v1.0.0 — Phase 10 Documentation*
