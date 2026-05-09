//! OpenVault Transport Module
//!
//! This module provides transport integration with OpenLink for remote backup management.
//! It includes storage routing, transfer routing, and OpenLink API integration.

pub mod config;
pub mod error;
pub mod router;
pub mod transport;

pub use config::{OpenLinkConfig, TransferConfig, StorageBackend};
pub use error::TransportError;
pub use router::{StorageRouter, TransferRouter, RouteDecision};
pub use transport::{OpenLinkTransport, Transport, TransferStats};

use anyhow::Result;

/// Result type for transport operations
pub type TransportResult<T> = Result<T, TransportError>;
