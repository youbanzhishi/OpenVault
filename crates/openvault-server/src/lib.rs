//! OpenVault Server Library
//!
//! HTTP API server for remote backup management via OpenLink.

pub mod api;
pub mod auth;
pub mod error;
pub mod handlers;
pub mod models;
pub mod services;

pub use error::ServerError;
pub use handlers::AppState;
