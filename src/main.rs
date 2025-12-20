mod bridge {
    pub mod http;
}
mod error;
mod mcp;
mod tools {
    pub mod filesystem;
}

use std::sync::Arc;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing::info;

use crate::bridge::http::{create_router, PluginBridge};
use crate::mcp::RobloxMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    // CRITICAL: Initialize logging to STDERR (stdout is reserved for MCP JSON-RPC protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "roblox_studio_mcp=info,tower_http=debug".into()),
        )
        .init();

    info!("Roblox Studio MCP Server starting...");

    // Create shared plugin bridge
    let bridge = Arc::new(PluginBridge::new());
    let project_root = std::env::current_dir()?;

    info!("Project root: {}", project_root.display());

    // Spawn HTTP bridge as background task (for plugin communication)
    let http_bridge = bridge.clone();
    tokio::spawn(async move {
        let app = create_router((*http_bridge).clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
            .await
            .expect("Failed to bind HTTP bridge to 127.0.0.1:8080");
        info!("HTTP bridge listening on 127.0.0.1:8080");
        axum::serve(listener, app)
            .await
            .expect("HTTP server error");
    });

    // Create MCP server with filesystem tools
    let server = RobloxMcpServer::new(bridge, project_root);

    // Run MCP server on STDIO (blocks main thread)
    info!("Starting MCP server on STDIO");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
