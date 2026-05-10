//! Storage and Transfer routing for optimal backup path selection

use crate::config::{StorageBackend, StorageType, TransferConfig};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Result of a routing decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDecision {
    /// The recommended storage type
    pub storage_type: StorageType,
    /// Whether to use direct transfer
    pub use_direct: bool,
    /// Estimated latency in milliseconds
    pub estimated_latency_ms: u64,
    /// Estimated bandwidth in bytes per second
    pub estimated_bandwidth_bps: u64,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Reason for the decision
    pub reason: String,
}

/// Storage Router - selects optimal storage backend based on various factors
pub struct StorageRouter {
    config: StorageBackend,
}

impl StorageRouter {
    /// Create a new storage router with the given configuration
    pub fn new(config: StorageBackend) -> Self {
        Self { config }
    }

    /// Select the optimal storage backend based on data characteristics
    pub fn select_storage(
        &self,
        data_size: u64,
        _access_frequency: AccessFrequency,
        required_durability: DurabilityLevel,
    ) -> RouteDecision {
        let storage_type = match required_durability {
            DurabilityLevel::Critical => {
                // Critical data: use distributed storage for maximum durability
                StorageType::Distributed
            }
            DurabilityLevel::High => {
                // High durability: prefer OpenLink managed storage
                if self.config.primary == StorageType::Local && self.config.backup.is_some() {
                    // Has backup configured
                    StorageType::OpenLink
                } else {
                    StorageType::OpenLink
                }
            }
            DurabilityLevel::Standard => {
                // Standard: use primary configured storage
                self.config.primary.clone()
            }
            DurabilityLevel::Low => {
                // Low durability: use local storage for speed
                StorageType::Local
            }
        };

        let (estimated_latency_ms, confidence) = match storage_type {
            StorageType::Local => (5, 0.95),
            StorageType::S3 => (50, 0.85),
            StorageType::OpenLink => (30, 0.90),
            StorageType::Distributed => (100, 0.75),
        };

        let estimated_bandwidth_bps = match storage_type {
            StorageType::Local => 100 * 1024 * 1024 * 1024, // 100 GB/s (local disk)
            StorageType::S3 => 100 * 1024 * 1024,           // 100 MB/s
            StorageType::OpenLink => 50 * 1024 * 1024,      // 50 MB/s
            StorageType::Distributed => 200 * 1024 * 1024,  // 200 MB/s (parallel)
        };

        let use_direct = storage_type == StorageType::Local;

        let reason = format!(
            "Selected {} storage for {} bytes with {:?} durability requirement",
            storage_type, data_size, required_durability
        );

        RouteDecision {
            storage_type,
            use_direct,
            estimated_latency_ms,
            estimated_bandwidth_bps,
            confidence,
            reason,
        }
    }

    /// Get the primary storage backend
    pub fn primary_storage(&self) -> &StorageType {
        &self.config.primary
    }

    /// Check if backup storage is configured
    pub fn has_backup(&self) -> bool {
        self.config.backup.is_some()
    }
}

/// Transfer Router - selects optimal transfer path
pub struct TransferRouter {
    config: TransferConfig,
}

impl TransferRouter {
    /// Create a new transfer router
    pub fn new(config: TransferConfig) -> Self {
        Self { config }
    }

    /// Decide the optimal transfer path based on network conditions
    pub fn select_path(
        &self,
        source_region: &str,
        target_region: &str,
        network_quality: NetworkQuality,
        data_size: u64,
    ) -> RouteDecision {
        let use_direct = if self.config.prefer_direct {
            // Check if direct transfer is feasible
            let same_region = source_region == target_region;
            let good_network = matches!(
                network_quality,
                NetworkQuality::Excellent | NetworkQuality::Good
            );

            same_region || (good_network && data_size < 1024 * 1024 * 1024) // < 1GB
        } else {
            false
        };

        let (estimated_latency_ms, estimated_bandwidth_bps, confidence) = if use_direct {
            // Direct transfer: low latency, high bandwidth
            let latency = match network_quality {
                NetworkQuality::Excellent => 10,
                NetworkQuality::Good => 30,
                NetworkQuality::Fair => 100,
                NetworkQuality::Poor => 300,
            };
            let bandwidth = match network_quality {
                NetworkQuality::Excellent => 1000 * 1024 * 1024, // 1 GB/s
                NetworkQuality::Good => 100 * 1024 * 1024,       // 100 MB/s
                NetworkQuality::Fair => 10 * 1024 * 1024,        // 10 MB/s
                NetworkQuality::Poor => 1024 * 1024,             // 1 MB/s
            };
            (latency, bandwidth, 0.9)
        } else {
            // Cloud relay: higher latency, consistent bandwidth
            let latency = 200; // Cloud relay overhead
            let bandwidth = 50 * 1024 * 1024; // Consistent 50 MB/s via relay
            (latency, bandwidth, 0.85)
        };

        let reason = if use_direct {
            format!(
                "Direct transfer selected: {}ms latency, {:?} network, {} bytes",
                estimated_latency_ms, network_quality, data_size
            )
        } else {
            format!(
                "Cloud relay selected: higher reliability for {} bytes transfer",
                data_size
            )
        };

        RouteDecision {
            storage_type: if use_direct {
                StorageType::Local
            } else {
                StorageType::OpenLink
            },
            use_direct,
            estimated_latency_ms,
            estimated_bandwidth_bps,
            confidence,
            reason,
        }
    }

    /// Estimate transfer time based on data size and path
    pub fn estimate_transfer_time(&self, data_size: u64, route: &RouteDecision) -> Duration {
        if route.estimated_bandwidth_bps == 0 {
            return Duration::from_secs(u64::MAX);
        }

        let bytes_per_sec = route
            .estimated_bandwidth_bps
            .min(self.config.bandwidth_limit);

        let secs = data_size as f64 / bytes_per_sec as f64;
        Duration::from_secs_f64(secs)
    }

    /// Get maximum concurrent transfers allowed
    pub fn max_concurrent(&self) -> usize {
        self.config.max_concurrent
    }
}

/// Data access frequency hint
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessFrequency {
    /// Data accessed very frequently (multiple times per hour)
    Hot,
    /// Data accessed regularly (daily)
    #[default]
    Warm,
    /// Data accessed occasionally (monthly)
    Cold,
    /// Data rarely accessed (yearly or less)
    Archive,
}

/// Required durability level
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurabilityLevel {
    /// Maximum durability (multiple redundant copies)
    Critical,
    /// High durability (geographically distributed)
    High,
    /// Standard durability (single redundant copy)
    #[default]
    Standard,
    /// Low durability (single copy)
    Low,
}

impl std::fmt::Display for DurabilityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DurabilityLevel::Critical => write!(f, "critical"),
            DurabilityLevel::High => write!(f, "high"),
            DurabilityLevel::Standard => write!(f, "standard"),
            DurabilityLevel::Low => write!(f, "low"),
        }
    }
}

/// Network quality assessment
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkQuality {
    /// Excellent network conditions
    Excellent,
    /// Good network conditions
    #[default]
    Good,
    /// Fair network conditions
    Fair,
    /// Poor network conditions
    Poor,
}
