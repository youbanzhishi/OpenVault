//! API route definitions for OpenVault server

use crate::handlers::*;
use crate::AppState;
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Create the API router with all routes
pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health & Status
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/status", get(system_status))
        
        // Device Management
        .route("/api/v1/devices", post(register_device))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/:device_id", get(get_device))
        .route("/api/v1/devices/:device_id", delete(unregister_device))
        .route("/api/v1/devices/:device_id/status", put(update_device_status))
        .route("/api/v1/devices/:device_id/heartbeat", post(device_heartbeat))
        .route("/api/v1/devices/:device_id/backups", get(list_device_backups))
        
        // Policy Management
        .route("/api/v1/policies", post(create_policy))
        .route("/api/v1/policies", get(list_policies))
        .route("/api/v1/policies/:policy_id", get(get_policy))
        .route("/api/v1/policies/:policy_id", put(update_policy))
        .route("/api/v1/policies/:policy_id", delete(delete_policy))
        
        // Backup Operations
        .route("/api/v1/backup", post(trigger_backup))
        .route("/api/v1/backup/:backup_id", get(get_backup_status))
        .route("/api/v1/backup/:backup_id/cancel", post(cancel_backup))
        
        // Restore Operations
        .route("/api/v1/restore", post(trigger_restore))
        .route("/api/v1/restore/:snapshot_id", get(get_restore_status))
        
        // Snapshot Management
        .route("/api/v1/snapshots", get(list_snapshots))
        .route("/api/v1/snapshots/:snapshot_id", get(get_snapshot))
        .route("/api/v1/snapshots/:snapshot_id", delete(delete_snapshot))
        
        // Notifications
        .route("/api/v1/notifications/config", get(get_notification_config))
        .route("/api/v1/notifications/config", put(update_notification_config))
        
        // Phase 7: Search & AI endpoints
        .route("/api/v1/search", post(search_files))
        .route("/api/v1/restore/ai", post(ai_restore))
        .route("/api/v1/intel/suggestions", get(get_intel_suggestions))
        
        // Add state and middleware
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}
