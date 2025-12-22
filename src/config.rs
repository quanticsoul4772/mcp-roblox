//! Server configuration with testable parsing logic
//!
//! Extracts configuration parsing from main.rs to enable unit testing
//! of environment variable handling and validation.

use std::ffi::OsString;
use std::path::PathBuf;
use thiserror::Error;
use tracing_subscriber::EnvFilter;

/// Configuration errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid RUST_LOG environment variable '{value}': {source}")]
    InvalidLogFilter {
        value: String,
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },

    #[error("RUST_LOG environment variable contains invalid unicode: {0:?}")]
    InvalidLogUnicode(OsString),

    #[error("ROBLOX_MCP_PORT environment variable contains invalid unicode: {0:?}")]
    InvalidPortUnicode(OsString),

    #[error("Invalid port number '{value}': {source}")]
    InvalidPort {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },

    #[error("Failed to get current directory: {0}")]
    CurrentDirError(#[from] std::io::Error),
}

/// Server configuration parsed from environment
#[derive(Debug)]
pub struct ServerConfig {
    /// Port for HTTP bridge (plugin communication)
    pub port: u16,
    /// Project root directory
    pub project_root: PathBuf,
    /// Tracing filter configuration
    pub env_filter: EnvFilter,
}

/// Default log filter when RUST_LOG is not set
pub const DEFAULT_LOG_FILTER: &str = "roblox_studio_mcp=info,tower_http=debug";

/// Default port when ROBLOX_MCP_PORT is not set
pub const DEFAULT_PORT: u16 = 8080;

impl ServerConfig {
    /// Create configuration from environment variables
    ///
    /// Reads:
    /// - `RUST_LOG` - Log filter configuration (optional, has default)
    /// - `ROBLOX_MCP_PORT` - HTTP bridge port (optional, defaults to 8080)
    /// - Current working directory as project root
    ///
    /// # Errors
    /// Returns error if:
    /// - RUST_LOG is set but invalid
    /// - RUST_LOG contains invalid unicode
    /// - ROBLOX_MCP_PORT is set but not a valid port number
    /// - Cannot determine current directory
    pub fn from_env() -> Result<Self, ConfigError> {
        let env_filter = parse_log_filter_from_env()?;
        let port = parse_port_from_env()?;
        let project_root = std::env::current_dir()?;

        Ok(Self {
            port,
            project_root,
            env_filter,
        })
    }

    /// Get the bind address for the HTTP bridge
    pub fn bind_addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// Parse log filter from RUST_LOG environment variable
///
/// # Returns
/// - If RUST_LOG is set and valid: parsed EnvFilter
/// - If RUST_LOG is not set: default filter
///
/// # Errors
/// - If RUST_LOG is set but invalid
/// - If RUST_LOG contains invalid unicode
pub fn parse_log_filter_from_env() -> Result<EnvFilter, ConfigError> {
    match std::env::var("RUST_LOG") {
        Ok(filter_str) => parse_log_filter(&filter_str),
        Err(std::env::VarError::NotPresent) => Ok(EnvFilter::new(DEFAULT_LOG_FILTER)),
        Err(std::env::VarError::NotUnicode(os_str)) => Err(ConfigError::InvalidLogUnicode(os_str)),
    }
}

/// Parse a log filter string into an EnvFilter
///
/// # Arguments
/// * `filter_str` - The filter string (e.g., "debug", "roblox_studio_mcp=info")
///
/// # Errors
/// Returns error if the filter string is invalid
pub fn parse_log_filter(filter_str: &str) -> Result<EnvFilter, ConfigError> {
    EnvFilter::try_new(filter_str).map_err(|e| ConfigError::InvalidLogFilter {
        value: filter_str.to_string(),
        source: e,
    })
}

/// Parse port from ROBLOX_MCP_PORT environment variable
///
/// # Returns
/// - If ROBLOX_MCP_PORT is set and valid: parsed port number
/// - If ROBLOX_MCP_PORT is not set: default port (8080)
///
/// # Errors
/// - If ROBLOX_MCP_PORT is set but not a valid u16
/// - If ROBLOX_MCP_PORT contains invalid unicode
pub fn parse_port_from_env() -> Result<u16, ConfigError> {
    match std::env::var("ROBLOX_MCP_PORT") {
        Ok(port_str) => parse_port(&port_str),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_PORT),
        Err(std::env::VarError::NotUnicode(os_str)) => Err(ConfigError::InvalidPortUnicode(os_str)),
    }
}

/// Parse a port string into a u16
///
/// # Arguments
/// * `port_str` - The port string (e.g., "8080", "3000")
///
/// # Errors
/// Returns error if the string is not a valid port number
pub fn parse_port(port_str: &str) -> Result<u16, ConfigError> {
    port_str.parse().map_err(|e| ConfigError::InvalidPort {
        value: port_str.to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // Global mutex to serialize tests that modify environment variables.
    // Environment variables are global process state, so parallel tests
    // can interfere with each other. Tests that modify RUST_LOG or
    // ROBLOX_MCP_PORT must acquire this lock first.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    // Helper to safely manage env vars in tests
    // MUST be used within a test that holds ENV_MUTEX
    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn new(key: &'static str) -> Self {
            let original = env::var(key).ok();
            Self { key, original }
        }

        fn set(&self, value: &str) {
            env::set_var(self.key, value);
        }

        fn remove(&self) {
            env::remove_var(self.key);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(val) => env::set_var(self.key, val),
                None => env::remove_var(self.key),
            }
        }
    }

    // ========================================
    // parse_log_filter tests
    // ========================================

    #[test]
    fn test_parse_log_filter_valid_simple() {
        let result = parse_log_filter("debug");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_log_filter_valid_complex() {
        let result = parse_log_filter("roblox_studio_mcp=info,tower_http=debug");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_log_filter_valid_with_target() {
        let result = parse_log_filter("my_crate::module=trace");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_log_filter_empty_string() {
        // Empty string is valid (means no filtering)
        let result = parse_log_filter("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_log_filter_invalid() {
        let result = parse_log_filter("invalid[filter");
        assert!(result.is_err());
        match result.unwrap_err() {
            ConfigError::InvalidLogFilter { value, .. } => {
                assert_eq!(value, "invalid[filter");
            }
            e => panic!("Expected InvalidLogFilter, got {e:?}"),
        }
    }

    // ========================================
    // parse_log_filter_from_env tests
    // These tests modify global environment state and must be serialized
    // ========================================

    #[test]
    fn test_parse_log_filter_from_env_not_set() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("RUST_LOG");
        guard.remove();

        let result = parse_log_filter_from_env();
        assert!(result.is_ok());
        // Should use default filter
    }

    #[test]
    fn test_parse_log_filter_from_env_valid() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("RUST_LOG");
        guard.set("warn");

        let result = parse_log_filter_from_env();
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_log_filter_from_env_invalid() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("RUST_LOG");
        guard.set("invalid[syntax");

        let result = parse_log_filter_from_env();
        assert!(result.is_err());
    }

    // ========================================
    // parse_port tests
    // ========================================

    #[test]
    fn test_parse_port_valid() {
        assert_eq!(parse_port("8080").unwrap(), 8080);
        assert_eq!(parse_port("3000").unwrap(), 3000);
        assert_eq!(parse_port("0").unwrap(), 0);
        assert_eq!(parse_port("65535").unwrap(), 65535);
    }

    #[test]
    fn test_parse_port_invalid_not_number() {
        let result = parse_port("abc");
        assert!(result.is_err());
        match result.unwrap_err() {
            ConfigError::InvalidPort { value, .. } => {
                assert_eq!(value, "abc");
            }
            e => panic!("Expected InvalidPort, got {e:?}"),
        }
    }

    #[test]
    fn test_parse_port_invalid_too_large() {
        let result = parse_port("99999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_port_invalid_negative() {
        let result = parse_port("-1");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_port_empty() {
        let result = parse_port("");
        assert!(result.is_err());
    }

    // ========================================
    // parse_port_from_env tests
    // These tests modify global environment state and must be serialized
    // ========================================

    #[test]
    fn test_parse_port_from_env_not_set() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("ROBLOX_MCP_PORT");
        guard.remove();

        let result = parse_port_from_env();
        assert_eq!(result.unwrap(), DEFAULT_PORT);
    }

    #[test]
    fn test_parse_port_from_env_valid() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("ROBLOX_MCP_PORT");
        guard.set("9000");

        let result = parse_port_from_env();
        assert_eq!(result.unwrap(), 9000);
    }

    #[test]
    fn test_parse_port_from_env_invalid() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("ROBLOX_MCP_PORT");
        guard.set("not_a_port");

        let result = parse_port_from_env();
        assert!(result.is_err());
    }

    // ========================================
    // ServerConfig tests
    // ========================================

    #[test]
    fn test_server_config_bind_addr() {
        // Create a config with known values - no env access needed
        let config = ServerConfig {
            port: 9999,
            project_root: PathBuf::from("/test"),
            env_filter: EnvFilter::new("info"),
        };

        assert_eq!(config.bind_addr(), "127.0.0.1:9999");
    }

    #[test]
    fn test_server_config_from_env() {
        // This test reads from environment, so needs the lock
        let _lock = ENV_MUTEX.lock().unwrap();

        // Ensure clean environment for this test
        let log_guard = EnvGuard::new("RUST_LOG");
        let port_guard = EnvGuard::new("ROBLOX_MCP_PORT");
        log_guard.remove();
        port_guard.remove();

        let result = ServerConfig::from_env();

        // Should succeed with clean environment
        assert!(result.is_ok(), "from_env should succeed with clean env");
        let config = result.unwrap();
        // Port should be default
        assert_eq!(config.port, DEFAULT_PORT);
        // Project root should not be empty
        assert!(!config.project_root.as_os_str().is_empty());
    }

    #[test]
    fn test_server_config_from_env_with_valid_port() {
        // This test modifies environment, so needs the lock
        let _lock = ENV_MUTEX.lock().unwrap();
        let guard = EnvGuard::new("ROBLOX_MCP_PORT");
        guard.set("9999");

        let result = parse_port_from_env();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 9999);
    }

    // ========================================
    // ConfigError tests
    // ========================================

    #[test]
    fn test_config_error_display_invalid_log_filter() {
        let err = ConfigError::InvalidLogFilter {
            value: "bad[filter".to_string(),
            source: EnvFilter::try_new("bad[filter").unwrap_err(),
        };
        let msg = err.to_string();
        assert!(msg.contains("RUST_LOG"));
        assert!(msg.contains("bad[filter"));
    }

    #[test]
    fn test_config_error_display_invalid_port() {
        let err = ConfigError::InvalidPort {
            value: "xyz".to_string(),
            source: "xyz".parse::<u16>().unwrap_err(),
        };
        let msg = err.to_string();
        assert!(msg.contains("port"));
        assert!(msg.contains("xyz"));
    }

    #[test]
    fn test_config_error_display_invalid_log_unicode() {
        use std::ffi::OsString;
        let err = ConfigError::InvalidLogUnicode(OsString::from("test"));
        let msg = err.to_string();
        assert!(msg.contains("RUST_LOG"));
        assert!(msg.contains("unicode"));
    }

    #[test]
    fn test_config_error_display_invalid_port_unicode() {
        use std::ffi::OsString;
        let err = ConfigError::InvalidPortUnicode(OsString::from("test"));
        let msg = err.to_string();
        assert!(msg.contains("ROBLOX_MCP_PORT"));
        assert!(msg.contains("unicode"));
    }

    // ========================================
    // Constants tests
    // ========================================

    #[test]
    fn test_default_log_filter_is_valid() {
        let result = parse_log_filter(DEFAULT_LOG_FILTER);
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_port_is_reasonable() {
        // Use runtime checks to avoid clippy::assertions_on_constants
        let port = DEFAULT_PORT;
        assert!(port > 0);
        assert!(port < 65535);
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_server_config_debug() {
        let config = ServerConfig {
            port: 8080,
            project_root: PathBuf::from("/test"),
            env_filter: EnvFilter::new("info"),
        };
        let debug = format!("{:?}", config);
        assert!(debug.contains("ServerConfig"));
        assert!(debug.contains("8080"));
    }
}
