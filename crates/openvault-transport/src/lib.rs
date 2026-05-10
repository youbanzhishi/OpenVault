//! OpenVault Transport Module
//!
//! This module provides transport integration with OpenLink for remote backup management.
//! It includes storage routing, transfer routing, and OpenLink API integration.

pub mod config;
pub mod error;
pub mod router;
pub mod transport;

pub use config::{OpenLinkConfig, StorageBackend, TransferConfig};
pub use error::TransportError;
pub use router::{RouteDecision, StorageRouter, TransferRouter};
pub use transport::{OpenLinkTransport, TransferStats, Transport};

use anyhow::Result;

/// Result type for transport operations
pub type TransportResult<T> = Result<T, TransportError>;
