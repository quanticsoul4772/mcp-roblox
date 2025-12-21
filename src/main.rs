mod bridge;
mod cloud;
mod config;
mod error;
mod http;
mod mcp;
mod metrics;
mod tools;
mod watcher;

use std::sync::Arc;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing::{error, info};

use crate::bridge::http::{create_router, PluginBridge};
use crate::config::ServerConfig;
use crate::mcp::RobloxMcpServer;
use crate::metrics::ServerMetrics;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse configuration from environment (testable logic in config module)
    let config = ServerConfig::from_env().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Extract values before consuming env_filter (which doesn't implement Copy)
    let bind_addr = config.bind_addr();
    let project_root = config.project_root;

    // Initialize tracing to STDERR (stdout reserved for MCP JSON-RPC)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(config.env_filter)
        .init();

    info!("Roblox Studio MCP Server starting...");
    info!("Project root: {}", project_root.display());

    // Create shared metrics for cross-component tracking
    // This enables late result tracking when plugin returns data after caller has timed out
    let metrics = Arc::new(ServerMetrics::new());

    // Create shared plugin bridge with metrics for late result tracking
    let bridge = Arc::new(PluginBridge::with_metrics(metrics.clone()));

    // Spawn HTTP bridge as background task (for plugin communication)
    // Graceful degradation: if port binding fails, MCP server continues without plugin support
    // Note: We clone PluginBridge here (not Arc) because create_router takes PluginBridge by value.
    // PluginBridge is cheap to clone (just Arc pointers internally).
    let http_bridge = (*bridge).clone();

    tokio::spawn(async move {
        let app = create_router(http_bridge);

        // Try to bind to configured port, with graceful fallback on failure
        let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => listener,
            Err(e) => {
                error!(
                    "Failed to bind HTTP bridge to {}: {}. \
                     Studio plugin communication will be unavailable. \
                     Ensure the port is not in use, or set ROBLOX_MCP_PORT to a different port.",
                    bind_addr, e
                );
                return; // Exit this task, but don't crash the main server
            }
        };

        info!("HTTP bridge listening on {}", bind_addr);

        // Serve HTTP requests, logging any errors without crashing
        if let Err(e) = axum::serve(listener, app).await {
            error!(
                "HTTP bridge server error: {}. Plugin communication has stopped.",
                e
            );
        }
    });

    // Create MCP server with shared metrics for unified tracking
    let server = RobloxMcpServer::new(bridge, project_root).with_shared_metrics(metrics);

    // Run MCP server on STDIO (blocks main thread)
    info!("Starting MCP server on STDIO");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
