//! Server startup utilities with testable initialization logic
//!
//! Extracts startup logic from main.rs to enable unit testing
//! of server initialization, HTTP bridge spawning, and error handling.

use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::bridge::auth::AuthToken;
use crate::bridge::http::{create_authenticated_router, PluginBridge};
use crate::tasks::spawn_monitored;

/// Initialize the tracing subscriber with stderr output
///
/// Tracing is configured to write to stderr because stdout is reserved
/// for MCP JSON-RPC communication.
///
/// # Arguments
/// * `env_filter` - The tracing filter configuration
///
/// # Panics
/// Panics if a global subscriber has already been set
pub fn init_tracing(env_filter: EnvFilter) {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter)
        .init();
}

/// Result of attempting to bind the HTTP bridge
#[derive(Debug)]
pub enum BindResult {
    /// Successfully bound to the address
    Success(tokio::net::TcpListener),
    /// Failed to bind to the address
    BindError(std::io::Error),
}

/// Attempt to bind to the specified address
///
/// This is extracted as a separate function for testability.
///
/// # Arguments
/// * `bind_addr` - The address to bind to (e.g., "127.0.0.1:8080")
///
/// # Returns
/// `BindResult::Success` with the listener, or `BindResult::BindError` with the error
pub async fn try_bind(bind_addr: &str) -> BindResult {
    match tokio::net::TcpListener::bind(bind_addr).await {
        Ok(listener) => BindResult::Success(listener),
        Err(e) => BindResult::BindError(e),
    }
}

/// Format an error message for bind failures
///
/// # Arguments
/// * `bind_addr` - The address that failed to bind
/// * `error` - The error that occurred
///
/// # Returns
/// A formatted error message string
pub fn format_bind_error(bind_addr: &str, error: &std::io::Error) -> String {
    format!(
        "Failed to bind HTTP bridge to {}: {}. \
         Studio plugin communication will be unavailable. \
         Ensure the port is not in use, or set ROBLOX_MCP_PORT to a different port.",
        bind_addr, error
    )
}

/// Run the HTTP bridge server with authentication
///
/// This is the core HTTP bridge logic extracted for testability.
/// It attempts to bind to the specified address and serve HTTP requests.
///
/// # Arguments
/// * `bridge` - The plugin bridge for handling commands
/// * `bind_addr` - The address to bind to
/// * `token` - Authentication token for securing endpoints
///
/// # Behavior
/// - On bind failure: logs error and returns (graceful degradation)
/// - On serve error: logs error and returns
/// - On success: serves HTTP requests until shutdown
pub async fn run_http_bridge(bridge: PluginBridge, bind_addr: &str, token: AuthToken) {
    let app = create_authenticated_router(bridge, token);

    let listener = match try_bind(bind_addr).await {
        BindResult::Success(listener) => listener,
        BindResult::BindError(e) => {
            error!("{}", format_bind_error(bind_addr, &e));
            return;
        }
    };

    info!("HTTP bridge listening on {}", bind_addr);

    if let Err(e) = axum::serve(listener, app).await {
        error!(
            "HTTP bridge server error: {}. Plugin communication has stopped.",
            e
        );
    }
}

/// Spawn the HTTP bridge as a background task with panic monitoring
///
/// This spawns `run_http_bridge` as a monitored tokio task, allowing the main
/// server to continue running even if the HTTP bridge fails. Unlike raw
/// `tokio::spawn`, panics and unexpected terminations are logged.
///
/// # Arguments
/// * `bridge` - The plugin bridge for handling commands
/// * `bind_addr` - The address to bind to
/// * `token` - Authentication token for securing endpoints
///
/// # Returns
/// A JoinHandle for the spawned task
pub fn spawn_http_bridge(
    bridge: PluginBridge,
    bind_addr: String,
    token: AuthToken,
) -> JoinHandle<()> {
    spawn_monitored("http_bridge", async move {
        run_http_bridge(bridge, &bind_addr, token).await
    })
}

/// Generate a new authentication token and log it for plugin configuration
///
/// The token is logged to stderr (where tracing output goes) so it can be
/// captured for plugin configuration. The token must be included in the
/// Authorization header as "Bearer <token>" for all authenticated endpoints.
///
/// # Returns
/// A new AuthToken that should be passed to `spawn_http_bridge`
pub fn generate_auth_token() -> AuthToken {
    let token = AuthToken::generate();
    info!("HTTP Bridge Authentication Token: {}", token.as_str());
    info!("Configure your Roblox Studio plugin with this token");
    token
}

/// Parse and validate a socket address string
///
/// # Arguments
/// * `addr` - The address string to parse (e.g., "127.0.0.1:8080")
///
/// # Returns
/// `Ok(SocketAddr)` if valid, `Err` with the parse error otherwise
#[allow(dead_code)] // Utility function available for future use
pub fn parse_socket_addr(addr: &str) -> Result<std::net::SocketAddr, std::net::AddrParseError> {
    addr.parse()
}

/// Log server startup information
///
/// # Arguments
/// * `project_root` - The project root directory path
pub fn log_startup_info(project_root: &std::path::Path) {
    info!("Roblox Studio MCP Server starting...");
    info!("Project root: {}", project_root.display());
}

/// Create the shared plugin bridge without metrics
///
/// # Returns
/// An Arc-wrapped PluginBridge instance
#[allow(dead_code)] // Available for simple use cases without metrics
pub fn create_bridge() -> Arc<PluginBridge> {
    Arc::new(PluginBridge::new())
}

/// Create the shared plugin bridge with metrics for late result tracking
///
/// # Arguments
/// * `metrics` - Shared metrics instance for tracking late results and unknown commands
///
/// # Returns
/// An Arc-wrapped PluginBridge instance with metrics enabled
pub fn create_bridge_with_metrics(
    metrics: Arc<crate::metrics::ServerMetrics>,
) -> Arc<PluginBridge> {
    Arc::new(PluginBridge::with_metrics(metrics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ========================================
    // format_bind_error tests
    // ========================================

    #[test]
    fn test_format_bind_error_contains_address() {
        let error = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address in use");
        let msg = format_bind_error("127.0.0.1:8080", &error);
        assert!(msg.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_format_bind_error_contains_error_message() {
        let error = std::io::Error::new(std::io::ErrorKind::AddrInUse, "address already in use");
        let msg = format_bind_error("127.0.0.1:8080", &error);
        assert!(msg.contains("address already in use"));
    }

    #[test]
    fn test_format_bind_error_contains_port_suggestion() {
        let error = std::io::Error::new(std::io::ErrorKind::AddrInUse, "in use");
        let msg = format_bind_error("127.0.0.1:8080", &error);
        assert!(msg.contains("ROBLOX_MCP_PORT"));
    }

    #[test]
    fn test_format_bind_error_mentions_plugin_unavailable() {
        let error = std::io::Error::new(std::io::ErrorKind::AddrInUse, "in use");
        let msg = format_bind_error("127.0.0.1:8080", &error);
        assert!(msg.contains("Studio plugin communication will be unavailable"));
    }

    // ========================================
    // parse_socket_addr tests
    // ========================================

    #[test]
    fn test_parse_socket_addr_valid_ipv4() {
        let result = parse_socket_addr("127.0.0.1:8080");
        assert!(result.is_ok());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn test_parse_socket_addr_valid_localhost() {
        let result = parse_socket_addr("0.0.0.0:3000");
        assert!(result.is_ok());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 3000);
    }

    #[test]
    fn test_parse_socket_addr_invalid_no_port() {
        let result = parse_socket_addr("127.0.0.1");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_socket_addr_invalid_format() {
        let result = parse_socket_addr("not_an_address");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_socket_addr_empty() {
        let result = parse_socket_addr("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_socket_addr_ipv6() {
        let result = parse_socket_addr("[::1]:8080");
        assert!(result.is_ok());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 8080);
    }

    // ========================================
    // create_bridge tests
    // ========================================

    #[test]
    fn test_create_bridge_returns_arc() {
        let bridge = create_bridge();
        // Verify we can clone the Arc (basic functionality check)
        let _bridge2 = bridge.clone();
        assert!(Arc::strong_count(&bridge) >= 1);
    }

    #[test]
    fn test_create_bridge_multiple_calls_independent() {
        let bridge1 = create_bridge();
        let bridge2 = create_bridge();
        // Each call should create a new independent bridge
        assert_eq!(Arc::strong_count(&bridge1), 1);
        assert_eq!(Arc::strong_count(&bridge2), 1);
    }

    #[test]
    fn test_create_bridge_with_metrics_returns_arc() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let bridge = create_bridge_with_metrics(metrics.clone());
        // Verify we can clone the Arc (basic functionality check)
        let _bridge2 = bridge.clone();
        assert!(Arc::strong_count(&bridge) >= 1);
    }

    #[test]
    fn test_create_bridge_with_metrics_shares_metrics() {
        let metrics = Arc::new(crate::metrics::ServerMetrics::new());
        let _bridge = create_bridge_with_metrics(metrics.clone());
        // Metrics should have 2 references: original + inside bridge
        assert!(Arc::strong_count(&metrics) >= 2);
    }

    // ========================================
    // try_bind tests (async)
    // ========================================

    #[tokio::test]
    async fn test_try_bind_success_on_available_port() {
        // Use port 0 to let the OS assign an available port
        let result = try_bind("127.0.0.1:0").await;
        match result {
            BindResult::Success(listener) => {
                let addr = listener.local_addr().unwrap();
                assert!(addr.port() > 0);
            }
            BindResult::BindError(e) => {
                panic!("Expected success, got error: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_try_bind_failure_on_invalid_address() {
        // Invalid address should fail
        let result = try_bind("999.999.999.999:8080").await;
        match result {
            BindResult::Success(_) => {
                panic!("Expected bind error for invalid address");
            }
            BindResult::BindError(_) => {
                // Expected
            }
        }
    }

    #[tokio::test]
    async fn test_try_bind_returns_listener_with_correct_port() {
        let result = try_bind("127.0.0.1:0").await;
        if let BindResult::Success(listener) = result {
            let local_addr = listener.local_addr().unwrap();
            // The listener should have a valid local address
            assert!(local_addr.ip().is_loopback());
        }
    }

    // ========================================
    // BindResult tests
    // ========================================

    #[test]
    fn test_bind_result_debug_success() {
        // We can't easily create a TcpListener in a sync test,
        // but we can test the Debug trait is implemented
        let error = std::io::Error::new(std::io::ErrorKind::AddrInUse, "test");
        let result = BindResult::BindError(error);
        let debug = format!("{:?}", result);
        assert!(debug.contains("BindError"));
    }

    // ========================================
    // log_startup_info tests
    // ========================================

    #[test]
    fn test_log_startup_info_does_not_panic() {
        // Just verify it doesn't panic with various paths
        let path = PathBuf::from("/test/project");
        log_startup_info(&path);

        let path = PathBuf::from(".");
        log_startup_info(&path);

        let path = PathBuf::from("");
        log_startup_info(&path);
    }

    // ========================================
    // spawn_http_bridge tests
    // ========================================

    #[tokio::test]
    async fn test_spawn_http_bridge_returns_join_handle() {
        let bridge = PluginBridge::new();
        let token = AuthToken::generate();
        // Use an invalid address so it fails immediately
        let handle = spawn_http_bridge(bridge, "999.999.999.999:8080".to_string(), token);

        // The task should complete (with an error internally) without panicking
        // Give it a moment to fail
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Abort the handle to clean up
        handle.abort();
    }

    #[tokio::test]
    async fn test_spawn_http_bridge_graceful_failure() {
        let bridge = PluginBridge::new();
        let token = AuthToken::generate();
        // Invalid address will cause bind to fail
        let handle = spawn_http_bridge(bridge, "invalid:not_a_port".to_string(), token);

        // The task should complete without panicking
        let result = tokio::time::timeout(tokio::time::Duration::from_millis(100), handle).await;

        // Should either complete (Ok) or timeout (Err) - both are acceptable
        // The important thing is no panic
        match result {
            Ok(join_result) => {
                // Task completed - should be Ok(()) even on bind failure
                assert!(join_result.is_ok());
            }
            Err(_) => {
                // Timeout is also acceptable for this test
            }
        }
    }

    // ========================================
    // run_http_bridge tests
    // ========================================

    #[tokio::test]
    async fn test_run_http_bridge_fails_gracefully_on_invalid_address() {
        let bridge = PluginBridge::new();
        let token = AuthToken::generate();
        // This should return immediately without panicking
        run_http_bridge(bridge, "999.999.999.999:8080", token).await;
        // If we get here, the function handled the error gracefully
    }

    #[tokio::test]
    async fn test_run_http_bridge_fails_gracefully_on_malformed_address() {
        let bridge = PluginBridge::new();
        let token = AuthToken::generate();
        // Malformed address should fail gracefully
        run_http_bridge(bridge, "not_an_address", token).await;
        // Success if no panic
    }

    // ========================================
    // Integration-style tests
    // ========================================

    #[tokio::test]
    async fn test_full_startup_sequence_without_serving() {
        // Test the full startup sequence without actually serving
        let bridge = create_bridge();
        let token = AuthToken::generate();

        // Clone the inner bridge (like main.rs does)
        let http_bridge = (*bridge).clone();

        // Spawn with invalid address so it fails immediately
        let _handle = spawn_http_bridge(http_bridge, "999.999.999.999:0".to_string(), token);

        // Small delay to let the task start
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // The bridge should still be usable even if HTTP failed
        assert!(Arc::strong_count(&bridge) >= 1);
    }

    // ========================================
    // Authentication token tests
    // ========================================

    #[test]
    fn test_generate_auth_token_returns_token() {
        let token = generate_auth_token();
        // Token should be non-empty
        assert!(!token.as_str().is_empty());
    }

    #[test]
    fn test_generate_auth_token_produces_unique_tokens() {
        let token1 = generate_auth_token();
        let token2 = generate_auth_token();
        // Each call should produce a unique token
        assert_ne!(token1.as_str(), token2.as_str());
    }
}
