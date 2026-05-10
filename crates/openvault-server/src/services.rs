//! Core services for OpenVault server

use crate::error::ServerResult;
use crate::models::*;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Device management service
pub struct DeviceService {
    devices: Arc<RwLock<HashMap<String, Device>>>,
}

impl DeviceService {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new device
    pub async fn register(&self, registration: DeviceRegistration) -> ServerResult<Device> {
        let device = Device::new(registration);
        
        let mut devices = self.devices.write().await;
        
        // Check if device already exists
        if devices.contains_key(&device.device_id) {
            // Update existing device
            let existing = devices.get_mut(&device.device_id).unwrap();
            existing.last_seen = Utc::now();
            existing.status = DeviceStatus::Online;
            return Ok(existing.clone());
        }
        
        devices.insert(device.device_id.clone(), device.clone());
        Ok(device)
    }

    /// Get device by ID
    pub async fn get(&self, device_id: &str) -> ServerResult<Device> {
        let devices = self.devices.read().await;
        devices
            .get(device_id)
            .cloned()
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Device {} not found", device_id)))
    }

    /// List all devices
    pub async fn list(&self) -> Vec<Device> {
        let devices = self.devices.read().await;
        devices.values().cloned().collect()
    }

    /// Update device status
    pub async fn update_status(&self, device_id: &str, status: DeviceStatus) -> ServerResult<()> {
        let mut devices = self.devices.write().await;
        let device = devices
            .get_mut(device_id)
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Device {} not found", device_id)))?;
        device.status = status;
        device.last_seen = Utc::now();
        Ok(())
    }

    /// Record device heartbeat
    pub async fn heartbeat(&self, device_id: &str) -> ServerResult<()> {
        let mut devices = self.devices.write().await;
        let device = devices
            .get_mut(device_id)
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Device {} not found", device_id)))?;
        device.last_seen = Utc::now();
        device.status = DeviceStatus::Online;
        Ok(())
    }

    /// Delete device
    pub async fn delete(&self, device_id: &str) -> ServerResult<()> {
        let mut devices = self.devices.write().await;
        devices
            .remove(device_id)
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Device {} not found", device_id)))?;
        Ok(())
    }

    /// Get online devices
    pub async fn online_devices(&self) -> Vec<Device> {
        let devices = self.devices.read().await;
        devices
            .values()
            .filter(|d| d.status == DeviceStatus::Online)
            .cloned()
            .collect()
    }
}

impl Default for DeviceService {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy management service
pub struct PolicyService {
    policies: Arc<RwLock<HashMap<String, BackupPolicy>>>,
}

impl PolicyService {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new policy
    pub async fn create(&self, policy: BackupPolicy) -> ServerResult<BackupPolicy> {
        let mut policies = self.policies.write().await;
        let mut policy = policy;
        policy.policy_id = Uuid::new_v4().to_string();
        policy.created_at = Utc::now();
        policy.updated_at = Utc::now();
        policies.insert(policy.policy_id.clone(), policy.clone());
        Ok(policy)
    }

    /// Get policy by ID
    pub async fn get(&self, policy_id: &str) -> ServerResult<BackupPolicy> {
        let policies = self.policies.read().await;
        policies
            .get(policy_id)
            .cloned()
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Policy {} not found", policy_id)))
    }

    /// List all policies
    pub async fn list(&self) -> Vec<BackupPolicy> {
        let policies = self.policies.read().await;
        policies.values().cloned().collect()
    }

    /// Update a policy
    pub async fn update(&self, policy_id: &str, updates: BackupPolicy) -> ServerResult<BackupPolicy> {
        let mut policies = self.policies.write().await;
        let policy = policies
            .get_mut(policy_id)
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Policy {} not found", policy_id)))?;
        
        policy.name = updates.name;
        policy.enabled = updates.enabled;
        policy.strategy = updates.strategy;
        policy.schedule = updates.schedule;
        policy.retention_days = updates.retention_days;
        policy.compression = updates.compression;
        policy.encryption = updates.encryption;
        policy.exclude_patterns = updates.exclude_patterns;
        policy.include_patterns = updates.include_patterns;
        policy.target_device_id = updates.target_device_id;
        policy.updated_at = Utc::now();
        
        Ok(policy.clone())
    }

    /// Delete a policy
    pub async fn delete(&self, policy_id: &str) -> ServerResult<()> {
        let mut policies = self.policies.write().await;
        policies
            .remove(policy_id)
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Policy {} not found", policy_id)))?;
        Ok(())
    }

    /// Get policies for a device
    pub async fn for_device(&self, device_id: &str) -> Vec<BackupPolicy> {
        let policies = self.policies.read().await;
        policies
            .values()
            .filter(|p| p.target_device_id.as_deref() == Some(device_id))
            .cloned()
            .collect()
    }
}

impl Default for PolicyService {
    fn default() -> Self {
        Self::new()
    }
}

/// Backup management service
pub struct BackupService {
    backups: Arc<RwLock<HashMap<String, BackupStatus>>>,
    snapshots: Arc<RwLock<HashMap<String, openvault_core::snapshot::Snapshot>>>,
}

impl BackupService {
    pub fn new() -> Self {
        Self {
            backups: Arc::new(RwLock::new(HashMap::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new backup status entry
    pub async fn create_backup(&self, device_id: &str) -> BackupStatus {
        let mut backup = BackupStatus::default();
        backup.device_id = device_id.to_string();
        
        let mut backups = self.backups.write().await;
        backups.insert(backup.backup_id.clone(), backup.clone());
        
        backup
    }

    /// Get backup status
    pub async fn get_backup(&self, backup_id: &str) -> ServerResult<BackupStatus> {
        let backups = self.backups.read().await;
        backups
            .get(backup_id)
            .cloned()
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Backup {} not found", backup_id)))
    }

    /// Update backup status
    pub async fn update_backup(&self, backup_id: &str, status: BackupStatusType) -> ServerResult<()> {
        let mut backups = self.backups.write().await;
        let backup = backups
            .get_mut(backup_id)
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Backup {} not found", backup_id)))?;
        
        backup.status = status.clone();
        if matches!(status, BackupStatusType::Completed | BackupStatusType::Failed) {
            backup.completed_at = Some(Utc::now());
        }
        
        Ok(())
    }

    /// List backups for a device
    pub async fn list_for_device(&self, device_id: &str) -> Vec<BackupStatus> {
        let backups = self.backups.read().await;
        backups
            .values()
            .filter(|b| b.device_id == device_id)
            .cloned()
            .collect()
    }

    /// Store snapshot
    pub async fn store_snapshot(&self, snapshot: openvault_core::snapshot::Snapshot) -> ServerResult<()> {
        let mut snapshots = self.snapshots.write().await;
        snapshots.insert(snapshot.id.clone(), snapshot);
        Ok(())
    }

    /// Get snapshot
    pub async fn get_snapshot(&self, snapshot_id: &str) -> ServerResult<openvault_core::snapshot::Snapshot> {
        let snapshots = self.snapshots.read().await;
        snapshots
            .get(snapshot_id)
            .cloned()
            .ok_or_else(|| crate::error::ServerError::NotFound(format!("Snapshot {} not found", snapshot_id)))
    }

    /// List all snapshots
    pub async fn list_snapshots(&self) -> Vec<openvault_core::snapshot::Snapshot> {
        let snapshots = self.snapshots.read().await;
        let mut list: Vec<_> = snapshots.values().cloned().collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    /// Get active backups count
    pub async fn active_count(&self) -> u32 {
        let backups = self.backups.read().await;
        backups
            .values()
            .filter(|b| matches!(b.status, BackupStatusType::Pending | BackupStatusType::Running))
            .count() as u32
    }

    /// Cancel a backup
    pub async fn cancel(&self, backup_id: &str) -> ServerResult<()> {
        self.update_backup(backup_id, BackupStatusType::Cancelled).await
    }
}

impl Default for BackupService {
    fn default() -> Self {
        Self::new()
    }
}

/// Notification service
pub struct NotificationService {
    config: Arc<RwLock<NotificationConfig>>,
}

impl NotificationService {
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(NotificationConfig {
                enabled: false,
                webhook_url: None,
                webhook_secret: None,
                events: vec![
                    NotificationEvent::BackupFailed,
                    NotificationEvent::DeviceOffline,
                    NotificationEvent::RiskDetected,
                ],
            })),
        }
    }

    /// Update notification configuration
    pub async fn configure(&self, config: NotificationConfig) {
        let mut cfg = self.config.write().await;
        *cfg = config;
    }

    /// Get current configuration
    pub async fn get_config(&self) -> NotificationConfig {
        self.config.read().await.clone()
    }

    /// Send a notification for an event
    pub async fn notify(&self, event: NotificationEvent, payload: WebhookPayload) -> ServerResult<()> {
        let config = self.config.read().await;
        
        if !config.enabled {
            return Ok(());
        }

        if !config.events.contains(&event) {
            return Ok(());
        }

        if let Some(webhook_url) = &config.webhook_url {
            let client = reqwest::Client::new();
            
            let mut request = client.post(webhook_url);
            
            if let Some(secret) = &config.webhook_secret {
                // Add HMAC signature
                let body = serde_json::to_string(&payload)?;
                let signature = compute_hmac(&body, secret);
                request = request.header("X-Signature", format!("sha256={}", signature));
            }
            
            request
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
                .map_err(|e| crate::error::ServerError::Internal(format!("Webhook request failed: {}", e)))?;
        }

        Ok(())
    }

    /// Notify backup completed
    pub async fn notify_backup_completed(&self, device_id: &str, snapshot_id: &str) -> ServerResult<()> {
        self.notify(
            NotificationEvent::BackupCompleted,
            WebhookPayload {
                event: NotificationEvent::BackupCompleted,
                timestamp: Utc::now(),
                device_id: Some(device_id.to_string()),
                snapshot_id: Some(snapshot_id.to_string()),
                message: format!("Backup completed for device {}", device_id),
                details: None,
            },
        ).await
    }

    /// Notify backup failed
    pub async fn notify_backup_failed(&self, device_id: &str, error: &str) -> ServerResult<()> {
        self.notify(
            NotificationEvent::BackupFailed,
            WebhookPayload {
                event: NotificationEvent::BackupFailed,
                timestamp: Utc::now(),
                device_id: Some(device_id.to_string()),
                snapshot_id: None,
                message: format!("Backup failed for device {}: {}", device_id, error),
                details: Some(serde_json::json!({ "error": error })),
            },
        ).await
    }

    /// Notify device offline
    pub async fn notify_device_offline(&self, device_id: &str) -> ServerResult<()> {
        self.notify(
            NotificationEvent::DeviceOffline,
            WebhookPayload {
                event: NotificationEvent::DeviceOffline,
                timestamp: Utc::now(),
                device_id: Some(device_id.to_string()),
                snapshot_id: None,
                message: format!("Device {} is offline", device_id),
                details: None,
            },
        ).await
    }
}

impl Default for NotificationService {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute HMAC-SHA256 signature for webhook
fn compute_hmac(message: &str, secret: &str) -> String {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
