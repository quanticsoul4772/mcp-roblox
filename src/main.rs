mod error;
mod tools {
    pub mod filesystem;
}
mod bridge {
    pub mod http;
}

use anyhow::Result;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::bridge::http::{create_router, PluginBridge};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to STDERR (CRITICAL: stdout is reserved for MCP JSON-RPC protocol)
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "roblox_studio_mcp=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    info!("Roblox Studio MCP Server starting...");

    // Create plugin bridge
    let bridge = PluginBridge::new();
    
    // Create HTTP server for plugin communication (LOCALHOST ONLY)
    let app = create_router(bridge.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    
    info!("Starting HTTP bridge on {}", addr);
    
    // Start HTTP server
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind to localhost:8080");
    
    info!("HTTP bridge listening on {}", addr);
    info!("Waiting for Studio plugin to connect...");
    
    // Run the server
    axum::serve(listener, app)
        .await
        .expect("Server error");
    
    Ok(())
}
