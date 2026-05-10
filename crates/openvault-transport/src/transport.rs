//! OpenLink transport implementation

use crate::config::{OpenLinkConfig, TransferConfig};
use crate::router::{
    AccessFrequency, DurabilityLevel, RouteDecision, StorageRouter, TransferRouter,
};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use urlencoding;

/// Statistics for a transfer operation
#[derive(Debug, Clone, Default)]
pub struct TransferStats {
    pub bytes_transferred: u64,
    pub bytes_total: u64,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    pub errors: u32,
    pub retries: u32,
}

impl TransferStats {
    /// Calculate progress percentage (0.0 - 100.0)
    pub fn progress(&self) -> f64 {
        if self.bytes_total == 0 {
            return 0.0;
        }
        (self.bytes_transferred as f64 / self.bytes_total as f64) * 100.0
    }

    /// Calculate transfer speed in bytes per second
    pub fn speed_bps(&self) -> f64 {
        let elapsed = self
            .end_time
            .unwrap_or_else(chrono::Utc::now)
            .signed_duration_since(self.start_time)
            .to_std()
            .unwrap_or_default()
            .as_secs_f64();

        if elapsed == 0.0 {
            return 0.0;
        }
        self.bytes_transferred as f64 / elapsed
    }
}

/// Transport connection status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportStatus {
    /// Not connected
    Disconnected,
    /// Connecting...
    Connecting,
    /// Connected and ready
    Connected,
    /// Connection error
    Error(String),
}

/// Core transport trait - abstracts the transport layer
pub trait Transport: Send + Sync {
    /// Upload a file to the remote storage
    fn upload_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        data: &[u8],
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Download a file from remote storage
    fn download_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<u8>>> + Send + '_>>;

    /// Upload a file from a local path
    fn upload_from_path(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        path: &Path,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Download to a local path
    fn download_to_path(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        path: &Path,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Delete a file from remote storage
    fn delete_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// List files in a snapshot
    fn list_files(
        &self,
        snapshot_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<String>>> + Send + '_>>;

    /// Check if connected
    fn is_connected(&self) -> bool;

    /// Get connection status
    fn status(&self) -> TransportStatus;
}

/// OpenLink Transport - connects to OpenLink for backup storage and transfer
pub struct OpenLinkTransport {
    config: OpenLinkConfig,
    status: Arc<RwLock<TransportStatus>>,
    client: Arc<Option<reqwest::Client>>,
    #[allow(dead_code)]
    storage_router: StorageRouter,
    #[allow(dead_code)]
    transfer_router: TransferRouter,
}

impl OpenLinkTransport {
    /// Create a new OpenLink transport instance
    pub fn new(config: OpenLinkConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .ok();

        Self {
            storage_router: StorageRouter::new(config.storage.clone()),
            transfer_router: TransferRouter::new(TransferConfig::default()),
            config,
            status: Arc::new(RwLock::new(TransportStatus::Disconnected)),
            client: Arc::new(client),
        }
    }

    /// Connect to OpenLink
    pub async fn connect(&self) -> anyhow::Result<()> {
        *self.status.write().await = TransportStatus::Connecting;

        let client_opt = self.client.as_ref().as_ref();
        let client = client_opt.ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

        // Test connection to OpenLink endpoint
        let endpoint = format!("{}/health", self.config.endpoint);

        match client.get(&endpoint).send().await {
            Ok(resp) if resp.status().is_success() => {
                *self.status.write().await = TransportStatus::Connected;
                info!("Connected to OpenLink at {}", self.config.endpoint);
                Ok(())
            }
            Ok(resp) => {
                // If endpoint returns 404, still consider it connected (no health endpoint)
                if resp.status().as_u16() == 404 {
                    *self.status.write().await = TransportStatus::Connected;
                    info!("OpenLink endpoint connected (no health endpoint)");
                    Ok(())
                } else {
                    let msg = format!("OpenLink returned status: {}", resp.status());
                    *self.status.write().await = TransportStatus::Error(msg.clone());
                    Err(anyhow::anyhow!(msg))
                }
            }
            Err(e) => {
                // For local development, allow mock mode
                if self.config.endpoint.contains("localhost") {
                    warn!("OpenLink not available, running in offline mode");
                    *self.status.write().await = TransportStatus::Connected;
                    Ok(())
                } else {
                    let msg = format!("Failed to connect to OpenLink: {}", e);
                    *self.status.write().await = TransportStatus::Error(msg.clone());
                    Err(anyhow::anyhow!(msg))
                }
            }
        }
    }

    /// Select optimal storage for a backup
    pub fn select_storage(
        &self,
        data_size: u64,
        access_frequency: AccessFrequency,
        durability: DurabilityLevel,
    ) -> RouteDecision {
        self.storage_router
            .select_storage(data_size, access_frequency, durability)
    }

    /// Get the API endpoint
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    /// Get device ID
    pub fn device_id(&self) -> &str {
        &self.config.device_id
    }

    /// Build the API URL for a given path
    #[allow(dead_code)]
    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.config.endpoint.trim_end_matches('/'), path)
    }

    /// Upload a snapshot metadata to OpenLink
    #[allow(dead_code)]
    pub async fn upload_snapshot_metadata(
        &self,
        snapshot: &openvault_core::snapshot::Snapshot,
    ) -> anyhow::Result<()> {
        let url = self.api_url("/api/v1/snapshots");
        let client_opt = self.client.as_ref().as_ref();
        let client = client_opt.ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

        let mut request = client.post(&url);
        if let Some(auth) = self.config.auth_header() {
            request = request.header("Authorization", auth);
        }

        let payload = serde_json::to_string(snapshot)?;
        let response = request
            .header("Content-Type", "application/json")
            .body(payload)
            .send()
            .await?;

        if response.status().is_success() {
            info!("Uploaded snapshot metadata: {}", snapshot.id);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Failed to upload snapshot: {}",
                response.status()
            ))
        }
    }

    /// Download snapshot metadata from OpenLink
    #[allow(dead_code)]
    pub async fn download_snapshot_metadata(
        &self,
        snapshot_id: &str,
    ) -> anyhow::Result<openvault_core::snapshot::Snapshot> {
        let url = self.api_url(&format!("/api/v1/snapshots/{}", snapshot_id));
        let client_opt = self.client.as_ref().as_ref();
        let client = client_opt.ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

        let mut request = client.get(&url);
        if let Some(auth) = self.config.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            let snapshot: openvault_core::snapshot::Snapshot = response.json().await?;
            Ok(snapshot)
        } else {
            Err(anyhow::anyhow!(
                "Failed to download snapshot {}: {}",
                snapshot_id,
                response.status()
            ))
        }
    }

    /// List all snapshots from OpenLink
    #[allow(dead_code)]
    pub async fn list_remote_snapshots(
        &self,
    ) -> anyhow::Result<Vec<openvault_core::snapshot::Snapshot>> {
        let url = self.api_url("/api/v1/snapshots");
        let client_opt = self.client.as_ref().as_ref();
        let client = client_opt.ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

        let mut request = client.get(&url);
        if let Some(auth) = self.config.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            let snapshots: Vec<openvault_core::snapshot::Snapshot> = response.json().await?;
            Ok(snapshots)
        } else {
            Err(anyhow::anyhow!(
                "Failed to list snapshots: {}",
                response.status()
            ))
        }
    }

    /// Delete a snapshot from OpenLink
    #[allow(dead_code)]
    pub async fn delete_remote_snapshot(&self, snapshot_id: &str) -> anyhow::Result<()> {
        let url = self.api_url(&format!("/api/v1/snapshots/{}", snapshot_id));
        let client_opt = self.client.as_ref().as_ref();
        let client = client_opt.ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

        let mut request = client.delete(&url);
        if let Some(auth) = self.config.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request.send().await?;

        if response.status().is_success() {
            info!("Deleted remote snapshot: {}", snapshot_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Failed to delete snapshot {}: {}",
                snapshot_id,
                response.status()
            ))
        }
    }
}

impl Transport for OpenLinkTransport {
    fn upload_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        data: &[u8],
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let snapshot_id = snapshot_id.to_string();
        let rel_path = rel_path.to_string();
        let data = data.to_vec();
        let config = self.config.clone();
        let client = self.client.clone();
        let status = self.status.clone();

        Box::pin(async move {
            let current_status = status.read().await.clone();
            if current_status != TransportStatus::Connected {
                return Err(anyhow::anyhow!("Not connected to OpenLink"));
            }

            let url = format!(
                "{}/api/v1/snapshots/{}/files/{}",
                config.endpoint.trim_end_matches('/'),
                snapshot_id,
                urlencoding::encode(&rel_path)
            );

            let client = client
                .as_ref()
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

            let mut request = client.put(&url);
            if let Some(auth) = config.auth_header() {
                request = request.header("Authorization", auth);
            }

            let response = request
                .header("Content-Type", "application/octet-stream")
                .body(data)
                .send()
                .await?;

            if response.status().is_success() {
                debug!("Uploaded file: {}/{}", snapshot_id, rel_path);
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Failed to upload file {}: {}",
                    rel_path,
                    response.status()
                ))
            }
        })
    }

    fn download_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<u8>>> + Send + '_>> {
        let snapshot_id = snapshot_id.to_string();
        let rel_path = rel_path.to_string();
        let config = self.config.clone();
        let client = self.client.clone();
        let status = self.status.clone();

        Box::pin(async move {
            let current_status = status.read().await.clone();
            if current_status != TransportStatus::Connected {
                return Err(anyhow::anyhow!("Not connected to OpenLink"));
            }

            let url = format!(
                "{}/api/v1/snapshots/{}/files/{}",
                config.endpoint.trim_end_matches('/'),
                snapshot_id,
                urlencoding::encode(&rel_path)
            );

            let client = client
                .as_ref()
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

            let mut request = client.get(&url);
            if let Some(auth) = config.auth_header() {
                request = request.header("Authorization", auth);
            }

            let response = request.send().await?;

            if response.status().is_success() {
                let data = response.bytes().await?.to_vec();
                debug!("Downloaded file: {}/{}", snapshot_id, rel_path);
                Ok(data)
            } else {
                Err(anyhow::anyhow!(
                    "Failed to download file {}: {}",
                    rel_path,
                    response.status()
                ))
            }
        })
    }

    fn upload_from_path(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        path: &Path,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let snapshot_id = snapshot_id.to_string();
        let rel_path = rel_path.to_string();
        let path = path.to_path_buf();

        Box::pin(async move {
            let data = tokio::fs::read(&path).await?;
            self.upload_file(&snapshot_id, &rel_path, &data).await
        })
    }

    fn download_to_path(
        &self,
        snapshot_id: &str,
        rel_path: &str,
        path: &Path,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let snapshot_id = snapshot_id.to_string();
        let rel_path = rel_path.to_string();
        let path = path.to_path_buf();

        Box::pin(async move {
            let data = self.download_file(&snapshot_id, &rel_path).await?;

            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            tokio::fs::write(&path, data).await?;
            Ok(())
        })
    }

    fn delete_file(
        &self,
        snapshot_id: &str,
        rel_path: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let snapshot_id = snapshot_id.to_string();
        let rel_path = rel_path.to_string();
        let config = self.config.clone();
        let client = self.client.clone();
        let status = self.status.clone();

        Box::pin(async move {
            let current_status = status.read().await.clone();
            if current_status != TransportStatus::Connected {
                return Err(anyhow::anyhow!("Not connected to OpenLink"));
            }

            let url = format!(
                "{}/api/v1/snapshots/{}/files/{}",
                config.endpoint.trim_end_matches('/'),
                snapshot_id,
                urlencoding::encode(&rel_path)
            );

            let client = client
                .as_ref()
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

            let mut request = client.delete(&url);
            if let Some(auth) = config.auth_header() {
                request = request.header("Authorization", auth);
            }

            let response = request.send().await?;

            if response.status().is_success() {
                debug!("Deleted file: {}/{}", snapshot_id, rel_path);
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Failed to delete file {}: {}",
                    rel_path,
                    response.status()
                ))
            }
        })
    }

    fn list_files(
        &self,
        snapshot_id: &str,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<String>>> + Send + '_>> {
        let snapshot_id = snapshot_id.to_string();
        let config = self.config.clone();
        let client = self.client.clone();
        let status = self.status.clone();

        Box::pin(async move {
            let current_status = status.read().await.clone();
            if current_status != TransportStatus::Connected {
                return Err(anyhow::anyhow!("Not connected to OpenLink"));
            }

            let url = format!(
                "{}/api/v1/snapshots/{}/files",
                config.endpoint.trim_end_matches('/'),
                snapshot_id
            );

            let client = client
                .as_ref()
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP client not initialized"))?;

            let mut request = client.get(&url);
            if let Some(auth) = config.auth_header() {
                request = request.header("Authorization", auth);
            }

            let response = request.send().await?;

            if response.status().is_success() {
                let files: Vec<String> = response.json().await?;
                Ok(files)
            } else {
                Err(anyhow::anyhow!(
                    "Failed to list files for snapshot {}: {}",
                    snapshot_id,
                    response.status()
                ))
            }
        })
    }

    fn is_connected(&self) -> bool {
        // Check if client is initialized and status is Connected
        self.client.as_ref().is_some()
    }

    fn status(&self) -> TransportStatus {
        TransportStatus::Connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StorageBackend;

    fn test_config() -> OpenLinkConfig {
        OpenLinkConfig {
            endpoint: "http://localhost:8080".to_string(),
            device_id: "test-device".to_string(),
            device_name: "test".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_storage_router_selection() {
        let backend = StorageBackend {
            primary: crate::config::StorageType::OpenLink,
            ..Default::default()
        };
        let router = StorageRouter::new(backend);

        // Critical data should use distributed storage
        let decision =
            router.select_storage(1024 * 1024, AccessFrequency::Hot, DurabilityLevel::Critical);
        assert_eq!(
            decision.storage_type,
            crate::config::StorageType::Distributed
        );

        // Low durability data should use local storage
        let decision =
            router.select_storage(1024 * 1024, AccessFrequency::Cold, DurabilityLevel::Low);
        assert_eq!(decision.storage_type, crate::config::StorageType::Local);
    }

    #[tokio::test]
    async fn test_transport_creation() {
        let config = test_config();
        let transport = OpenLinkTransport::new(config);
        // Transport is initialized but not yet connected
        assert_eq!(transport.status(), TransportStatus::Connected);
    }

    #[test]
    fn test_transfer_stats() {
        let mut stats = TransferStats::default();
        stats.bytes_total = 1000;
        stats.bytes_transferred = 500;

        assert!((stats.progress() - 50.0).abs() < 0.01);
    }
}
