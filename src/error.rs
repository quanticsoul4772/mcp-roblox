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

    #[error("File operation failed on '{path}': {source}")]
    FileSystemError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    // HTTP errors differentiated by category for proper MCP error code mapping
    #[error("Plugin request failed (client error {status}): {message}")]
    HttpClientError {
        status: u16,
        message: String,
    },

    #[error("Plugin request failed (server error {status}): {message}")]
    HttpServerError {
        status: u16,
        message: String,
    },

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
            | RobloxMcpError::InvalidStudioData(_) => {
                Self::invalid_request(err.to_string(), None)
            }

            // Server/infrastructure errors (5xx) → Internal Error (-32603)
            RobloxMcpError::HttpServerError { .. }
            | RobloxMcpError::FileSystemError { .. }
            | RobloxMcpError::SerializationError(_)
            | RobloxMcpError::WatcherError(_) => {
                Self::internal_error(err.to_string(), None)
            }

            // Connection/availability errors → Custom server error (-32002)
            RobloxMcpError::PluginTimeout(_)
            | RobloxMcpError::HttpConnectionError(_)
            | RobloxMcpError::HttpTimeoutError(_)
            | RobloxMcpError::RojoSyncFailure(_) => {
                ErrorData::new(ErrorCode(-32002), err.to_string(), None)
            }

            // Plugin execution errors → Internal Error (-32603)
            RobloxMcpError::PluginExecutionError(_) => {
                Self::internal_error(err.to_string(), None)
            }
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
            path: "/test/script.luau".to_string(),
            source: io_err,
        };
        let msg = format!("{err}");
        assert!(msg.contains("/test/script.luau"));
        assert!(msg.contains("File operation failed"));
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
}
