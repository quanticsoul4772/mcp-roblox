mod bridge;
mod cloud;
mod config;
mod error;
mod http;
mod limits;
mod mcp;
mod metrics;
mod regex_safety;
mod startup;
mod tasks;
mod tools;
mod watcher;
mod ai;

use std::sync::Arc;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing::info;

use crate::config::ServerConfig;
use crate::mcp::RobloxMcpServer;
use crate::metrics::ServerMetrics;

use crate::ai::{LuauKnowledgeGraph, Neo4jConfig, VoyageConfig, VoyageEmbedder};
use crate::http::ReqwestHttpClient;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse configuration from environment (testable logic in config module)
    let config = ServerConfig::from_env().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Extract values before consuming env_filter (which doesn't implement Copy)
    let bind_addr = config.bind_addr();
    let project_root = config.project_root;

    // Initialize tracing to STDERR (stdout reserved for MCP JSON-RPC)
    startup::init_tracing(config.env_filter);
    startup::log_startup_info(&project_root);

    // Create shared metrics for cross-component tracking
    // This enables late result tracking when plugin returns data after caller has timed out
    let metrics = Arc::new(ServerMetrics::new());

    // Create shared plugin bridge with metrics for late result tracking
    let bridge = startup::create_bridge_with_metrics(metrics.clone());

    // Generate authentication token for HTTP bridge security
    let auth_token = startup::generate_auth_token();

    // Spawn HTTP bridge as background task (for plugin communication)
    // Graceful degradation: if port binding fails, MCP server continues without plugin support
    // Note: We clone PluginBridge here (not Arc) because spawn_http_bridge takes PluginBridge by value.
    // PluginBridge is cheap to clone (just Arc pointers internally).
    let http_bridge = (*bridge).clone();
    startup::spawn_http_bridge(http_bridge, bind_addr.to_string(), auth_token);

    // Create MCP server with shared metrics for unified tracking
    let server = RobloxMcpServer::new(bridge, project_root).with_shared_metrics(metrics);

    // Initialize AI knowledge graph - REQUIRED
    let voyage_config = VoyageConfig::from_env()
        .expect("VOYAGE_API_KEY is required");
    let neo4j_config = Neo4jConfig::from_env()
        .expect("NEO4J_URI, NEO4J_USERNAME, and NEO4J_PASSWORD are required");

    info!("Initializing knowledge graph...");
    let http_client = Arc::new(ReqwestHttpClient::new().expect("Failed to create HTTP client"));
    let embedder = Arc::new(VoyageEmbedder::new(http_client, voyage_config));
    let kg = LuauKnowledgeGraph::new(neo4j_config, embedder).await
        .expect("Failed to connect to Neo4j");
    info!("Knowledge graph initialized successfully");
    let server = server.with_knowledge_graph(Arc::new(kg));

    // Run MCP server on STDIO (blocks main thread)
    info!("Starting MCP server on STDIO");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
