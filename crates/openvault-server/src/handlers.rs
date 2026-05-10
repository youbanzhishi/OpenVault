//! HTTP request handlers for OpenVault API

use crate::auth::AuthManager;
use crate::error::ServerResult;
use crate::models::*;
use crate::services::{
    AuditService, BackupService, ComplianceSvc, DeviceService, InAppNotificationSvc,
    NotificationService, PolicyService, TenantSvc,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};

use openvault_core::audit::{AuditOperation, AuditQuery, AuditResult};
use openvault_core::notification::{Channel, NotificationRule, NotificationType, Severity};
use std::collections::HashMap;
use std::sync::Arc;

/// Application state shared across handlers
pub struct AppState {
    pub device_service: Arc<DeviceService>,
    pub policy_service: Arc<PolicyService>,
    pub backup_service: Arc<BackupService>,
    pub notification_service: Arc<NotificationService>,
    pub auth_manager: Arc<AuthManager>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    // Phase 8 services
    pub audit_service: Arc<AuditService>,
    pub tenant_service: Arc<TenantSvc>,
    pub compliance_service: Arc<ComplianceSvc>,
    pub notification_svc: Arc<InAppNotificationSvc>,
}

impl AppState {
    pub fn new(auth_secret: String) -> Self {
        Self {
            device_service: Arc::new(DeviceService::new()),
            policy_service: Arc::new(PolicyService::new()),
            backup_service: Arc::new(BackupService::new()),
            notification_service: Arc::new(NotificationService::new()),
            auth_manager: Arc::new(AuthManager::new(auth_secret, "openvault".to_string())),
            start_time: chrono::Utc::now(),
            audit_service: Arc::new(AuditService::new()),
            tenant_service: Arc::new(TenantSvc::new()),
            compliance_service: Arc::new(ComplianceSvc::new()),
            notification_svc: Arc::new(InAppNotificationSvc::new()),
        }
    }
}

// ============================================================================
// Health & Status Handlers
// ============================================================================

/// GET /api/v1/health
pub async fn health_check() -> &'static str {
    "OK"
}

/// GET /api/v1/status
pub async fn system_status(State(state): State<Arc<AppState>>) -> ServerResult<Json<SystemStatus>> {
    let device_service = state.device_service.clone();
    let backup_service = state.backup_service.clone();

    let devices = device_service.list().await;
    let snapshots = backup_service.list_snapshots().await;
    let active_backups = backup_service.active_count().await;

    let total_storage: u64 = snapshots.iter().map(|s| s.total_size).sum();

    let health = if devices.is_empty() {
        SystemHealth::Healthy
    } else {
        let offline_count = devices
            .iter()
            .filter(|d| d.status == DeviceStatus::Offline)
            .count();
        if offline_count as f32 / devices.len() as f32 > 0.5 {
            SystemHealth::Critical
        } else if offline_count > 0 {
            SystemHealth::Degraded
        } else {
            SystemHealth::Healthy
        }
    };

    let uptime = chrono::Utc::now()
        .signed_duration_since(state.start_time)
        .to_std()
        .unwrap_or_default()
        .as_secs();

    Ok(Json(SystemStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: uptime,
        connected_devices: devices.len() as u32,
        total_snapshots: snapshots.len() as u32,
        total_storage_bytes: total_storage,
        active_backups,
        health,
    }))
}

// ============================================================================
// Device Handlers
// ============================================================================

/// POST /api/v1/devices - Register a new device
pub async fn register_device(
    State(state): State<Arc<AppState>>,
    Json(registration): Json<DeviceRegistration>,
) -> ServerResult<Json<Device>> {
    let device = state.device_service.register(registration).await?;
    Ok(Json(device))
}

/// GET /api/v1/devices - List all devices
pub async fn list_devices(State(state): State<Arc<AppState>>) -> ServerResult<Json<Vec<Device>>> {
    let devices = state.device_service.list().await;
    Ok(Json(devices))
}

/// GET /api/v1/devices/:device_id - Get device details
pub async fn get_device(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> ServerResult<Json<Device>> {
    let device = state.device_service.get(&device_id).await?;
    Ok(Json(device))
}

/// PUT /api/v1/devices/:device_id/status - Update device status
pub async fn update_device_status(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
    Json(status): Json<DeviceStatus>,
) -> ServerResult<Json<Device>> {
    state
        .device_service
        .update_status(&device_id, status)
        .await?;
    state.device_service.get(&device_id).await.map(Json)
}

/// POST /api/v1/devices/:device_id/heartbeat - Device heartbeat
pub async fn device_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    state.device_service.heartbeat(&device_id).await?;
    Ok(Json(
        serde_json::json!({ "status": "ok", "timestamp": chrono::Utc::now() }),
    ))
}

/// DELETE /api/v1/devices/:device_id - Unregister device
pub async fn unregister_device(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    state.device_service.delete(&device_id).await?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ============================================================================
// Policy Handlers
// ============================================================================

/// POST /api/v1/policies - Create a new policy
pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    Json(policy): Json<BackupPolicy>,
) -> ServerResult<Json<BackupPolicy>> {
    let policy = state.policy_service.create(policy).await?;
    Ok(Json(policy))
}

/// GET /api/v1/policies - List all policies
pub async fn list_policies(
    State(state): State<Arc<AppState>>,
) -> ServerResult<Json<Vec<BackupPolicy>>> {
    let policies = state.policy_service.list().await;
    Ok(Json(policies))
}

/// GET /api/v1/policies/:policy_id - Get policy details
pub async fn get_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> ServerResult<Json<BackupPolicy>> {
    let policy = state.policy_service.get(&policy_id).await?;
    Ok(Json(policy))
}

/// PUT /api/v1/policies/:policy_id - Update a policy
pub async fn update_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
    Json(policy): Json<BackupPolicy>,
) -> ServerResult<Json<BackupPolicy>> {
    let policy = state.policy_service.update(&policy_id, policy).await?;
    Ok(Json(policy))
}

/// DELETE /api/v1/policies/:policy_id - Delete a policy
pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    state.policy_service.delete(&policy_id).await?;
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ============================================================================
// Backup Handlers
// ============================================================================

/// POST /api/v1/backup - Trigger a backup
#[derive(Debug, serde::Deserialize)]
pub struct TriggerBackupRequest {
    pub device_id: String,
    pub policy_id: Option<String>,
    pub source_path: Option<String>,
}

pub async fn trigger_backup(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TriggerBackupRequest>,
) -> ServerResult<Json<BackupStatus>> {
    let backup = state.backup_service.create_backup(&request.device_id).await;

    // Audit log
    let mut meta = HashMap::new();
    meta.insert("device_id".into(), request.device_id.clone());
    if let Some(ref pid) = request.policy_id {
        meta.insert("policy_id".into(), pid.clone());
    }
    let _ = state
        .audit_service
        .append(
            &request.device_id,
            AuditOperation::BackupStarted,
            &backup.backup_id,
            AuditResult::Success,
            meta,
        )
        .await;

    tracing::info!(
        backup_id = %backup.backup_id,
        device_id = %request.device_id,
        "Backup triggered"
    );

    Ok(Json(backup))
}

/// GET /api/v1/backup/:backup_id - Get backup status
pub async fn get_backup_status(
    State(state): State<Arc<AppState>>,
    Path(backup_id): Path<String>,
) -> ServerResult<Json<BackupStatus>> {
    let backup = state.backup_service.get_backup(&backup_id).await?;
    Ok(Json(backup))
}

/// POST /api/v1/backup/:backup_id/cancel - Cancel a running backup
pub async fn cancel_backup(
    State(state): State<Arc<AppState>>,
    Path(backup_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    state.backup_service.cancel(&backup_id).await?;
    Ok(Json(serde_json::json!({ "status": "cancelled" })))
}

/// GET /api/v1/devices/:device_id/backups - List backups for device
pub async fn list_device_backups(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> ServerResult<Json<Vec<BackupStatus>>> {
    let backups = state.backup_service.list_for_device(&device_id).await;
    Ok(Json(backups))
}

// ============================================================================
// Restore Handlers
// ============================================================================

/// POST /api/v1/restore - Trigger a restore
pub async fn trigger_restore(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RestoreRequest>,
) -> ServerResult<Json<serde_json::Value>> {
    let _snapshot = state
        .backup_service
        .get_snapshot(&request.snapshot_id)
        .await?;

    tracing::info!(
        snapshot_id = %request.snapshot_id,
        target_device = ?request.target_device_id,
        "Restore triggered"
    );

    Ok(Json(serde_json::json!({
        "status": "started",
        "snapshot_id": request.snapshot_id,
        "message": "Restore operation has been queued"
    })))
}

/// GET /api/v1/restore/:snapshot_id - Get restore status
pub async fn get_restore_status(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    let snapshot = state.backup_service.get_snapshot(&snapshot_id).await?;

    Ok(Json(serde_json::json!({
        "snapshot_id": snapshot.id,
        "source": snapshot.source,
        "file_count": snapshot.entries.len(),
        "total_size": snapshot.total_size,
        "created_at": snapshot.created_at
    })))
}

// ============================================================================
// Snapshot Handlers
// ============================================================================

/// GET /api/v1/snapshots - List all snapshots
pub async fn list_snapshots(
    State(state): State<Arc<AppState>>,
) -> ServerResult<Json<Vec<openvault_core::snapshot::Snapshot>>> {
    let snapshots = state.backup_service.list_snapshots().await;
    Ok(Json(snapshots))
}

/// GET /api/v1/snapshots/:snapshot_id - Get snapshot details
pub async fn get_snapshot(
    State(state): State<Arc<AppState>>,
    Path(snapshot_id): Path<String>,
) -> ServerResult<Json<openvault_core::snapshot::Snapshot>> {
    let snapshot = state.backup_service.get_snapshot(&snapshot_id).await?;
    Ok(Json(snapshot))
}

/// DELETE /api/v1/snapshots/:snapshot_id - Delete a snapshot
pub async fn delete_snapshot(
    State(_state): State<Arc<AppState>>,
    Path(snapshot_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    tracing::info!(snapshot_id = %snapshot_id, "Snapshot deletion requested");
    Ok(Json(serde_json::json!({
        "status": "deleted",
        "snapshot_id": snapshot_id
    })))
}

// ============================================================================
// Notification Handlers (Phase 5)
// ============================================================================

/// POST /api/v1/notifications/config - Update notification config
pub async fn update_notification_config(
    State(state): State<Arc<AppState>>,
    Json(config): Json<NotificationConfig>,
) -> ServerResult<Json<NotificationConfig>> {
    state.notification_service.configure(config.clone()).await;
    Ok(Json(config))
}

/// GET /api/v1/notifications/config - Get notification config
pub async fn get_notification_config(
    State(state): State<Arc<AppState>>,
) -> ServerResult<Json<NotificationConfig>> {
    let config = state.notification_service.get_config().await;
    Ok(Json(config))
}

// ============================================================================
// Phase 7: Search & AI Handlers
// ============================================================================

use openvault_core::restore::NaturalLanguageQuery;
use openvault_core::search::FileIndex;

/// GET /api/v1/intel/suggestions — Return AI intelligence suggestions.
pub async fn get_intel_suggestions(
    State(_state): State<Arc<AppState>>,
) -> ServerResult<Json<IntelSuggestionsResponse>> {
    let classification = vec![
        ClassificationSuggestion {
            path_pattern: "**/*.rs".to_string(),
            category: "code".to_string(),
            priority: "high".to_string(),
            backup_mode: "realtime".to_string(),
        },
        ClassificationSuggestion {
            path_pattern: "**/*.jpg".to_string(),
            category: "image".to_string(),
            priority: "high".to_string(),
            backup_mode: "scheduled".to_string(),
        },
        ClassificationSuggestion {
            path_pattern: "**/tmp/**".to_string(),
            category: "temp".to_string(),
            priority: "none".to_string(),
            backup_mode: "none".to_string(),
        },
        ClassificationSuggestion {
            path_pattern: "**/*.log".to_string(),
            category: "log".to_string(),
            priority: "low".to_string(),
            backup_mode: "scheduled".to_string(),
        },
        ClassificationSuggestion {
            path_pattern: "**/*.pdf".to_string(),
            category: "document".to_string(),
            priority: "medium".to_string(),
            backup_mode: "scheduled".to_string(),
        },
    ];

    let scheduling = vec![
        "Schedule backups during off-peak hours (22:00-06:00) to minimize bandwidth impact."
            .to_string(),
        "Prioritize code and config files for real-time backup.".to_string(),
        "Consider backing up photos daily rather than in real-time.".to_string(),
    ];

    let risk = RiskSummary {
        overall_level: "low".to_string(),
        factors: vec![],
        recommendation: "All systems healthy. Continue normal operations.".to_string(),
    };

    Ok(Json(IntelSuggestionsResponse {
        classification,
        scheduling,
        risk,
    }))
}

/// POST /api/v1/search — Keyword search over file index.
pub async fn search_files(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> ServerResult<Json<serde_json::Value>> {
    let index = FileIndex::new();
    let results = index.search_keyword(&request.query);
    let items: Vec<SearchResponseItem> = results
        .into_iter()
        .map(|r| SearchResponseItem {
            path: r.path,
            snippet: r.snippet,
            relevance: r.relevance,
            tags: r.tags,
            size: r.size,
            modified_at: r.modified_at,
        })
        .collect();
    Ok(Json(serde_json::json!({
        "query": request.query,
        "results": items,
        "total": items.len(),
    })))
}

/// POST /api/v1/restore/ai — AI-powered natural language restore.
pub async fn ai_restore(
    State(_state): State<Arc<AppState>>,
    Json(request): Json<AiRestoreRequest>,
) -> ServerResult<Json<AiRestoreResponse>> {
    let parsed = NaturalLanguageQuery::parse(&request.query);
    let time_range = parsed.time_range.map(|tr| TimeRangeResponse {
        start: tr.start,
        end: tr.end,
    });
    let file_type = parsed.file_type.map(|ft| ft.to_string());
    let operation = parsed.operation.map(|op| match op {
        openvault_core::restore::OperationFilter::Modified => "modified".to_string(),
        openvault_core::restore::OperationFilter::Created => "created".to_string(),
        openvault_core::restore::OperationFilter::Deleted => "deleted".to_string(),
        openvault_core::restore::OperationFilter::Any => "any".to_string(),
    });
    Ok(Json(AiRestoreResponse {
        time_range,
        file_type,
        operation,
        path_pattern: parsed.path_pattern,
        matching_files: Vec::new(),
        original_query: parsed.original_query,
    }))
}

// ============================================================================
// Phase 8: Audit Handlers
// ============================================================================

/// GET /api/v1/audit — Query audit log.
pub async fn query_audit(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditQueryRequest>,
) -> ServerResult<Json<AuditQueryResponse>> {
    let mut q = AuditQuery::new();
    if let Some(st) = params.start_time {
        q = q.start_time(st);
    }
    if let Some(et) = params.end_time {
        q = q.end_time(et);
    }
    if let Some(ref uid) = params.user_id {
        q = q.user_id(uid);
    }
    if let Some(ref tgt) = params.target {
        q = q.target(tgt);
    }
    if let Some(page) = params.page {
        let per_page = params.per_page.unwrap_or(50);
        q = q.paginate(page, per_page);
    }

    let (total, page, per_page, items) = state.audit_service.query(&q).await;
    Ok(Json(AuditQueryResponse {
        total,
        page,
        per_page,
        items,
    }))
}

// ============================================================================
// Phase 8: Tenant Handlers
// ============================================================================

/// POST /api/v1/tenants — Create a tenant.
pub async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTenantRequest>,
) -> ServerResult<Json<serde_json::Value>> {
    let tenant = state.tenant_service.create_tenant(&req).await?;
    // Audit
    let mut meta = HashMap::new();
    meta.insert("name".into(), req.name.clone());
    let _ = state
        .audit_service
        .append(
            "system",
            AuditOperation::TenantCreated,
            &tenant.tenant_id,
            AuditResult::Success,
            meta,
        )
        .await;
    Ok(Json(serde_json::to_value(tenant).unwrap_or_default()))
}

/// GET /api/v1/tenants/:id/usage — Get tenant usage.
pub async fn get_tenant_usage(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> ServerResult<Json<TenantUsageResponse>> {
    let usage = state.tenant_service.get_usage(&tenant_id).await?;
    Ok(Json(usage))
}

// ============================================================================
// Phase 8: Compliance Handlers
// ============================================================================

/// GET /api/v1/compliance/check — Run compliance check.
pub async fn compliance_check(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ComplianceCheckRequest>,
) -> ServerResult<Json<ComplianceReportResponse>> {
    let report = state
        .compliance_service
        .check(&params.path, &params.region, params.policy_retention_days)
        .await;

    // Audit
    let mut meta = HashMap::new();
    meta.insert("path".into(), params.path.clone());
    meta.insert("region".into(), params.region.clone());
    let audit_result =
        if report.overall_status == openvault_core::compliance::ComplianceStatus::Pass {
            AuditResult::Success
        } else {
            AuditResult::Failure
        };
    let _ = state
        .audit_service
        .append(
            "system",
            AuditOperation::ComplianceCheck,
            &params.path,
            audit_result,
            meta,
        )
        .await;

    Ok(Json(ComplianceReportResponse {
        timestamp: report.timestamp,
        overall_status: format!("{:?}", report.overall_status).to_lowercase(),
        rules_checked: report.rules_checked,
        rules_passed: report.rules_passed,
        rules_failed: report.rules_failed,
        findings: report
            .findings
            .into_iter()
            .map(|f| ComplianceFindingResponse {
                rule_id: f.rule_id,
                rule_name: f.rule_name,
                severity: format!("{:?}", f.severity).to_lowercase(),
                resource: f.resource,
                message: f.message,
                detail: f.detail,
            })
            .collect(),
    }))
}

/// GET /api/v1/compliance/report — Get compliance report (same as check with defaults).
pub async fn compliance_report(
    State(state): State<Arc<AppState>>,
) -> ServerResult<Json<serde_json::Value>> {
    let report = state.compliance_service.check("/", "EU", 365).await;
    Ok(Json(serde_json::to_value(&report).unwrap_or_default()))
}

// ============================================================================
// Phase 8: Notification Handlers (enhanced)
// ============================================================================

/// GET /api/v1/notifications — List notifications.
pub async fn list_notifications(
    State(state): State<Arc<AppState>>,
) -> ServerResult<Json<NotificationListResponse>> {
    let resp = state.notification_svc.list().await;
    Ok(Json(resp))
}

/// POST /api/v1/notifications/rules — Configure notification rule.
pub async fn create_notification_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateNotificationRuleRequest>,
) -> ServerResult<Json<serde_json::Value>> {
    let min_severity = match req.min_severity.as_deref() {
        Some("warning") => Severity::Warning,
        Some("error") => Severity::Error,
        Some("critical") => Severity::Critical,
        _ => Severity::Info,
    };
    let channels: Vec<Channel> = req
        .channels
        .iter()
        .map(|c| match c.as_str() {
            "webhook" => Channel::Webhook,
            "email" => Channel::Email,
            _ => Channel::InApp,
        })
        .collect();
    let notification_types: Vec<NotificationType> = req
        .notification_types
        .iter()
        .map(|t| match t.as_str() {
            "backup_completed" => NotificationType::BackupCompleted,
            "backup_failed" => NotificationType::BackupFailed,
            "compliance_violation" => NotificationType::ComplianceViolation,
            "quota_warning" => NotificationType::QuotaWarning,
            "risk_warning" => NotificationType::RiskWarning,
            _ => NotificationType::Custom(t.clone()),
        })
        .collect();

    let rule = NotificationRule {
        rule_id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        notification_types,
        min_severity,
        channels,
        dedup_minutes: req.dedup_minutes.unwrap_or(5),
        enabled: true,
        webhook_url: req.webhook_url,
    };
    let rule_id = rule.rule_id.clone();
    state.notification_svc.set_rule(rule).await;
    Ok(Json(
        serde_json::json!({ "rule_id": rule_id, "status": "created" }),
    ))
}
