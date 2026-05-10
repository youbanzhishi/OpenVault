//! OpenVault Server
//!
//! HTTP API server for remote backup management via OpenLink.

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod auth;
mod error;
mod handlers;
mod models;
mod services;

use handlers::AppState;

/// Command line arguments for OpenVault server
#[derive(Parser, Debug)]
#[command(name = "openvault-server")]
#[command(about = "OpenVault HTTP API server for remote backup management")]
struct Args {
    /// Server bind address
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    bind: String,

    /// JWT secret for authentication
    #[arg(short, long)]
    jwt_secret: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// OpenLink endpoint URL
    #[arg(long)]
    openlink_endpoint: Option<String>,

    /// Server name for identification
    #[arg(long, default_value = "openvault")]
    server_name: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("openvault_server={}", log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting OpenVault Server v{}", env!("CARGO_PKG_VERSION"));

    // Get JWT secret from args or environment
    let jwt_secret = args
        .jwt_secret
        .clone()
        .or_else(|| std::env::var("OPENVault_JWT_SECRET").ok())
        .unwrap_or_else(|| {
            // Generate a random secret for development
            let mut secret = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret);
            hex::encode(secret)
        });

    if args.jwt_secret.is_none() {
        tracing::warn!(
            "Using auto-generated JWT secret. Set --jwt-secret or OPENVault_JWT_SECRET for production."
        );
    }

    // Create application state
    let state = Arc::new(AppState::new(jwt_secret));

    // Create router
    let app = api::create_router(state);

    // Parse bind address
    let addr: SocketAddr = args.bind.parse().expect("Invalid bind address");

    info!("Server listening on {}", addr);
    info!("API endpoints available at http://{}/api/v1/", addr);

    // Run the server using axum 0.6's Server API
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
