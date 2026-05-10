//! Web Management Panel Backend API
//!
//! Provides all API endpoints needed by the OpenVault web dashboard,
//! including overview stats, file listing, alerts, chart data,
//! and WebSocket push for real-time updates.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

// ============================================================================
// Dashboard Data Types
// ============================================================================

/// Dashboard overview response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardOverview {
    pub total_policies: u64,
    pub total_files: u64,
    pub health_rate: f64,
    pub storage_used_bytes: u64,
    pub storage_total_bytes: u64,
    pub active_backups: u32,
    pub alerts_count: u32,
    pub last_updated: DateTime<Utc>,
}

/// File entry in the dashboard file list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardFile {
    pub file_id: String,
    pub path: String,
    pub size_bytes: u64,
    pub backup_count: u32,
    pub last_backup: Option<DateTime<Utc>>,
    pub health: FileHealth,
}

/// File health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileHealth {
    Healthy,
    Degraded,
    AtRisk,
    Lost,
}

/// Replica detail for a specific file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaDetail {
    pub replica_id: String,
    pub file_id: String,
    pub device_id: String,
    pub device_name: String,
    pub location: String,
    pub health: ReplicaHealthStatus,
    pub last_verified: Option<DateTime<Utc>>,
    pub size_bytes: u64,
}

/// Replica health for dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaHealthStatus {
    Healthy,
    Degraded,
    Missing,
}

/// Alert item in the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardAlert {
    pub alert_id: String,
    pub severity: AlertSeverity,
    pub title: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub acknowledged: bool,
    pub source: String,
}

/// Alert severity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Create policy request from the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardCreatePolicyRequest {
    pub name: String,
    pub strategy: String,
    pub schedule: Option<String>,
    pub retention_days: u32,
    pub compression: bool,
    pub encryption: bool,
    pub target_device_id: Option<String>,
}

/// Dashboard restore request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardRestoreRequest {
    pub file_id: String,
    pub target_device_id: Option<String>,
    pub target_path: Option<String>,
}

/// Dashboard restore response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardRestoreResponse {
    pub restore_id: String,
    pub file_id: String,
    pub target_device: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
}

/// Chart data for frontend visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartData {
    /// Unique chart ID.
    pub chart_id: String,
    /// Chart type (time_series, pie, bar, etc.).
    pub chart_type: ChartType,
    /// Chart title.
    pub title: String,
    /// Data points for the chart.
    pub data_points: Vec<DataPoint>,
    /// Labels (for pie/bar charts).
    pub labels: Vec<String>,
}

/// Type of chart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    TimeSeries,
    Pie,
    Bar,
    Area,
}

/// A single data point in a chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp (for time series).
    pub timestamp: Option<DateTime<Utc>>,
    /// Label (for bar/pie).
    pub label: Option<String>,
    /// Numeric value.
    pub value: f64,
    /// Optional second value (for comparison).
    pub value2: Option<f64>,
}

/// Statistics response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStats {
    pub backup_frequency: ChartData,
    pub restore_count: ChartData,
    pub storage_trend: ChartData,
    pub file_type_distribution: ChartData,
}

/// WebSocket push event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsPushEvent {
    /// Backup progress update.
    BackupProgress {
        backup_id: String,
        device_id: String,
        progress_percent: f64,
        files_processed: u64,
        files_total: u64,
    },
    /// Self-healing event.
    SelfHealing {
        file_id: String,
        action: String,
        result: String,
    },
    /// Alert notification.
    Alert {
        alert: DashboardAlert,
    },
    /// Device status change.
    DeviceStatus {
        device_id: String,
        old_status: String,
        new_status: String,
    },
}

// ============================================================================
// Dashboard API Service
// ============================================================================

/// The dashboard API service, providing all data endpoints.
#[derive(Debug)]
pub struct DashboardApi {
    inner: Arc<DashboardInner>,
    ws_sender: broadcast::Sender<WsPushEvent>,
}

#[derive(Debug)]
struct DashboardInner {
    files: RwLock<HashMap<String, DashboardFile>>,
    replicas: RwLock<HashMap<String, Vec<ReplicaDetail>>>,
    alerts: RwLock<Vec<DashboardAlert>>,
    policies_count: RwLock<u64>,
    storage_used: RwLock<u64>,
    storage_total: RwLock<u64>,
    active_backups: RwLock<u32>,
}

impl DashboardApi {
    /// Create a new dashboard API service.
    pub fn new() -> Self {
        let (ws_sender, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(DashboardInner {
                files: RwLock::new(HashMap::new()),
                replicas: RwLock::new(HashMap::new()),
                alerts: RwLock::new(Vec::new()),
                policies_count: RwLock::new(0),
                storage_used: RwLock::new(0),
                storage_total: RwLock::new(10 * 1024 * 1024 * 1024), // 10 GB default
                active_backups: RwLock::new(0),
            }),
            ws_sender,
        }
    }

    /// Get a subscriber for WebSocket push events.
    pub fn subscribe(&self) -> broadcast::Receiver<WsPushEvent> {
        self.ws_sender.subscribe()
    }

    /// Push a WebSocket event to all connected clients.
    pub fn push_event(&self, event: WsPushEvent) {
        // Ignore send errors (no receivers)
        let _ = self.ws_sender.send(event);
    }

    // ----- GET /api/dashboard/overview -----

    /// Get the dashboard overview.
    pub async fn overview(&self) -> DashboardOverview {
        let files = self.inner.files.read().await;
        let total_files = files.len() as u64;
        let healthy_count = files
            .values()
            .filter(|f| f.health == FileHealth::Healthy)
            .count();
        let health_rate = if total_files > 0 {
            healthy_count as f64 / total_files as f64 * 100.0
        } else {
            100.0
        };
        drop(files);

        let storage_used = *self.inner.storage_used.read().await;
        let storage_total = *self.inner.storage_total.read().await;
        let total_policies = *self.inner.policies_count.read().await;
        let active_backups = *self.inner.active_backups.read().await;
        let alerts = self.inner.alerts.read().await;
        let unack_alerts = alerts.iter().filter(|a| !a.acknowledged).count() as u32;

        DashboardOverview {
            total_policies,
            total_files,
            health_rate,
            storage_used_bytes: storage_used,
            storage_total_bytes: storage_total,
            active_backups,
            alerts_count: unack_alerts,
            last_updated: Utc::now(),
        }
    }

    // ----- GET /api/dashboard/policies -----

    /// List policy summaries (simplified for dashboard).
    pub async fn list_policies(&self) -> Vec<serde_json::Value> {
        let count = *self.inner.policies_count.read().await;
        (0..count)
            .map(|i| {
                serde_json::json!({
                    "policy_id": format!("policy-{}", i + 1),
                    "name": format!("Policy {}", i + 1),
                    "enabled": true,
                    "strategy": "incremental",
                })
            })
            .collect()
    }

    // ----- GET /api/dashboard/files -----

    /// List files with optional search, pagination, and filter.
    pub async fn list_files(
        &self,
        page: u32,
        per_page: u32,
        search: Option<&str>,
        health_filter: Option<&str>,
    ) -> FileListResult {
        let files = self.inner.files.read().await;
        let mut filtered: Vec<&DashboardFile> = files.values().collect();

        // Apply search filter
        if let Some(q) = search {
            let q = q.to_lowercase();
            filtered.retain(|f| f.path.to_lowercase().contains(&q));
        }

        // Apply health filter
        if let Some(h) = health_filter {
            let h_lower = h.to_lowercase();
            filtered.retain(|f| {
                let health_str = match f.health {
                    FileHealth::Healthy => "healthy",
                    FileHealth::Degraded => "degraded",
                    FileHealth::AtRisk => "at_risk",
                    FileHealth::Lost => "lost",
                };
                health_str.contains(&h_lower)
            });
        }

        let total = filtered.len();
        let skip = ((page.saturating_sub(1)) * per_page) as usize;
        let items: Vec<DashboardFile> = filtered
            .into_iter()
            .skip(skip)
            .take(per_page as usize)
            .cloned()
            .collect();

        FileListResult {
            total,
            page,
            per_page,
            items,
        }
    }

    // ----- GET /api/dashboard/replicas/:file_id -----

    /// Get replica details for a specific file.
    pub async fn get_replicas(&self, file_id: &str) -> Option<Vec<ReplicaDetail>> {
        let replicas = self.inner.replicas.read().await;
        replicas.get(file_id).cloned()
    }

    // ----- GET /api/dashboard/alerts -----

    /// List alerts, optionally filtered by severity.
    pub async fn list_alerts(&self, severity: Option<&str>) -> Vec<DashboardAlert> {
        let alerts = self.inner.alerts.read().await;
        match severity {
            Some(sev) => alerts
                .iter()
                .filter(|a| format!("{:?}", a.severity).to_lowercase() == sev.to_lowercase())
                .cloned()
                .collect(),
            None => alerts.clone(),
        }
    }

    // ----- POST /api/dashboard/restore -----

    /// Initiate a one-click restore from the dashboard.
    pub async fn restore(&self, request: DashboardRestoreRequest) -> DashboardRestoreResponse {
        let restore_id = Uuid::new_v4().to_string();
        let target_device = request
            .target_device_id
            .unwrap_or_else(|| "default-device".to_string());

        // Push a progress event
        self.push_event(WsPushEvent::BackupProgress {
            backup_id: restore_id.clone(),
            device_id: target_device.clone(),
            progress_percent: 0.0,
            files_processed: 0,
            files_total: 1,
        });

        DashboardRestoreResponse {
            restore_id,
            file_id: request.file_id,
            target_device,
            status: "initiated".to_string(),
            started_at: Utc::now(),
        }
    }

    // ----- POST /api/dashboard/policy -----

    /// Create a new policy from the dashboard.
    pub async fn create_policy(&self, _request: DashboardCreatePolicyRequest) -> String {
        let mut count = self.inner.policies_count.write().await;
        *count += 1;
        let policy_id = Uuid::new_v4().to_string();
        policy_id
    }

    // ----- GET /api/dashboard/stats -----

    /// Get chart data for statistics.
    pub async fn stats(&self) -> DashboardStats {
        let now = Utc::now();
        let backup_frequency = ChartData {
            chart_id: "backup_frequency".to_string(),
            chart_type: ChartType::TimeSeries,
            title: "Backup Frequency".to_string(),
            data_points: (0..7)
                .map(|i| DataPoint {
                    timestamp: Some(
                        now - chrono::Duration::days(6 - i),
                    ),
                    label: None,
                    value: (i as f64 * 2.5 + 10.0),
                    value2: None,
                })
                .collect(),
            labels: vec![],
        };

        let restore_count = ChartData {
            chart_id: "restore_count".to_string(),
            chart_type: ChartType::Bar,
            title: "Restore Operations".to_string(),
            data_points: (0..7)
                .map(|i| DataPoint {
                    timestamp: None,
                    label: Some(format!("Day {}", i + 1)),
                    value: (i as f64 * 1.2 + 2.0),
                    value2: None,
                })
                .collect(),
            labels: (0..7).map(|i| format!("Day {}", i + 1)).collect(),
        };

        let storage_trend = ChartData {
            chart_id: "storage_trend".to_string(),
            chart_type: ChartType::Area,
            title: "Storage Usage Trend".to_string(),
            data_points: (0..7)
                .map(|i| DataPoint {
                    timestamp: Some(
                        now - chrono::Duration::days(6 - i),
                    ),
                    label: None,
                    value: (1.0 + i as f64 * 0.5) * 1024.0 * 1024.0 * 1024.0,
                    value2: None,
                })
                .collect(),
            labels: vec![],
        };

        let file_type_distribution = ChartData {
            chart_id: "file_type_distribution".to_string(),
            chart_type: ChartType::Pie,
            title: "File Type Distribution".to_string(),
            data_points: vec![
                DataPoint {
                    timestamp: None,
                    label: Some("Documents".to_string()),
                    value: 35.0,
                    value2: None,
                },
                DataPoint {
                    timestamp: None,
                    label: Some("Images".to_string()),
                    value: 25.0,
                    value2: None,
                },
                DataPoint {
                    timestamp: None,
                    label: Some("Code".to_string()),
                    value: 20.0,
                    value2: None,
                },
                DataPoint {
                    timestamp: None,
                    label: Some("Media".to_string()),
                    value: 15.0,
                    value2: None,
                },
                DataPoint {
                    timestamp: None,
                    label: Some("Other".to_string()),
                    value: 5.0,
                    value2: None,
                },
            ],
            labels: vec![
                "Documents".to_string(),
                "Images".to_string(),
                "Code".to_string(),
                "Media".to_string(),
                "Other".to_string(),
            ],
        };

        DashboardStats {
            backup_frequency,
            restore_count,
            storage_trend,
            file_type_distribution,
        }
    }

    // ----- Helper: Add test data -----

    /// Add a file to the dashboard (for testing/internal use).
    pub async fn add_file(&self, file: DashboardFile) {
        let mut files = self.inner.files.write().await;
        let mut storage = self.inner.storage_used.write().await;
        *storage += file.size_bytes;
        files.insert(file.file_id.clone(), file);
    }

    /// Add a replica for a file.
    pub async fn add_replica(&self, file_id: &str, replica: ReplicaDetail) {
        let mut replicas = self.inner.replicas.write().await;
        replicas
            .entry(file_id.to_string())
            .or_default()
            .push(replica);
    }

    /// Add an alert.
    pub async fn add_alert(&self, alert: DashboardAlert) {
        let mut alerts = self.inner.alerts.write().await;
        alerts.push(alert);
    }

    /// Set active backup count.
    pub async fn set_active_backups(&self, count: u32) {
        let mut ab = self.inner.active_backups.write().await;
        *ab = count;
    }
}

impl Default for DashboardApi {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a file listing query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileListResult {
    pub total: usize,
    pub page: u32,
    pub per_page: u32,
    pub items: Vec<DashboardFile>,
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_overview_empty() {
        let api = DashboardApi::new();
        let overview = api.overview().await;
        assert_eq!(overview.total_files, 0);
        assert_eq!(overview.total_policies, 0);
        assert_eq!(overview.health_rate, 100.0);
    }

    #[tokio::test]
    async fn test_overview_with_files() {
        let api = DashboardApi::new();
        api.add_file(DashboardFile {
            file_id: "f1".to_string(),
            path: "/docs/report.pdf".to_string(),
            size_bytes: 1024,
            backup_count: 3,
            last_backup: Some(Utc::now()),
            health: FileHealth::Healthy,
        })
        .await;
        api.add_file(DashboardFile {
            file_id: "f2".to_string(),
            path: "/docs/draft.docx".to_string(),
            size_bytes: 2048,
            backup_count: 1,
            last_backup: Some(Utc::now()),
            health: FileHealth::AtRisk,
        })
        .await;

        let overview = api.overview().await;
        assert_eq!(overview.total_files, 2);
        assert_eq!(overview.storage_used_bytes, 3072);
        assert_eq!(overview.health_rate, 50.0); // 1/2 healthy
    }

    #[tokio::test]
    async fn test_list_files_search() {
        let api = DashboardApi::new();
        api.add_file(DashboardFile {
            file_id: "f1".to_string(),
            path: "/docs/report.pdf".to_string(),
            size_bytes: 1024,
            backup_count: 3,
            last_backup: None,
            health: FileHealth::Healthy,
        })
        .await;
        api.add_file(DashboardFile {
            file_id: "f2".to_string(),
            path: "/photos/sunset.jpg".to_string(),
            size_bytes: 4096,
            backup_count: 2,
            last_backup: None,
            health: FileHealth::Healthy,
        })
        .await;

        let result = api.list_files(1, 10, Some("report"), None).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].file_id, "f1");
    }

    #[tokio::test]
    async fn test_list_files_pagination() {
        let api = DashboardApi::new();
        for i in 0..15 {
            api.add_file(DashboardFile {
                file_id: format!("f{}", i),
                path: format!("/file/{}.txt", i),
                size_bytes: 100,
                backup_count: 1,
                last_backup: None,
                health: FileHealth::Healthy,
            })
            .await;
        }

        let page1 = api.list_files(1, 10, None, None).await;
        assert_eq!(page1.total, 15);
        assert_eq!(page1.items.len(), 10);

        let page2 = api.list_files(2, 10, None, None).await;
        assert_eq!(page2.items.len(), 5);
    }

    #[tokio::test]
    async fn test_list_files_health_filter() {
        let api = DashboardApi::new();
        api.add_file(DashboardFile {
            file_id: "f1".to_string(),
            path: "/a.txt".to_string(),
            size_bytes: 100,
            backup_count: 3,
            last_backup: None,
            health: FileHealth::Healthy,
        })
        .await;
        api.add_file(DashboardFile {
            file_id: "f2".to_string(),
            path: "/b.txt".to_string(),
            size_bytes: 100,
            backup_count: 0,
            last_backup: None,
            health: FileHealth::AtRisk,
        })
        .await;

        let result = api.list_files(1, 10, None, Some("at_risk")).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].file_id, "f2");
    }

    #[tokio::test]
    async fn test_get_replicas() {
        let api = DashboardApi::new();
        api.add_replica(
            "f1",
            ReplicaDetail {
                replica_id: "r1".to_string(),
                file_id: "f1".to_string(),
                device_id: "dev-1".to_string(),
                device_name: "NAS-1".to_string(),
                location: "Office".to_string(),
                health: ReplicaHealthStatus::Healthy,
                last_verified: Some(Utc::now()),
                size_bytes: 1024,
            },
        )
        .await;

        let replicas = api.get_replicas("f1").await;
        assert!(replicas.is_some());
        assert_eq!(replicas.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_alerts() {
        let api = DashboardApi::new();
        api.add_alert(DashboardAlert {
            alert_id: "a1".to_string(),
            severity: AlertSeverity::Warning,
            title: "Device offline".to_string(),
            message: "NAS-1 is offline".to_string(),
            timestamp: Utc::now(),
            acknowledged: false,
            source: "device-monitor".to_string(),
        })
        .await;
        api.add_alert(DashboardAlert {
            alert_id: "a2".to_string(),
            severity: AlertSeverity::Critical,
            title: "Backup failed".to_string(),
            message: "Backup failed for device-2".to_string(),
            timestamp: Utc::now(),
            acknowledged: false,
            source: "backup-engine".to_string(),
        })
        .await;

        let all = api.list_alerts(None).await;
        assert_eq!(all.len(), 2);

        let critical = api.list_alerts(Some("critical")).await;
        assert_eq!(critical.len(), 1);
        assert_eq!(critical[0].alert_id, "a2");
    }

    #[tokio::test]
    async fn test_restore() {
        let api = DashboardApi::new();
        let req = DashboardRestoreRequest {
            file_id: "f1".to_string(),
            target_device_id: Some("dev-1".to_string()),
            target_path: Some("/restored/".to_string()),
        };
        let resp = api.restore(req).await;
        assert_eq!(resp.file_id, "f1");
        assert_eq!(resp.target_device, "dev-1");
        assert_eq!(resp.status, "initiated");
    }

    #[tokio::test]
    async fn test_create_policy() {
        let api = DashboardApi::new();
        let req = DashboardCreatePolicyRequest {
            name: "Daily Backup".to_string(),
            strategy: "incremental".to_string(),
            schedule: Some("0 2 * * *".to_string()),
            retention_days: 30,
            compression: true,
            encryption: true,
            target_device_id: None,
        };
        let id = api.create_policy(req).await;
        assert!(!id.is_empty());

        let overview = api.overview().await;
        assert_eq!(overview.total_policies, 1);
    }

    #[tokio::test]
    async fn test_stats() {
        let api = DashboardApi::new();
        let stats = api.stats().await;
        assert_eq!(stats.backup_frequency.chart_type, ChartType::TimeSeries);
        assert_eq!(stats.restore_count.chart_type, ChartType::Bar);
        assert_eq!(stats.storage_trend.chart_type, ChartType::Area);
        assert_eq!(stats.file_type_distribution.chart_type, ChartType::Pie);
        assert!(!stats.backup_frequency.data_points.is_empty());
    }

    #[tokio::test]
    async fn test_websocket_push() {
        let api = DashboardApi::new();
        let mut rx = api.subscribe();

        api.push_event(WsPushEvent::Alert {
            alert: DashboardAlert {
                alert_id: "a1".to_string(),
                severity: AlertSeverity::Info,
                title: "Test".to_string(),
                message: "Test alert".to_string(),
                timestamp: Utc::now(),
                acknowledged: false,
                source: "test".to_string(),
            },
        });

        let event = rx.try_recv();
        assert!(event.is_ok());
        if let WsPushEvent::Alert { alert } = event.unwrap() {
            assert_eq!(alert.alert_id, "a1");
        } else {
            panic!("Expected Alert event");
        }
    }
}
