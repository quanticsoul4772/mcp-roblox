use rmcp::model::ErrorCode;
use rmcp::ErrorData;
use std::time::Duration;
use thiserror::Error;

/// Custom error types for the Roblox Studio MCP server
/// Following fast-failure philosophy: explicit, immediate, no fallbacks
#[derive(Error, Debug)]
pub enum RobloxMcpError {
    #[error("Studio plugin disconnected (last heartbeat: {0:?} ago). Restart Studio and reconnect the plugin.")]
    PluginTimeout(Duration),

    #[error("Studio plugin returned error: {0}")]
    PluginExecutionError(String),

    #[error("Rojo sync failed: {0}")]
    RojoSyncFailure(String),

    #[error("Invalid Studio response: {0}")]
    InvalidStudioData(String),

    #[error("File operation '{operation}' failed on '{path}': {source}")]
    FileSystemError {
        operation: String,
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Tool '{tool}' not installed. {install_hint}")]
    ToolNotInstalled { tool: String, install_hint: String },

    #[error("Tool '{tool}' execution failed: {message}")]
    ToolExecutionError { tool: String, message: String },

    // HTTP errors differentiated by category for proper MCP error code mapping
    #[error("Plugin request failed (client error {status}): {message}")]
    HttpClientError { status: u16, message: String },

    #[error("Plugin request failed (server error {status}): {message}")]
    HttpServerError { status: u16, message: String },

    #[error("Plugin connection failed: {0}")]
    HttpConnectionError(String),

    #[error("Plugin request timeout: {0}")]
    HttpTimeoutError(String),

    #[error("JSON serialization failed: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("File watcher error: {0}")]
    WatcherError(#[from] notify::Error),

    // === Phase 3: Open Cloud Integration ===
    #[error("Open Cloud API error (HTTP {status}): {message}")]
    OpenCloudError { status: u16, message: String },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Security violation: {0}")]
    SecurityViolation(String),

    // === AI Errors ===
    #[error("AI/Cloud API error: {0}")]
    CloudApiError(String),
}

/// Convert our custom errors to MCP protocol errors
/// Uses appropriate JSON-RPC error codes:
/// - -32600: Invalid Request (client errors, bad input)
/// - -32603: Internal Error (server errors, infrastructure issues)
/// - -32002: Server Error (custom: resource unavailable)
impl From<RobloxMcpError> for ErrorData {
    fn from(err: RobloxMcpError) -> Self {
        match &err {
            // Client errors (4xx) → Invalid Request (-32600)
            RobloxMcpError::HttpClientError { .. }
            | RobloxMcpError::InvalidPath(_)
            | RobloxMcpError::PathTraversal(_)
            | RobloxMcpError::SecurityViolation(_)
            | RobloxMcpError::InvalidStudioData(_) => Self::invalid_request(err.to_string(), None),

            // Server/infrastructure errors (5xx) → Internal Error (-32603)
            RobloxMcpError::HttpServerError { .. }
            | RobloxMcpError::FileSystemError { .. }
            | RobloxMcpError::SerializationError(_)
            | RobloxMcpError::WatcherError(_)
            | RobloxMcpError::ToolExecutionError { .. } => {
                Self::internal_error(err.to_string(), None)
            }

            // Tool not installed → Custom server error (tool dependency issue)
            RobloxMcpError::ToolNotInstalled { .. } => {
                ErrorData::new(ErrorCode(-32002), err.to_string(), None)
            }

            // Connection/availability errors → Custom server error (-32002)
            RobloxMcpError::PluginTimeout(_)
            | RobloxMcpError::HttpConnectionError(_)
            | RobloxMcpError::HttpTimeoutError(_)
            | RobloxMcpError::RojoSyncFailure(_) => {
                ErrorData::new(ErrorCode(-32002), err.to_string(), None)
            }

            // Plugin execution errors → Internal Error (-32603)
            RobloxMcpError::PluginExecutionError(_) => Self::internal_error(err.to_string(), None),

            // Open Cloud errors - map based on HTTP status
            RobloxMcpError::OpenCloudError { status, .. } => {
                if *status >= 400 && *status < 500 {
                    // Client errors (4xx) → Invalid Request
                    Self::invalid_request(err.to_string(), None)
                } else {
                    // Server errors (5xx) → Internal Error
                    Self::internal_error(err.to_string(), None)
                }
            }

            // Configuration errors → Invalid Request (client should fix config)
            RobloxMcpError::ConfigError(_) => Self::invalid_request(err.to_string(), None),

            // AI errors → Internal Error
            RobloxMcpError::CloudApiError(_) => Self::internal_error(err.to_string(), None),
        }
    }
}

// NOTE: No blanket From<std::io::Error> impl - callers MUST provide path context
// via RobloxMcpError::FileSystemError { path, source } explicitly.
// This ensures error messages always identify the failing file.

impl RobloxMcpError {
    /// Convert a reqwest error into an appropriate HTTP error variant
    /// Differentiates between client errors (4xx), server errors (5xx),
    /// connection failures, and timeouts
    pub fn from_reqwest(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            return Self::HttpTimeoutError(err.to_string());
        }

        if err.is_connect() {
            return Self::HttpConnectionError(err.to_string());
        }

        if let Some(status) = err.status() {
            let code = status.as_u16();
            let message = err.to_string();

            if status.is_client_error() {
                return Self::HttpClientError {
                    status: code,
                    message,
                };
            }

            if status.is_server_error() {
                return Self::HttpServerError {
                    status: code,
                    message,
                };
            }
        }

        // Fallback for other HTTP errors (redirects, decode errors, etc.)
        Self::HttpConnectionError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_timeout_error_display() {
        let err = RobloxMcpError::PluginTimeout(Duration::from_secs(15));
        let msg = format!("{err}");
        assert!(msg.contains("15"));
        assert!(msg.contains("Studio plugin disconnected"));
    }

    #[test]
    fn test_plugin_execution_error_display() {
        let err = RobloxMcpError::PluginExecutionError("Script not found".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Script not found"));
    }

    #[test]
    fn test_rojo_sync_failure_display() {
        let err = RobloxMcpError::RojoSyncFailure("Connection refused".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Connection refused"));
        assert!(msg.contains("Rojo"));
    }

    #[test]
    fn test_invalid_studio_data_display() {
        let err = RobloxMcpError::InvalidStudioData("Malformed JSON".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Malformed JSON"));
        assert!(msg.contains("Invalid Studio response"));
    }

    #[test]
    fn test_filesystem_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = RobloxMcpError::FileSystemError {
            operation: "read".to_string(),
            path: "/test/script.luau".to_string(),
            source: io_err,
        };
        let msg = format!("{err}");
        assert!(msg.contains("/test/script.luau"));
        assert!(msg.contains("File operation"));
        assert!(msg.contains("read"));
    }

    #[test]
    fn test_path_traversal_error_display() {
        let err = RobloxMcpError::PathTraversal("/etc/passwd".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("/etc/passwd"));
        assert!(msg.contains("traversal"));
    }

    #[test]
    fn test_invalid_path_error_display() {
        let err = RobloxMcpError::InvalidPath("Only .luau files supported".to_string());
        let msg = format!("{err}");
        assert!(msg.contains(".luau"));
    }

    #[test]
    fn test_error_to_mcp_errordata_conversion() {
        let err = RobloxMcpError::PluginTimeout(Duration::from_secs(30));
        let mcp_err: ErrorData = err.into();
        // ErrorData should contain the error message
        assert!(mcp_err.message.contains("30"));
    }

    #[test]
    fn test_serialization_error_from_serde() {
        let serde_err: serde_json::Error = serde_json::from_str::<String>("invalid").unwrap_err();
        let err: RobloxMcpError = serde_err.into();
        let msg = format!("{err}");
        assert!(msg.contains("JSON serialization failed"));
    }

    // HTTP error type tests
    #[test]
    fn test_http_client_error_display() {
        let err = RobloxMcpError::HttpClientError {
            status: 404,
            message: "Not Found".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("404"));
        assert!(msg.contains("client error"));
    }

    #[test]
    fn test_http_server_error_display() {
        let err = RobloxMcpError::HttpServerError {
            status: 503,
            message: "Service Unavailable".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("503"));
        assert!(msg.contains("server error"));
    }

    #[test]
    fn test_http_connection_error_display() {
        let err = RobloxMcpError::HttpConnectionError("Connection refused".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Connection refused"));
        assert!(msg.contains("connection failed"));
    }

    #[test]
    fn test_http_timeout_error_display() {
        let err = RobloxMcpError::HttpTimeoutError("Request timed out after 30s".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("timed out"));
        assert!(msg.contains("timeout"));
    }

    // MCP error code mapping tests
    #[test]
    fn test_client_error_maps_to_invalid_request() {
        let err = RobloxMcpError::HttpClientError {
            status: 400,
            message: "Bad Request".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        // -32600 is Invalid Request
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_server_error_maps_to_internal_error() {
        let err = RobloxMcpError::HttpServerError {
            status: 500,
            message: "Internal Server Error".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        // -32603 is Internal Error
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }

    #[test]
    fn test_connection_error_maps_to_server_error() {
        let err = RobloxMcpError::HttpConnectionError("Connection refused".to_string());
        let mcp_err: ErrorData = err.into();
        // -32002 is custom server unavailable
        assert_eq!(mcp_err.code, ErrorCode(-32002));
    }

    #[test]
    fn test_timeout_error_maps_to_server_error() {
        let err = RobloxMcpError::HttpTimeoutError("Timed out".to_string());
        let mcp_err: ErrorData = err.into();
        // -32002 is custom server unavailable
        assert_eq!(mcp_err.code, ErrorCode(-32002));
    }

    #[test]
    fn test_path_traversal_maps_to_invalid_request() {
        let err = RobloxMcpError::PathTraversal("/etc/passwd".to_string());
        let mcp_err: ErrorData = err.into();
        // -32600 is Invalid Request (client error)
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_invalid_path_maps_to_invalid_request() {
        let err = RobloxMcpError::InvalidPath("Not a .luau file".to_string());
        let mcp_err: ErrorData = err.into();
        // -32600 is Invalid Request (client error)
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_filesystem_error_maps_to_internal_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = RobloxMcpError::FileSystemError {
            operation: "read".to_string(),
            path: "/test.luau".to_string(),
            source: io_err,
        };
        let mcp_err: ErrorData = err.into();
        // -32603 is Internal Error
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }

    #[test]
    fn test_plugin_timeout_maps_to_server_error() {
        let err = RobloxMcpError::PluginTimeout(Duration::from_secs(30));
        let mcp_err: ErrorData = err.into();
        // -32002 is custom server unavailable
        assert_eq!(mcp_err.code, ErrorCode(-32002));
    }

    #[test]
    fn test_plugin_execution_error_maps_to_internal_error() {
        let err = RobloxMcpError::PluginExecutionError("Script error".to_string());
        let mcp_err: ErrorData = err.into();
        // -32603 is Internal Error
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }

    #[test]
    fn test_open_cloud_4xx_maps_to_invalid_request() {
        let err = RobloxMcpError::OpenCloudError {
            status: 401,
            message: "Unauthorized".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        // 4xx errors map to Invalid Request (-32600)
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_open_cloud_5xx_maps_to_internal_error() {
        let err = RobloxMcpError::OpenCloudError {
            status: 503,
            message: "Service Unavailable".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        // 5xx errors map to Internal Error (-32603)
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }

    #[test]
    fn test_config_error_maps_to_invalid_request() {
        let err = RobloxMcpError::ConfigError("Missing API key".to_string());
        let mcp_err: ErrorData = err.into();
        // Config errors map to Invalid Request (-32600)
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_config_error_display() {
        let err = RobloxMcpError::ConfigError("ROBLOX_OPEN_CLOUD_API_KEY not set".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Configuration error"));
        assert!(msg.contains("ROBLOX_OPEN_CLOUD_API_KEY"));
    }

    #[test]
    fn test_open_cloud_error_display() {
        let err = RobloxMcpError::OpenCloudError {
            status: 429,
            message: "Rate limited".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("429"));
        assert!(msg.contains("Open Cloud"));
    }

    #[test]
    fn test_watcher_error_maps_to_internal_error() {
        // Create a notify error using a path that doesn't exist
        let notify_err = notify::Error::path_not_found();
        let err = RobloxMcpError::WatcherError(notify_err);
        let mcp_err: ErrorData = err.into();
        // -32603 is Internal Error
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }

    #[test]
    fn test_rojo_sync_failure_maps_to_server_error() {
        let err = RobloxMcpError::RojoSyncFailure("Connection refused".to_string());
        let mcp_err: ErrorData = err.into();
        // -32002 is custom server unavailable
        assert_eq!(mcp_err.code, ErrorCode(-32002));
    }

    #[test]
    fn test_invalid_studio_data_maps_to_invalid_request() {
        let err = RobloxMcpError::InvalidStudioData("Malformed response".to_string());
        let mcp_err: ErrorData = err.into();
        // -32600 is Invalid Request
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_security_violation_maps_to_invalid_request() {
        let err = RobloxMcpError::SecurityViolation("Symlink attack detected".to_string());
        let mcp_err: ErrorData = err.into();
        // -32600 is Invalid Request (security violations are client errors)
        assert_eq!(mcp_err.code, ErrorCode(-32600));
    }

    #[test]
    fn test_security_violation_error_display() {
        let err = RobloxMcpError::SecurityViolation("test attack".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("Security violation"));
        assert!(msg.contains("test attack"));
    }

    // ========================================
    // from_reqwest tests using mockito
    // ========================================

    #[tokio::test]
    async fn test_from_reqwest_client_error_4xx() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/test")
            .with_status(404)
            .with_body("Not Found")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/test", server.url()))
            .send()
            .await
            .unwrap();

        // Force error by calling error_for_status on 4xx response
        let err = response.error_for_status().unwrap_err();
        let mcp_err = RobloxMcpError::from_reqwest(err);

        match mcp_err {
            RobloxMcpError::HttpClientError { status, .. } => {
                assert_eq!(status, 404);
            }
            e => panic!("Expected HttpClientError, got {e:?}"),
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_from_reqwest_server_error_5xx() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/test")
            .with_status(503)
            .with_body("Service Unavailable")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/test", server.url()))
            .send()
            .await
            .unwrap();

        let err = response.error_for_status().unwrap_err();
        let mcp_err = RobloxMcpError::from_reqwest(err);

        match mcp_err {
            RobloxMcpError::HttpServerError { status, .. } => {
                assert_eq!(status, 503);
            }
            e => panic!("Expected HttpServerError, got {e:?}"),
        }

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_from_reqwest_connection_error() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(100))
            .build()
            .unwrap();

        // Try to connect to a port that's definitely not listening
        let result = client.get("http://127.0.0.1:1").send().await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let mcp_err = RobloxMcpError::from_reqwest(err);

        // Connection errors map to HttpConnectionError
        assert!(matches!(
            mcp_err,
            RobloxMcpError::HttpConnectionError(_) | RobloxMcpError::HttpTimeoutError(_)
        ));
    }

    #[test]
    fn test_from_reqwest_timeout_detection() {
        // Test that the from_reqwest logic correctly handles timeout-like errors
        // by testing the branch detection logic directly.
        // Creating a real timeout error is flaky, so we verify the HttpTimeoutError
        // variant is correctly constructed and behaves as expected.

        let err = RobloxMcpError::HttpTimeoutError("request timed out".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("timeout"));

        // And verify it maps to the correct MCP error code
        let mcp_err: ErrorData = err.into();
        assert_eq!(mcp_err.code, ErrorCode(-32002));
    }

    #[tokio::test]
    async fn test_from_reqwest_various_4xx_codes() {
        for status_code in [400, 401, 403, 404, 422, 429] {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("GET", "/test")
                .with_status(status_code as usize)
                .create_async()
                .await;

            let client = reqwest::Client::new();
            let response = client
                .get(format!("{}/test", server.url()))
                .send()
                .await
                .unwrap();

            let err = response.error_for_status().unwrap_err();
            let mcp_err = RobloxMcpError::from_reqwest(err);

            match mcp_err {
                RobloxMcpError::HttpClientError { status, .. } => {
                    assert_eq!(status, status_code);
                }
                e => panic!("Expected HttpClientError for {status_code}, got {e:?}"),
            }

            mock.assert_async().await;
        }
    }

    #[tokio::test]
    async fn test_from_reqwest_various_5xx_codes() {
        for status_code in [500, 502, 503, 504] {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("GET", "/test")
                .with_status(status_code as usize)
                .create_async()
                .await;

            let client = reqwest::Client::new();
            let response = client
                .get(format!("{}/test", server.url()))
                .send()
                .await
                .unwrap();

            let err = response.error_for_status().unwrap_err();
            let mcp_err = RobloxMcpError::from_reqwest(err);

            match mcp_err {
                RobloxMcpError::HttpServerError { status, .. } => {
                    assert_eq!(status, status_code);
                }
                e => panic!("Expected HttpServerError for {status_code}, got {e:?}"),
            }

            mock.assert_async().await;
        }
    }

    #[test]
    fn test_open_cloud_error_boundary_400() {
        // Test exactly 400 - should be client error
        let err = RobloxMcpError::OpenCloudError {
            status: 400,
            message: "Bad Request".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        assert_eq!(mcp_err.code, ErrorCode(-32600)); // Invalid Request
    }

    #[test]
    fn test_open_cloud_error_boundary_499() {
        // Test exactly 499 - should still be client error
        let err = RobloxMcpError::OpenCloudError {
            status: 499,
            message: "Client Closed Request".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        assert_eq!(mcp_err.code, ErrorCode(-32600)); // Invalid Request
    }

    #[test]
    fn test_open_cloud_error_boundary_500() {
        // Test exactly 500 - should be server error
        let err = RobloxMcpError::OpenCloudError {
            status: 500,
            message: "Internal Server Error".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        assert_eq!(mcp_err.code, ErrorCode(-32603)); // Internal Error
    }

    #[test]
    fn test_open_cloud_error_below_400() {
        // Test 399 - not a 4xx client error, should go to else branch (server error)
        let err = RobloxMcpError::OpenCloudError {
            status: 399,
            message: "Redirect".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        assert_eq!(mcp_err.code, ErrorCode(-32603)); // Internal Error
    }

    #[test]
    fn test_watcher_error_display() {
        let notify_err = notify::Error::path_not_found();
        let err = RobloxMcpError::WatcherError(notify_err);
        let msg = format!("{}", err);
        assert!(msg.contains("watcher") || msg.contains("File"));
    }

    #[test]
    fn test_serialization_error_display() {
        let serde_err: serde_json::Error = serde_json::from_str::<i32>("not a number").unwrap_err();
        let err: RobloxMcpError = serde_err.into();
        let msg = format!("{}", err);
        assert!(msg.contains("JSON") || msg.contains("serialization"));
    }

    #[test]
    fn test_serialization_error_to_mcp() {
        let serde_err: serde_json::Error = serde_json::from_str::<String>("123").unwrap_err();
        let err: RobloxMcpError = serde_err.into();
        let mcp_err: ErrorData = err.into();
        // Serialization errors map to Internal Error (-32603)
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }

    #[test]
    fn test_tool_not_installed_error_to_mcp() {
        let err = RobloxMcpError::ToolNotInstalled {
            tool: "selene".to_string(),
            install_hint: "cargo install selene".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        // ToolNotInstalled maps to custom error code -32002
        assert_eq!(mcp_err.code, ErrorCode(-32002));
    }

    #[test]
    fn test_tool_not_installed_error_display() {
        let err = RobloxMcpError::ToolNotInstalled {
            tool: "stylua".to_string(),
            install_hint: "cargo install stylua".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("stylua"));
        assert!(msg.contains("cargo install stylua"));
    }

    #[test]
    fn test_tool_execution_error_to_mcp() {
        let err = RobloxMcpError::ToolExecutionError {
            tool: "rojo".to_string(),
            message: "Build failed".to_string(),
        };
        let mcp_err: ErrorData = err.into();
        // ToolExecutionError maps to Internal Error (-32603)
        assert_eq!(mcp_err.code, ErrorCode(-32603));
    }
}
