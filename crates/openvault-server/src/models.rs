//! Data models for the OpenVault API

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Device registration request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistration {
    pub device_id: Option<String>,
    pub device_name: String,
    pub device_type: DeviceType,
    pub capabilities: Vec<String>,
    pub storage_capacity_bytes: Option<u64>,
}

/// Device type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Server,
    Desktop,
    Laptop,
    Nas,
    Cloud,
    Mobile,
}

impl Default for DeviceType {
    fn default() -> Self {
        DeviceType::Server
    }
}

/// Registered device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub device_id: String,
    pub device_name: String,
    pub device_type: DeviceType,
    pub capabilities: Vec<String>,
    pub storage_capacity_bytes: Option<u64>,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub status: DeviceStatus,
    pub last_backup: Option<DateTime<Utc>>,
    pub region: Option<String>,
}

impl Device {
    pub fn new(registration: DeviceRegistration) -> Self {
        let now = Utc::now();
        Self {
            device_id: registration.device_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            device_name: registration.device_name,
            device_type: registration.device_type,
            capabilities: registration.capabilities,
            storage_capacity_bytes: registration.storage_capacity_bytes,
            registered_at: now,
            last_seen: now,
            status: DeviceStatus::Online,
            last_backup: None,
            region: None,
        }
    }
}

/// Device connection status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Online,
    Offline,
    Busy,
    Error,
}

/// Backup policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPolicy {
    pub policy_id: String,
    pub name: String,
    pub enabled: bool,
    pub strategy: BackupStrategy,
    pub schedule: Option<String>,
    pub retention_days: u32,
    pub compression: bool,
    pub encryption: bool,
    pub exclude_patterns: Vec<String>,
    pub include_patterns: Vec<String>,
    pub target_device_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            policy_id: Uuid::new_v4().to_string(),
            name: "Default Policy".to_string(),
            enabled: true,
            strategy: BackupStrategy::Incremental,
            schedule: Some("0 2 * * *".to_string()), // Daily at 2 AM
            retention_days: 30,
            compression: true,
            encryption: true,
            exclude_patterns: vec![
                "*.tmp".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
                "__pycache__".to_string(),
            ],
            include_patterns: vec!["*".to_string()],
            target_device_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

/// Backup strategy type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackupStrategy {
    Full,
    Incremental,
    Differential,
}

/// Backup status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStatus {
    pub backup_id: String,
    pub device_id: String,
    pub snapshot_id: Option<String>,
    pub status: BackupStatusType,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub bytes_transferred: u64,
    pub bytes_total: u64,
    pub files_total: u64,
    pub files_processed: u64,
    pub errors: Vec<BackupError>,
}

impl Default for BackupStatus {
    fn default() -> Self {
        Self {
            backup_id: Uuid::new_v4().to_string(),
            device_id: String::new(),
            snapshot_id: None,
            status: BackupStatusType::Pending,
            started_at: Utc::now(),
            completed_at: None,
            bytes_transferred: 0,
            bytes_total: 0,
            files_total: 0,
            files_processed: 0,
            errors: Vec::new(),
        }
    }
}

/// Backup execution status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatusType {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

/// Backup error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupError {
    pub code: String,
    pub message: String,
    pub file_path: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Restore request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub snapshot_id: String,
    pub target_device_id: Option<String>,
    pub target_path: Option<String>,
    pub files: Option<Vec<String>>, // If None, restore all
    pub conflict_strategy: ConflictStrategy,
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    Skip,
    Overwrite,
    Rename,
    Ask,
}

/// System status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub version: String,
    pub uptime_seconds: u64,
    pub connected_devices: u32,
    pub total_snapshots: u32,
    pub total_storage_bytes: u64,
    pub active_backups: u32,
    pub health: SystemHealth,
}

/// System health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SystemHealth {
    Healthy,
    Degraded,
    Critical,
}

/// Notification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
    pub events: Vec<NotificationEvent>,
}

/// Events that can trigger notifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEvent {
    BackupCompleted,
    BackupFailed,
    RestoreCompleted,
    RestoreFailed,
    DeviceOffline,
    DeviceOnline,
    SelfHealingStarted,
    SelfHealingCompleted,
    RiskDetected,
}

/// Webhook notification payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub event: NotificationEvent,
    pub timestamp: DateTime<Utc>,
    pub device_id: Option<String>,
    pub snapshot_id: Option<String>,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

// ============================================================================
// Phase 7: Search & AI Models
// ============================================================================

/// Search request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Keyword query string.
    pub query: String,
    /// Maximum number of results to return.
    pub limit: Option<usize>,
}

/// Search result item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponseItem {
    pub path: String,
    pub snippet: String,
    pub relevance: f64,
    pub tags: Vec<String>,
    pub size: u64,
    pub modified_at: DateTime<Utc>,
}

/// AI-powered restore request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRestoreRequest {
    /// Natural language query (e.g., "restore my photos from last week").
    pub query: String,
    /// Target device for restoration.
    pub target_device_id: Option<String>,
    /// Target path for restoration.
    pub target_path: Option<String>,
}

/// AI restore response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRestoreResponse {
    /// Parsed time range.
    pub time_range: Option<TimeRangeResponse>,
    /// Parsed file type filter.
    pub file_type: Option<String>,
    /// Parsed operation type.
    pub operation: Option<String>,
    /// Parsed path pattern.
    pub path_pattern: Option<String>,
    /// Matching files found.
    pub matching_files: Vec<String>,
    /// Original query.
    pub original_query: String,
}

/// Time range in API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRangeResponse {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Intelligence suggestions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelSuggestionsResponse {
    /// File classification suggestions.
    pub classification: Vec<ClassificationSuggestion>,
    /// Scheduling suggestions.
    pub scheduling: Vec<String>,
    /// Risk assessment.
    pub risk: RiskSummary,
}

/// A single classification suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationSuggestion {
    pub path_pattern: String,
    pub category: String,
    pub priority: String,
    pub backup_mode: String,
}

/// Risk summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSummary {
    pub overall_level: String,
    pub factors: Vec<RiskFactorItem>,
    pub recommendation: String,
}

/// A single risk factor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactorItem {
    pub name: String,
    pub severity: String,
    pub description: String,
}
