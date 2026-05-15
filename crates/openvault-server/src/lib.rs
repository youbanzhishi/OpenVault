//! OpenVault Server Library
//!
//! HTTP API server for remote backup management via OpenLink.
//!
//! # Phase 9 Modules
//!
//! - **agent_api**: Physical agent/robot API for voice queries, restores, replica verification
//! - **web_api**: Web management panel backend API with dashboard, charts, WebSocket push
//! - **device_manager**: Multi-device registry, sync coordination, device-policy mapping
//! - **static_files**: Embedded static file serving with SPA routing

pub mod agent_api;
pub mod api;
pub mod auth;
pub mod device_manager;
pub mod error;
pub mod handlers;
pub mod models;
pub mod services;
pub mod static_files;
pub mod web_api;
pub mod web_ui;

pub use error::ServerError;
pub use handlers::AppState;
