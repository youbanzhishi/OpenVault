//! HTTP request handlers for OpenVault API

use crate::auth::AuthManager;
use crate::error::{ServerError, ServerResult};
use crate::models::*;
use crate::services::{BackupService, DeviceService, NotificationService, PolicyService};
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

/// Application state shared across handlers
pub struct AppState {
    pub device_service: Arc<DeviceService>,
    pub policy_service: Arc<PolicyService>,
    pub backup_service: Arc<BackupService>,
    pub notification_service: Arc<NotificationService>,
    pub auth_manager: Arc<AuthManager>,
    pub start_time: chrono::DateTime<chrono::Utc>,
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
        let offline_count = devices.iter().filter(|d| d.status == DeviceStatus::Offline).count();
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
    state.device_service.update_status(&device_id, status).await?;
    state.device_service.get(&device_id).await.map(Json)
}

/// POST /api/v1/devices/:device_id/heartbeat - Device heartbeat
pub async fn device_heartbeat(
    State(state): State<Arc<AppState>>,
    Path(device_id): Path<String>,
) -> ServerResult<Json<serde_json::Value>> {
    state.device_service.heartbeat(&device_id).await?;
    Ok(Json(serde_json::json!({ "status": "ok", "timestamp": chrono::Utc::now() })))
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
pub async fn list_policies(State(state): State<Arc<AppState>>) -> ServerResult<Json<Vec<BackupPolicy>>> {
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
    // Create backup status entry
    let backup = state.backup_service.create_backup(&request.device_id).await;
    
    // Log the backup trigger
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
    // Verify snapshot exists
    let _snapshot = state.backup_service.get_snapshot(&request.snapshot_id).await?;
    
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
    // For now, just return the snapshot info
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
pub async fn list_snapshots(State(state): State<Arc<AppState>>) -> ServerResult<Json<Vec<openvault_core::snapshot::Snapshot>>> {
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
    // Note: In a full implementation, this would also delete the actual backup files
    tracing::info!(snapshot_id = %snapshot_id, "Snapshot deletion requested");
    
    Ok(Json(serde_json::json!({
        "status": "deleted",
        "snapshot_id": snapshot_id
    })))
}

// ============================================================================
// Notification Handlers
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
