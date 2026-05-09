//! OpenLink transport configuration

use serde::{Deserialize, Serialize};

/// Configuration for OpenLink transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenLinkConfig {
    /// OpenLink API endpoint URL
    pub endpoint: String,

    /// Authentication token for OpenLink API
    pub token: Option<String>,

    /// API key (alternative to token)
    pub api_key: Option<String>,

    /// Device ID for this instance
    pub device_id: String,

    /// Device name for identification
    pub device_name: String,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Retry attempts for failed transfers
    #[serde(default = "default_retries")]
    pub max_retries: u32,

    /// Chunk size for file uploads (bytes)
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u64,

    /// Enable compression for transfers
    #[serde(default = "default_compression")]
    pub compression: bool,

    /// Storage backend configuration
    pub storage: StorageBackend,
}

fn default_timeout() -> u64 { 30 }
fn default_retries() -> u32 { 3 }
fn default_chunk_size() -> u64 { 1024 * 1024 * 10 } // 10MB
fn default_compression() -> bool { true }

impl OpenLinkConfig {
    /// Get device name
    pub fn get_device_name() -> String {
        hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    }

    /// Get auth header value
    pub fn auth_header(&self) -> Option<String> {
        self.token
            .clone()
            .map(|t| format!("Bearer {}", t))
            .or_else(|| self.api_key.clone().map(|k| format!("ApiKey {}", k)))
    }
}

impl Default for OpenLinkConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8080".to_string(),
            token: None,
            api_key: None,
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: Self::get_device_name(),
            timeout_secs: default_timeout(),
            max_retries: default_retries(),
            chunk_size: default_chunk_size(),
            compression: default_compression(),
            storage: StorageBackend::default(),
        }
    }
}

/// Storage backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBackend {
    /// Primary storage type
    #[serde(default)]
    pub primary: StorageType,

    /// Backup storage type
    #[serde(default)]
    pub backup: Option<Box<StorageBackend>>,

    /// Storage region (for geo-distributed storage)
    #[serde(default)]
    pub region: Option<String>,

    /// Storage class (e.g., "hot", "cold", "archive")
    #[serde(default)]
    pub storage_class: Option<String>,
}

impl Default for StorageBackend {
    fn default() -> Self {
        Self {
            primary: StorageType::Local,
            backup: None,
            region: None,
            storage_class: None,
        }
    }
}

/// Supported storage types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageType {
    /// Local filesystem storage
    Local,
    /// S3-compatible object storage
    S3,
    /// OpenLink managed storage
    OpenLink,
    /// Distributed storage via OpenLink
    Distributed,
}

impl Default for StorageType {
    fn default() -> Self {
        StorageType::Local
    }
}

impl std::fmt::Display for StorageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageType::Local => write!(f, "local"),
            StorageType::S3 => write!(f, "s3"),
            StorageType::OpenLink => write!(f, "openlink"),
            StorageType::Distributed => write!(f, "distributed"),
        }
    }
}

/// Transfer configuration for optimal routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferConfig {
    /// Prefer direct transfer over cloud relay
    #[serde(default = "default_prefer_direct")]
    pub prefer_direct: bool,

    /// Maximum concurrent transfers
    #[serde(default = "default_concurrent")]
    pub max_concurrent: usize,

    /// Bandwidth limit (bytes per second), 0 = unlimited
    #[serde(default)]
    pub bandwidth_limit: u64,

    /// Enable adaptive routing based on network conditions
    #[serde(default = "default_adaptive")]
    pub adaptive_routing: bool,
}

fn default_prefer_direct() -> bool { true }
fn default_concurrent() -> usize { 4 }
fn default_adaptive() -> bool { true }

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            prefer_direct: default_prefer_direct(),
            max_concurrent: default_concurrent(),
            bandwidth_limit: 0,
            adaptive_routing: default_adaptive(),
        }
    }
}

impl OpenLinkConfig {
    /// Load configuration from file
    pub fn load_from_file(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Load configuration from string
    pub fn load_from_str(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Save configuration to file
    pub fn save_to_file(&self, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
