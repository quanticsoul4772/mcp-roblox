use thiserror::Error;
use rmcp::ErrorData;
use std::time::Duration;

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
    
    #[error("HTTP request to plugin failed: {0}")]
    HttpError(#[from] reqwest::Error),
    
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
impl From<RobloxMcpError> for ErrorData {
    fn from(err: RobloxMcpError) -> Self {
        ErrorData::internal_error(err.to_string(), None)
    }
}

// NOTE: No blanket From<std::io::Error> impl - callers MUST provide path context
// via RobloxMcpError::FileSystemError { path, source } explicitly.
// This ensures error messages always identify the failing file.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_timeout_error_display() {
        let err = RobloxMcpError::PluginTimeout(Duration::from_secs(15));
        let msg = format!("{}", err);
        assert!(msg.contains("15"));
        assert!(msg.contains("Studio plugin disconnected"));
    }

    #[test]
    fn test_plugin_execution_error_display() {
        let err = RobloxMcpError::PluginExecutionError("Script not found".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Script not found"));
    }

    #[test]
    fn test_rojo_sync_failure_display() {
        let err = RobloxMcpError::RojoSyncFailure("Connection refused".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Connection refused"));
        assert!(msg.contains("Rojo"));
    }

    #[test]
    fn test_invalid_studio_data_display() {
        let err = RobloxMcpError::InvalidStudioData("Malformed JSON".to_string());
        let msg = format!("{}", err);
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
        let msg = format!("{}", err);
        assert!(msg.contains("/test/script.luau"));
        assert!(msg.contains("File operation failed"));
    }

    #[test]
    fn test_path_traversal_error_display() {
        let err = RobloxMcpError::PathTraversal("/etc/passwd".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("/etc/passwd"));
        assert!(msg.contains("traversal"));
    }

    #[test]
    fn test_invalid_path_error_display() {
        let err = RobloxMcpError::InvalidPath("Only .luau files supported".to_string());
        let msg = format!("{}", err);
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
        let msg = format!("{}", err);
        assert!(msg.contains("JSON serialization failed"));
    }
}
