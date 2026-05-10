//! Multi-Device Management
//!
//! Manages multiple backup devices including registration, heartbeat monitoring,
//! group management, synchronization coordination, and device-policy mapping.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ============================================================================
// Device Profile & Types
// ============================================================================

/// Extended device type including robots and NAS.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    #[default]
    PC,
    NAS,
    Mobile,
    Robot,
    Server,
    Cloud,
}

/// Device online status.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceOnlineStatus {
    Online,
    #[default]
    Offline,
    Busy,
    Error,
}

/// Detailed device profile for multi-device management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub location: String,
    pub status: DeviceOnlineStatus,
    pub storage_capacity_bytes: u64,
    pub storage_used_bytes: u64,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub group: Option<String>,
    /// Tags for flexible grouping.
    pub tags: Vec<String>,
}

impl DeviceProfile {
    /// Create a new device profile.
    pub fn new(name: &str, kind: DeviceKind, location: &str, capacity: u64) -> Self {
        Self {
            device_id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            kind,
            location: location.to_string(),
            status: DeviceOnlineStatus::Online,
            storage_capacity_bytes: capacity,
            storage_used_bytes: 0,
            registered_at: Utc::now(),
            last_heartbeat: Some(Utc::now()),
            group: None,
            tags: Vec::new(),
        }
    }

    /// Check if device is low on storage (< 10% free).
    pub fn is_storage_low(&self) -> bool {
        if self.storage_capacity_bytes == 0 {
            return false;
        }
        let free = self.storage_capacity_bytes.saturating_sub(self.storage_used_bytes);
        free * 10 < self.storage_capacity_bytes
    }

    /// Get free storage percentage.
    pub fn free_percentage(&self) -> f64 {
        if self.storage_capacity_bytes == 0 {
            return 100.0;
        }
        let free = self.storage_capacity_bytes.saturating_sub(self.storage_used_bytes);
        free as f64 / self.storage_capacity_bytes as f64 * 100.0
    }

    /// Check if heartbeat is stale (older than threshold seconds).
    pub fn is_heartbeat_stale(&self, threshold_secs: i64) -> bool {
        match self.last_heartbeat {
            Some(hb) => {
                let elapsed = Utc::now()
                    .signed_duration_since(hb)
                    .to_std()
                    .unwrap_or_default();
                elapsed.as_secs() as i64 > threshold_secs
            }
            None => true,
        }
    }
}

/// Heartbeat check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatReport {
    pub total_devices: usize,
    pub online: usize,
    pub offline: usize,
    pub stale: Vec<String>,
}

// ============================================================================
// Device Registry
// ============================================================================

/// Registry for managing multiple backup devices.
#[derive(Debug, Clone)]
pub struct DeviceRegistry {
    inner: Arc<RwLock<DeviceRegistryInner>>,
}

#[derive(Debug)]
struct DeviceRegistryInner {
    devices: HashMap<String, DeviceProfile>,
    heartbeat_threshold_secs: i64,
}

impl DeviceRegistry {
    /// Create a new device registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DeviceRegistryInner {
                devices: HashMap::new(),
                heartbeat_threshold_secs: 60,
            })),
        }
    }

    /// Create a registry with custom heartbeat threshold.
    pub fn with_heartbeat_threshold(threshold_secs: i64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(DeviceRegistryInner {
                devices: HashMap::new(),
                heartbeat_threshold_secs: threshold_secs,
            })),
        }
    }

    /// Register a device and return its profile.
    pub async fn register(&self, profile: DeviceProfile) -> DeviceProfile {
        let mut inner = self.inner.write().await;
        inner.devices.insert(profile.device_id.clone(), profile.clone());
        profile
    }

    /// Unregister a device.
    pub async fn unregister(&self, device_id: &str) -> Option<DeviceProfile> {
        let mut inner = self.inner.write().await;
        inner.devices.remove(device_id)
    }

    /// Get a device by ID.
    pub async fn get(&self, device_id: &str) -> Option<DeviceProfile> {
        let inner = self.inner.read().await;
        inner.devices.get(device_id).cloned()
    }

    /// List all devices.
    pub async fn list(&self) -> Vec<DeviceProfile> {
        let inner = self.inner.read().await;
        inner.devices.values().cloned().collect()
    }

    /// List devices in a specific group.
    pub async fn list_by_group(&self, group: &str) -> Vec<DeviceProfile> {
        let inner = self.inner.read().await;
        inner
            .devices
            .values()
            .filter(|d| d.group.as_deref() == Some(group))
            .cloned()
            .collect()
    }

    /// List devices by location.
    pub async fn list_by_location(&self, location: &str) -> Vec<DeviceProfile> {
        let inner = self.inner.read().await;
        inner
            .devices
            .values()
            .filter(|d| d.location == location)
            .cloned()
            .collect()
    }

    /// Record a heartbeat from a device.
    pub async fn heartbeat(&self, device_id: &str) -> Option<DeviceProfile> {
        let mut inner = self.inner.write().await;
        if let Some(device) = inner.devices.get_mut(device_id) {
            device.last_heartbeat = Some(Utc::now());
            device.status = DeviceOnlineStatus::Online;
            return Some(device.clone());
        }
        None
    }

    /// Check all device heartbeats and return a report.
    /// Marks stale devices as offline.
    pub async fn check_heartbeats(&self) -> HeartbeatReport {
        let mut inner = self.inner.write().await;
        let threshold = inner.heartbeat_threshold_secs;
        let mut online = 0;
        let mut offline = 0;
        let mut stale = Vec::new();

        for device in inner.devices.values_mut() {
            if device.is_heartbeat_stale(threshold) {
                if device.status != DeviceOnlineStatus::Offline {
                    stale.push(device.device_id.clone());
                }
                device.status = DeviceOnlineStatus::Offline;
                offline += 1;
            } else {
                device.status = DeviceOnlineStatus::Online;
                online += 1;
            }
        }

        let total = inner.devices.len();
        HeartbeatReport {
            total_devices: total,
            online,
            offline,
            stale,
        }
    }

    /// Update device storage usage.
    pub async fn update_storage(&self, device_id: &str, used: u64) -> Option<DeviceProfile> {
        let mut inner = self.inner.write().await;
        if let Some(device) = inner.devices.get_mut(device_id) {
            device.storage_used_bytes = used;
            return Some(device.clone());
        }
        None
    }

    /// Set device group.
    pub async fn set_group(&self, device_id: &str, group: Option<&str>) -> Option<DeviceProfile> {
        let mut inner = self.inner.write().await;
        if let Some(device) = inner.devices.get_mut(device_id) {
            device.group = group.map(|g| g.to_string());
            return Some(device.clone());
        }
        None
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Device Sync Coordinator
// ============================================================================

/// Coordinates backup and restore operations across multiple devices.
#[derive(Debug, Clone)]
pub struct DeviceSyncCoordinator {
    registry: Arc<DeviceRegistry>,
}

impl DeviceSyncCoordinator {
    /// Create a new sync coordinator.
    pub fn new(registry: Arc<DeviceRegistry>) -> Self {
        Self { registry }
    }

    /// Select the best device as a restore source.
    /// Criteria: online, most free space, closest location match.
    pub async fn select_restore_source(
        &self,
        preferred_location: Option<&str>,
        exclude_devices: &[String],
    ) -> Option<DeviceProfile> {
        let devices = self.registry.list().await;
        let mut candidates: Vec<&DeviceProfile> = devices
            .iter()
            .filter(|d| d.status == DeviceOnlineStatus::Online)
            .filter(|d| !exclude_devices.contains(&d.device_id))
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // If preferred location, prioritize matching
        if let Some(loc) = preferred_location {
            let location_matches: Vec<&DeviceProfile> = candidates
                .iter()
                .filter(|d| d.location == loc)
                .cloned()
                .collect();
            if !location_matches.is_empty() {
                candidates = location_matches;
            }
        }

        // Sort by free space (descending)
        candidates.sort_by(|a, b| {
            let free_a = a.storage_capacity_bytes.saturating_sub(a.storage_used_bytes);
            let free_b = b.storage_capacity_bytes.saturating_sub(b.storage_used_bytes);
            free_b.cmp(&free_a)
        });

        candidates.first().cloned().cloned()
    }

    /// Schedule a backup across devices, avoiding offline ones.
    /// Returns the ordered list of device IDs to backup to.
    pub async fn schedule_backup(
        &self,
        target_count: usize,
        preferred_group: Option<&str>,
    ) -> Vec<String> {
        let devices = self.registry.list().await;
        let mut candidates: Vec<DeviceProfile> = devices
            .into_iter()
            .filter(|d| d.status == DeviceOnlineStatus::Online && !d.is_storage_low())
            .collect();

        // Prioritize same group
        if let Some(group) = preferred_group {
            candidates.sort_by(|a, b| {
                let a_in = a.group.as_deref() == Some(group);
                let b_in = b.group.as_deref() == Some(group);
                b_in.cmp(&a_in)
            });
        }

        candidates.into_iter().take(target_count).map(|d| d.device_id).collect()
    }

    /// Get a degraded (fallback) restore strategy when devices are offline.
    /// Returns available online devices or the least-stale offline device.
    pub async fn degraded_restore_strategy(&self) -> DegradedStrategy {
        let devices = self.registry.list().await;
        let online: Vec<DeviceProfile> = devices
            .iter()
            .filter(|d| d.status == DeviceOnlineStatus::Online)
            .cloned()
            .collect();

        if !online.is_empty() {
            return DegradedStrategy::UseOnline { devices: online };
        }

        // All offline — find the one with the most recent heartbeat
        let mut offline: Vec<&DeviceProfile> = devices.iter().collect();
        offline.sort_by(|a, b| {
            let a_time = a.last_heartbeat.unwrap_or(DateTime::UNIX_EPOCH);
            let b_time = b.last_heartbeat.unwrap_or(DateTime::UNIX_EPOCH);
            b_time.cmp(&a_time)
        });

        match offline.first() {
            Some(d) => DegradedStrategy::AllOffline {
                best_candidate: d.device_id.clone(),
                last_seen: d.last_heartbeat,
            },
            None => DegradedStrategy::NoDevices,
        }
    }
}

/// Degraded strategy result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradedStrategy {
    /// Use these online devices.
    UseOnline { devices: Vec<DeviceProfile> },
    /// All devices offline; best candidate has the most recent heartbeat.
    AllOffline {
        best_candidate: String,
        last_seen: Option<DateTime<Utc>>,
    },
    /// No devices registered at all.
    NoDevices,
}

// ============================================================================
// Device Policy Mapper
// ============================================================================

/// Maps devices to backup policies with per-device configuration overrides.
#[derive(Debug, Clone)]
pub struct DevicePolicyMapper {
    inner: Arc<RwLock<DevicePolicyInner>>,
}

#[derive(Debug)]
struct DevicePolicyInner {
    /// policy_id -> default config.
    policies: HashMap<String, PolicyConfig>,
    /// (device_id, policy_id) -> device-specific overrides.
    overrides: HashMap<(String, String), PolicyOverride>,
    /// Capacity warnings: device_id -> warning level (0.0-1.0).
    capacity_warnings: HashMap<String, f64>,
}

/// Policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub policy_id: String,
    pub name: String,
    pub schedule: String,
    pub retention_days: u32,
    pub compression: bool,
    pub encryption: bool,
}

/// Per-device policy overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyOverride {
    pub device_id: String,
    pub policy_id: String,
    pub schedule: Option<String>,
    pub retention_days: Option<u32>,
    pub compression: Option<bool>,
    pub encryption: Option<bool>,
}

impl DevicePolicyMapper {
    /// Create a new device policy mapper.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DevicePolicyInner {
                policies: HashMap::new(),
                overrides: HashMap::new(),
                capacity_warnings: HashMap::new(),
            })),
        }
    }

    /// Add a policy configuration.
    pub async fn add_policy(&self, config: PolicyConfig) {
        let mut inner = self.inner.write().await;
        inner.policies.insert(config.policy_id.clone(), config);
    }

    /// Get effective policy config for a specific device (default + overrides).
    pub async fn get_effective_config(
        &self,
        device_id: &str,
        policy_id: &str,
    ) -> Option<PolicyConfig> {
        let inner = self.inner.read().await;
        let base = inner.policies.get(policy_id)?;

        let mut effective = base.clone();
        if let Some(ov) = inner.overrides.get(&(device_id.to_string(), policy_id.to_string())) {
            if let Some(ref s) = ov.schedule {
                effective.schedule = s.clone();
            }
            if let Some(r) = ov.retention_days {
                effective.retention_days = r;
            }
            if let Some(c) = ov.compression {
                effective.compression = c;
            }
            if let Some(e) = ov.encryption {
                effective.encryption = e;
            }
        }

        Some(effective)
    }

    /// Set a device-specific policy override.
    pub async fn set_override(&self, override_cfg: PolicyOverride) {
        let mut inner = self.inner.write().await;
        inner.overrides.insert(
            (override_cfg.device_id.clone(), override_cfg.policy_id.clone()),
            override_cfg,
        );
    }

    /// Check device capacity and update warnings.
    /// Returns true if a warning was triggered.
    pub async fn check_capacity(&self, device_id: &str, used: u64, total: u64) -> bool {
        let ratio = if total > 0 { used as f64 / total as f64 } else { 0.0 };
        let threshold = 0.9; // 90% used triggers warning
        let triggered = ratio >= threshold;

        let mut inner = self.inner.write().await;
        if triggered {
            inner.capacity_warnings.insert(device_id.to_string(), ratio);
        } else {
            inner.capacity_warnings.remove(device_id);
        }

        triggered
    }

    /// Get all devices with capacity warnings.
    pub async fn get_capacity_warnings(&self) -> HashMap<String, f64> {
        let inner = self.inner.read().await;
        inner.capacity_warnings.clone()
    }

    /// List all policies.
    pub async fn list_policies(&self) -> Vec<PolicyConfig> {
        let inner = self.inner.read().await;
        inner.policies.values().cloned().collect()
    }

    /// Get policies applicable to a device.
    pub async fn policies_for_device(&self, device_id: &str) -> Vec<PolicyConfig> {
        let inner = self.inner.read().await;
        let mut result = Vec::new();
        for (policy_id, base) in &inner.policies {
            let mut cfg = base.clone();
            if let Some(ov) = inner.overrides.get(&(device_id.to_string(), policy_id.clone())) {
                if let Some(ref s) = ov.schedule {
                    cfg.schedule = s.clone();
                }
                if let Some(r) = ov.retention_days {
                    cfg.retention_days = r;
                }
                if let Some(c) = ov.compression {
                    cfg.compression = c;
                }
                if let Some(e) = ov.encryption {
                    cfg.encryption = e;
                }
            }
            result.push(cfg);
        }
        result
    }
}

impl Default for DevicePolicyMapper {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_get_device() {
        let registry = DeviceRegistry::new();
        let profile = DeviceProfile::new("My PC", DeviceKind::PC, "Office", 500 * 1024 * 1024 * 1024);
        let id = profile.device_id.clone();
        registry.register(profile).await;

        let fetched = registry.get(&id).await;
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "My PC");
    }

    #[tokio::test]
    async fn test_heartbeat_and_check() {
        let registry = DeviceRegistry::with_heartbeat_threshold(600); // 10 minutes
        let profile = DeviceProfile::new("NAS-1", DeviceKind::NAS, "Basement", 2_000_000_000_000);
        let id = profile.device_id.clone();
        registry.register(profile).await;

        // Heartbeat should succeed
        let result = registry.heartbeat(&id).await;
        assert!(result.is_some());

        // Check heartbeats — device should be online
        let report = registry.check_heartbeats().await;
        assert_eq!(report.online, 1);
        assert_eq!(report.offline, 0);
    }

    #[tokio::test]
    async fn test_device_groups() {
        let registry = DeviceRegistry::new();
        let p1 = DeviceProfile::new("PC-1", DeviceKind::PC, "Office", 500_000_000_000);
        let p2 = DeviceProfile::new("NAS-1", DeviceKind::NAS, "Basement", 2_000_000_000_000);
        let id1 = p1.device_id.clone();
        let id2 = p2.device_id.clone();
        registry.register(p1).await;
        registry.register(p2).await;

        registry.set_group(&id1, Some("office-group")).await;
        registry.set_group(&id2, Some("home-group")).await;

        let office = registry.list_by_group("office-group").await;
        assert_eq!(office.len(), 1);
        assert_eq!(office[0].device_id, id1);
    }

    #[tokio::test]
    async fn test_storage_low_warning() {
        let mut profile = DeviceProfile::new("Small NAS", DeviceKind::NAS, "Office", 1000);
        profile.storage_used_bytes = 950; // 95% used
        assert!(profile.is_storage_low());
        assert!(profile.free_percentage() < 10.0);
    }

    #[tokio::test]
    async fn test_sync_coordinator_select_source() {
        let registry = Arc::new(DeviceRegistry::new());
        let mut p1 = DeviceProfile::new("NAS-1", DeviceKind::NAS, "Office", 1_000_000);
        p1.storage_used_bytes = 100_000;
        let mut p2 = DeviceProfile::new("NAS-2", DeviceKind::NAS, "Home", 1_000_000);
        p2.storage_used_bytes = 900_000;
        registry.register(p1).await;
        registry.register(p2).await;

        let coord = DeviceSyncCoordinator::new(registry);
        let source = coord.select_restore_source(None, &[]).await;
        assert!(source.is_some());
        // NAS-1 has more free space
        assert_eq!(source.unwrap().name, "NAS-1");
    }

    #[tokio::test]
    async fn test_sync_coordinator_schedule_backup() {
        let registry = Arc::new(DeviceRegistry::new());
        let p1 = DeviceProfile::new("PC-1", DeviceKind::PC, "Office", 1_000_000);
        let p2 = DeviceProfile::new("PC-2", DeviceKind::PC, "Home", 1_000_000);
        registry.register(p1).await;
        registry.register(p2).await;

        let coord = DeviceSyncCoordinator::new(registry);
        let scheduled = coord.schedule_backup(3, None).await;
        assert_eq!(scheduled.len(), 2); // Only 2 devices available
    }

    #[tokio::test]
    async fn test_degraded_strategy_all_offline() {
        let registry = Arc::new(DeviceRegistry::new());
        let mut p1 = DeviceProfile::new("NAS-1", DeviceKind::NAS, "Office", 1_000_000);
        p1.status = DeviceOnlineStatus::Offline;
        p1.last_heartbeat = Some(Utc::now() - chrono::Duration::minutes(5));
        registry.register(p1).await;

        let coord = DeviceSyncCoordinator::new(registry);
        let strategy = coord.degraded_restore_strategy().await;
        match strategy {
            DegradedStrategy::AllOffline { best_candidate, .. } => {
                assert!(!best_candidate.is_empty());
            }
            _ => panic!("Expected AllOffline strategy"),
        }
    }

    #[tokio::test]
    async fn test_policy_mapper_with_overrides() {
        let mapper = DevicePolicyMapper::new();
        mapper
            .add_policy(PolicyConfig {
                policy_id: "pol-1".to_string(),
                name: "Daily Backup".to_string(),
                schedule: "0 2 * * *".to_string(),
                retention_days: 30,
                compression: true,
                encryption: true,
            })
            .await;

        // Override for device-1: shorter retention
        mapper
            .set_override(PolicyOverride {
                device_id: "device-1".to_string(),
                policy_id: "pol-1".to_string(),
                schedule: None,
                retention_days: Some(7),
                compression: None,
                encryption: None,
            })
            .await;

        // Default config should be unchanged
        let default_cfg = mapper.get_effective_config("device-2", "pol-1").await;
        assert!(default_cfg.is_some());
        assert_eq!(default_cfg.unwrap().retention_days, 30);

        // Device-1 should get the override
        let device1_cfg = mapper.get_effective_config("device-1", "pol-1").await;
        assert!(device1_cfg.is_some());
        assert_eq!(device1_cfg.unwrap().retention_days, 7);
    }

    #[tokio::test]
    async fn test_capacity_warning() {
        let mapper = DevicePolicyMapper::new();
        let triggered = mapper.check_capacity("dev-1", 950, 1000).await;
        assert!(triggered);

        let warnings = mapper.get_capacity_warnings().await;
        assert!(warnings.contains_key("dev-1"));

        // Below threshold
        let not_triggered = mapper.check_capacity("dev-2", 500, 1000).await;
        assert!(!not_triggered);

        let warnings2 = mapper.get_capacity_warnings().await;
        assert!(!warnings2.contains_key("dev-2"));
    }
}
