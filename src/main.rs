mod bridge;
mod cloud;
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
use crate::mcp::RobloxMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    // CRITICAL: Initialize logging to STDERR (stdout is reserved for MCP JSON-RPC protocol)
    // NO SILENT FALLBACK: If RUST_LOG is set but invalid, we FAIL instead of hiding the error
    let env_filter = match std::env::var("RUST_LOG") {
        Ok(filter_str) => {
            // RUST_LOG is set - parse it and FAIL if invalid
            tracing_subscriber::EnvFilter::try_new(&filter_str).map_err(|e| {
                anyhow::anyhow!(
                    "Invalid RUST_LOG environment variable '{}': {}",
                    filter_str,
                    e
                )
            })?
        }
        Err(std::env::VarError::NotPresent) => {
            // RUST_LOG not set - use sensible default
            tracing_subscriber::EnvFilter::new("roblox_studio_mcp=info,tower_http=debug")
        }
        Err(std::env::VarError::NotUnicode(os_str)) => {
            // RUST_LOG is set but not valid unicode - FAIL
            return Err(anyhow::anyhow!(
                "RUST_LOG environment variable contains invalid unicode: {:?}",
                os_str
            ));
        }
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter)
        .init();

    info!("Roblox Studio MCP Server starting...");

    // Create shared plugin bridge
    let bridge = Arc::new(PluginBridge::new());
    let project_root = std::env::current_dir()?;

    info!("Project root: {}", project_root.display());

    // Spawn HTTP bridge as background task (for plugin communication)
    // Graceful degradation: if port binding fails, MCP server continues without plugin support
    // Note: We clone PluginBridge here (not Arc) because create_router takes PluginBridge by value.
    // PluginBridge is cheap to clone (just Arc pointers internally).
    let http_bridge = (*bridge).clone();

    // Port is configurable via ROBLOX_MCP_PORT environment variable (default: 8080)
    let port = std::env::var("ROBLOX_MCP_PORT").unwrap_or_else(|_| "8080".to_string());
    let bind_addr = format!("127.0.0.1:{}", port);

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

    // Create MCP server with filesystem tools
    let server = RobloxMcpServer::new(bridge, project_root);

    // Run MCP server on STDIO (blocks main thread)
    info!("Starting MCP server on STDIO");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
