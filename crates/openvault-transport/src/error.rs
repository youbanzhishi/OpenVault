//! Transport error types

use thiserror::Error;

/// Errors that can occur during transport operations
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Storage backend error: {0}")]
    Storage(String),

    #[error("Transfer failed: {0}")]
    Transfer(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Not connected: {0}")]
    NotConnected(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl From<reqwest::Error> for TransportError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            TransportError::Timeout(e.to_string())
        } else if e.is_connect() {
            TransportError::Network(format!("Connection failed: {}", e))
        } else if e.status().map(|s| s.as_u16() == 401).unwrap_or(false) {
            TransportError::Auth("Invalid or expired token".to_string())
        } else {
            TransportError::Network(e.to_string())
        }
    }
}
