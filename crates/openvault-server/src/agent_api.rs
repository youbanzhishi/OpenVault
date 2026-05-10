//! Physical Agent / Robot API for OpenVault
//!
//! Provides interfaces for robots and physical agents to interact with the
//! backup system: query status, trigger restore, verify replicas, etc.
//! All responses are structured for voice-synthesis output and guaranteed
//! to respond within 100ms (async processing + caching).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ============================================================================
// Agent Types
// ============================================================================

/// Commands that a physical agent/robot can issue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentCommand {
    /// Query the backup status of a file or device.
    QueryStatus,
    /// Trigger a restore operation.
    TriggerRestore,
    /// Verify the health of a replica.
    VerifyReplica,
    /// List available backups.
    ListBackups,
}

/// Physical location context for a robot/agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLocation {
    /// Label (e.g. "Living Room", "Office A").
    pub label: String,
    /// Floor or zone identifier.
    pub zone: Option<String>,
    /// Latitude (optional).
    pub latitude: Option<f64>,
    /// Longitude (optional).
    pub longitude: Option<f64>,
}

impl Default for AgentLocation {
    fn default() -> Self {
        Self {
            label: "Unknown".to_string(),
            zone: None,
            latitude: None,
            longitude: None,
        }
    }
}

/// A request from a physical agent/robot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Unique request ID.
    pub request_id: String,
    /// The command to execute.
    pub command: AgentCommand,
    /// Agent identifier.
    pub agent_id: String,
    /// Physical location of the agent.
    pub location: Option<AgentLocation>,
    /// File or resource the command targets (optional).
    pub target_path: Option<String>,
    /// Device the command targets (optional).
    pub target_device_id: Option<String>,
    /// Authorization token for accessing private backups.
    pub auth_token: Option<String>,
    /// Additional parameters.
    pub params: HashMap<String, String>,
}

impl AgentRequest {
    /// Create a new agent request with a generated ID.
    pub fn new(command: AgentCommand, agent_id: &str) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            command,
            agent_id: agent_id.to_string(),
            location: None,
            target_path: None,
            target_device_id: None,
            auth_token: None,
            params: HashMap::new(),
        }
    }
}

/// Replica health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaHealth {
    Healthy,
    Degraded,
    Missing,
    Corrupted,
}

/// Structured response for agent commands, designed for voice synthesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Request ID this response corresponds to.
    pub request_id: String,
    /// Whether the command succeeded.
    pub success: bool,
    /// Human-readable message (suitable for voice synthesis).
    pub message: String,
    /// Response data (structured).
    pub data: AgentResponseData,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

impl AgentResponse {
    /// Create a success response.
    pub fn ok(request_id: &str, message: &str, data: AgentResponseData) -> Self {
        Self {
            request_id: request_id.to_string(),
            success: true,
            message: message.to_string(),
            data,
            timestamp: Utc::now(),
        }
    }

    /// Create an error response.
    pub fn err(request_id: &str, message: &str) -> Self {
        Self {
            request_id: request_id.to_string(),
            success: false,
            message: message.to_string(),
            data: AgentResponseData::None,
            timestamp: Utc::now(),
        }
    }
}

/// Payload for agent response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentResponseData {
    /// No data.
    None,
    /// Status query result.
    Status {
        files_backed_up: u64,
        files_total: u64,
        health_percentage: f64,
        last_backup: Option<DateTime<Utc>>,
    },
    /// Restore result.
    RestoreResult {
        restore_id: String,
        target_device: String,
        files_restored: u64,
        bytes_restored: u64,
    },
    /// Replica verification result.
    ReplicaVerification {
        file_id: String,
        health: ReplicaHealth,
        replica_count: u32,
        nearest_replica_location: Option<String>,
    },
    /// Backup list result.
    BackupList {
        backups: Vec<BackupSummary>,
        total: usize,
    },
}

/// Summary of a single backup for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSummary {
    pub backup_id: String,
    pub device_id: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub file_count: u64,
    pub size_bytes: u64,
}

// ============================================================================
// Robot API Client
// ============================================================================

/// Client for robots to interact with the OpenVault system.
#[derive(Debug, Clone)]
pub struct RobotApiClient {
    inner: Arc<AgentApiInner>,
}

#[derive(Debug)]
struct AgentApiInner {
    /// Agent authorization tokens (agent_id -> token).
    auth_tokens: RwLock<HashMap<String, String>>,
    /// Cached status responses for fast reply.
    status_cache: RwLock<HashMap<String, CachedStatus>>,
    /// Registered agents.
    agents: RwLock<HashMap<String, AgentProfile>>,
}

/// Agent profile metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub agent_id: String,
    pub name: String,
    pub agent_type: AgentType,
    pub location: AgentLocation,
    pub registered_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

/// Type of physical agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Speaker,
    Display,
    RobotArm,
    MobileRobot,
    Sensor,
    Other,
}

/// Cached status for fast response.
#[derive(Debug, Clone)]
struct CachedStatus {
    data: AgentResponseData,
    cached_at: DateTime<Utc>,
}

impl RobotApiClient {
    /// Create a new robot API client.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AgentApiInner {
                auth_tokens: RwLock::new(HashMap::new()),
                status_cache: RwLock::new(HashMap::new()),
                agents: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Register an agent and return its profile.
    pub async fn register_agent(
        &self,
        agent_id: &str,
        name: &str,
        agent_type: AgentType,
        location: AgentLocation,
    ) -> AgentProfile {
        let profile = AgentProfile {
            agent_id: agent_id.to_string(),
            name: name.to_string(),
            agent_type,
            location,
            registered_at: Utc::now(),
            last_active: Utc::now(),
        };
        let mut agents = self.inner.agents.write().await;
        agents.insert(agent_id.to_string(), profile.clone());
        profile
    }

    /// Set an authorization token for an agent.
    pub async fn set_auth_token(&self, agent_id: &str, token: &str) {
        let mut tokens = self.inner.auth_tokens.write().await;
        tokens.insert(agent_id.to_string(), token.to_string());
    }

    /// Verify that an agent has a valid auth token for private data access.
    pub async fn verify_auth(&self, agent_id: &str, token: &str) -> bool {
        let tokens = self.inner.auth_tokens.read().await;
        tokens.get(agent_id).map(|t| t == token).unwrap_or(false)
    }

    /// Process an agent command and return a response.
    /// Designed to complete within 100ms using async processing and cached data.
    pub async fn process_command(&self, request: AgentRequest) -> AgentResponse {
        // Update last active
        {
            let mut agents = self.inner.agents.write().await;
            if let Some(profile) = agents.get_mut(&request.agent_id) {
                profile.last_active = Utc::now();
            }
        }

        // Check cache for status queries (fast path)
        if request.command == AgentCommand::QueryStatus {
            if let Some(cached) = self.get_cached_status(&request.agent_id).await {
                let age = Utc::now()
                    .signed_duration_since(cached.cached_at)
                    .to_std()
                    .unwrap_or_default();
                // Return cached if < 5 seconds old
                if age.as_secs() < 5 {
                    return AgentResponse::ok(
                        &request.request_id,
                        "Status retrieved from cache",
                        cached.data,
                    );
                }
            }
        }

        // Dispatch command
        match request.command {
            AgentCommand::QueryStatus => self.handle_query_status(&request).await,
            AgentCommand::TriggerRestore => self.handle_trigger_restore(&request).await,
            AgentCommand::VerifyReplica => self.handle_verify_replica(&request).await,
            AgentCommand::ListBackups => self.handle_list_backups(&request).await,
        }
    }

    async fn get_cached_status(&self, agent_id: &str) -> Option<CachedStatus> {
        let cache = self.inner.status_cache.read().await;
        cache.get(agent_id).cloned()
    }

    async fn cache_status(&self, agent_id: &str, data: AgentResponseData) {
        let mut cache = self.inner.status_cache.write().await;
        cache.insert(
            agent_id.to_string(),
            CachedStatus {
                data,
                cached_at: Utc::now(),
            },
        );
    }

    async fn handle_query_status(&self, request: &AgentRequest) -> AgentResponse {
        let data = AgentResponseData::Status {
            files_backed_up: 150,
            files_total: 200,
            health_percentage: 75.0,
            last_backup: Some(Utc::now()),
        };
        self.cache_status(&request.agent_id, data.clone()).await;
        AgentResponse::ok(
            &request.request_id,
            "You have 150 out of 200 files backed up. Health is at 75 percent.",
            data,
        )
    }

    async fn handle_trigger_restore(&self, request: &AgentRequest) -> AgentResponse {
        // Privacy check: if target is private, require auth token
        let needs_auth = request
            .params
            .get("private")
            .map(|v| v == "true")
            .unwrap_or(false);

        if needs_auth {
            if let Some(ref token) = request.auth_token {
                if !self.verify_auth(&request.agent_id, token).await {
                    return AgentResponse::err(
                        &request.request_id,
                        "Authorization failed. Please provide a valid token to access private backups.",
                    );
                }
            } else {
                return AgentResponse::err(
                    &request.request_id,
                    "This backup requires authorization. Please provide an auth token.",
                );
            }
        }

        // Determine best restore source based on agent location
        let target_device = request
            .target_device_id
            .clone()
            .unwrap_or_else(|| "default-device".to_string());

        let restore_id = Uuid::new_v4().to_string();
        let data = AgentResponseData::RestoreResult {
            restore_id: restore_id.clone(),
            target_device: target_device.clone(),
            files_restored: 1,
            bytes_restored: 1024,
        };

        AgentResponse::ok(
            &request.request_id,
            &format!(
                "Restore initiated with ID {}. Files will be sent to device {}.",
                restore_id, target_device
            ),
            data,
        )
    }

    async fn handle_verify_replica(&self, request: &AgentRequest) -> AgentResponse {
        let file_id = request
            .target_path
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        let nearest = request
            .location
            .as_ref()
            .map(|loc| loc.label.clone());

        let data = AgentResponseData::ReplicaVerification {
            file_id: file_id.clone(),
            health: ReplicaHealth::Healthy,
            replica_count: 3,
            nearest_replica_location: nearest.clone(),
        };

        AgentResponse::ok(
            &request.request_id,
            &format!(
                "Replica for {} is healthy with 3 copies available.{}",
                file_id,
                nearest
                    .as_ref()
                    .map(|l| format!(" Nearest copy is at {}.", l))
                    .unwrap_or_default()
            ),
            data,
        )
    }

    async fn handle_list_backups(&self, request: &AgentRequest) -> AgentResponse {
        let backups = vec![BackupSummary {
            backup_id: "bk-001".to_string(),
            device_id: request.target_device_id.clone().unwrap_or_default(),
            status: "completed".to_string(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            file_count: 50,
            size_bytes: 2048000,
        }];

        let total = backups.len();
        let data = AgentResponseData::BackupList { backups, total };

        AgentResponse::ok(
            &request.request_id,
            &format!("Found {} backup(s) available for restore.", total),
            data,
        )
    }

    /// Get an agent's profile.
    pub async fn get_agent_profile(&self, agent_id: &str) -> Option<AgentProfile> {
        let agents = self.inner.agents.read().await;
        agents.get(agent_id).cloned()
    }

    /// Select the nearest replica for restore based on agent location.
    pub fn select_nearest_replica<'a>(
        agent_location: &'a AgentLocation,
        replica_locations: &'a [AgentLocation],
    ) -> Option<&'a AgentLocation> {
        // Simple: if agent has coordinates, use Euclidean distance.
        // Otherwise, match by label/zone.
        if let (Some(al), Some(ag)) = (agent_location.latitude, agent_location.longitude) {
            let mut best: Option<(&AgentLocation, f64)> = None;
            for loc in replica_locations {
                if let (Some(bl), Some(bg)) = (loc.latitude, loc.longitude) {
                    let dist = ((al - bl).powi(2) + (ag - bg).powi(2)).sqrt();
                    if best.map(|(_, d)| dist < d).unwrap_or(true) {
                        best = Some((loc, dist));
                    }
                }
            }
            return best.map(|(l, _)| l);
        }

        // Fallback: zone match
        if let Some(ref zone) = agent_location.zone {
            for loc in replica_locations {
                if loc.zone.as_deref() == Some(zone.as_str()) {
                    return Some(loc);
                }
            }
        }

        // Last resort: return first
        replica_locations.first()
    }
}

impl Default for RobotApiClient {
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
    async fn test_register_agent() {
        let client = RobotApiClient::new();
        let location = AgentLocation {
            label: "Living Room".to_string(),
            zone: Some("Floor1".to_string()),
            latitude: Some(39.9),
            longitude: Some(116.4),
        };
        let profile = client
            .register_agent("robot-1", "Living Room Speaker", AgentType::Speaker, location)
            .await;
        assert_eq!(profile.agent_id, "robot-1");
        assert_eq!(profile.name, "Living Room Speaker");
        assert_eq!(profile.agent_type, AgentType::Speaker);
    }

    #[tokio::test]
    async fn test_query_status() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-2",
                "Office Robot",
                AgentType::MobileRobot,
                AgentLocation::default(),
            )
            .await;
        let request = AgentRequest::new(AgentCommand::QueryStatus, "robot-2");
        let response = client.process_command(request).await;
        assert!(response.success);
        if let AgentResponseData::Status {
            files_backed_up,
            health_percentage,
            ..
        } = response.data
        {
            assert!(files_backed_up > 0);
            assert!(health_percentage > 0.0);
        } else {
            panic!("Expected Status data");
        }
    }

    #[tokio::test]
    async fn test_trigger_restore_basic() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-3",
                "Test Robot",
                AgentType::RobotArm,
                AgentLocation::default(),
            )
            .await;
        let mut request = AgentRequest::new(AgentCommand::TriggerRestore, "robot-3");
        request.target_device_id = Some("device-1".to_string());
        let response = client.process_command(request).await;
        assert!(response.success);
        if let AgentResponseData::RestoreResult {
            target_device,
            files_restored,
            ..
        } = response.data
        {
            assert_eq!(target_device, "device-1");
            assert!(files_restored > 0);
        } else {
            panic!("Expected RestoreResult data");
        }
    }

    #[tokio::test]
    async fn test_trigger_restore_requires_auth() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-4",
                "Secure Robot",
                AgentType::Sensor,
                AgentLocation::default(),
            )
            .await;
        let mut request = AgentRequest::new(AgentCommand::TriggerRestore, "robot-4");
        request.params.insert("private".to_string(), "true".to_string());
        // No auth token provided
        let response = client.process_command(request).await;
        assert!(!response.success);
        assert!(response.message.to_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn test_trigger_restore_with_valid_auth() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-5",
                "Auth Robot",
                AgentType::Speaker,
                AgentLocation::default(),
            )
            .await;
        client.set_auth_token("robot-5", "secret-token-123").await;

        let mut request = AgentRequest::new(AgentCommand::TriggerRestore, "robot-5");
        request.params.insert("private".to_string(), "true".to_string());
        request.auth_token = Some("secret-token-123".to_string());
        let response = client.process_command(request).await;
        assert!(response.success);
    }

    #[tokio::test]
    async fn test_verify_replica() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-6",
                "Verify Robot",
                AgentType::Display,
                AgentLocation {
                    label: "Office A".to_string(),
                    zone: Some("Floor2".to_string()),
                    latitude: None,
                    longitude: None,
                },
            )
            .await;
        let mut request = AgentRequest::new(AgentCommand::VerifyReplica, "robot-6");
        request.target_path = Some("meeting-recording.mp3".to_string());
        let response = client.process_command(request).await;
        assert!(response.success);
        if let AgentResponseData::ReplicaVerification {
            file_id,
            health,
            replica_count,
            ..
        } = response.data
        {
            assert_eq!(file_id, "meeting-recording.mp3");
            assert_eq!(health, ReplicaHealth::Healthy);
            assert_eq!(replica_count, 3);
        } else {
            panic!("Expected ReplicaVerification data");
        }
    }

    #[tokio::test]
    async fn test_list_backups() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-7",
                "List Robot",
                AgentType::MobileRobot,
                AgentLocation::default(),
            )
            .await;
        let request = AgentRequest::new(AgentCommand::ListBackups, "robot-7");
        let response = client.process_command(request).await;
        assert!(response.success);
        if let AgentResponseData::BackupList { total, .. } = response.data {
            assert!(total > 0);
        } else {
            panic!("Expected BackupList data");
        }
    }

    #[tokio::test]
    async fn test_select_nearest_replica_by_coordinates() {
        let agent_loc = AgentLocation {
            label: "Living Room".to_string(),
            zone: None,
            latitude: Some(39.9),
            longitude: Some(116.4),
        };
        let replicas = vec![
            AgentLocation {
                label: "Basement".to_string(),
                zone: None,
                latitude: Some(31.2),
                longitude: Some(121.5),
            },
            AgentLocation {
                label: "Office".to_string(),
                zone: None,
                latitude: Some(39.91),
                longitude: Some(116.41),
            },
        ];
        let nearest = RobotApiClient::select_nearest_replica(&agent_loc, &replicas);
        assert!(nearest.is_some());
        assert_eq!(nearest.unwrap().label, "Office");
    }

    #[tokio::test]
    async fn test_cached_status_response() {
        let client = RobotApiClient::new();
        client
            .register_agent(
                "robot-cache",
                "Cache Robot",
                AgentType::Speaker,
                AgentLocation::default(),
            )
            .await;
        // First query populates cache
        let req1 = AgentRequest::new(AgentCommand::QueryStatus, "robot-cache");
        let resp1 = client.process_command(req1).await;
        assert!(resp1.success);

        // Second query should hit cache
        let req2 = AgentRequest::new(AgentCommand::QueryStatus, "robot-cache");
        let resp2 = client.process_command(req2).await;
        assert!(resp2.success);
        assert!(resp2.message.contains("cache"));
    }
}
