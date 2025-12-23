//! Luau code formatting integration via StyLua
//!
//! Provides code formatting for Luau scripts using the StyLua formatter.
//!
//! This module provides a trait-based abstraction for formatting to enable testing
//! without requiring the external StyLua binary.

use crate::error::RobloxMcpError;
use crate::tools::timeout::execute_with_timeout;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Result from formatting a Luau script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatResult {
    /// Path to the formatted file
    pub file_path: String,
    /// Whether the file was formatted (changed)
    pub formatted: bool,
    /// Diff output if check_only mode was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Original content before formatting (only when check_only=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    /// Formatted content (only when check_only=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formatted_content: Option<String>,
}

// ============================================================================
// Formatter Trait Abstraction
// ============================================================================

/// Abstraction over formatting operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external StyLua binary to be installed.
#[async_trait]
pub trait Formatter: Send + Sync {
    /// Format a Luau script file
    ///
    /// # Arguments
    /// * `file_path` - Path to the .luau file to format
    /// * `config_path` - Optional path to stylua.toml configuration file
    /// * `check_only` - If true, don't modify file, just report if changes needed
    ///
    /// # Returns
    /// FormatResult indicating what was done
    async fn format(
        &self,
        file_path: &Path,
        config_path: Option<&Path>,
        check_only: bool,
    ) -> Result<FormatResult, RobloxMcpError>;
}

/// Production formatter using StyLua
///
/// Requires the `stylua` binary to be installed and available in PATH.
/// Install via: `cargo install stylua`
#[derive(Debug, Default, Clone)]
pub struct StyLuaFormatter;

impl StyLuaFormatter {
    /// Create a new StyLuaFormatter instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Formatter for StyLuaFormatter {
    async fn format(
        &self,
        file_path: &Path,
        config_path: Option<&Path>,
        check_only: bool,
    ) -> Result<FormatResult, RobloxMcpError> {
        format_script(file_path, config_path, check_only).await
    }
}

// ============================================================================
// Implementation Functions
// ============================================================================

/// Format a Luau script using the stylua CLI
///
/// # Errors
/// Returns error if:
/// - StyLua is not installed
/// - File cannot be formatted
/// - Tool execution times out (default: 30 seconds)
async fn format_script(
    file_path: &Path,
    config_path: Option<&Path>,
    check_only: bool,
) -> Result<FormatResult, RobloxMcpError> {
    let mut cmd = Command::new("stylua");

    // Add config path if provided
    if let Some(config) = config_path {
        cmd.arg("--config-path").arg(config);
    }

    if check_only {
        // In check mode, use --check flag and capture output
        cmd.arg("--check");
        cmd.arg(file_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Execute with timeout protection
        let output = execute_with_timeout(cmd, "stylua", None)
            .await
            .map_err(|e| {
                if e.to_string().contains("timed out") {
                    e
                } else {
                    RobloxMcpError::ToolNotInstalled {
                        tool: "stylua".to_string(),
                        install_hint: "Install via: cargo install stylua".to_string(),
                    }
                }
            })?;

        // stylua --check exits 0 if already formatted, non-zero if changes needed
        let needs_formatting = !output.status.success();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(FormatResult {
            file_path: file_path.display().to_string(),
            formatted: needs_formatting,
            diff: if needs_formatting && !stderr.is_empty() {
                Some(stderr)
            } else {
                None
            },
            original: None,
            formatted_content: None,
        })
    } else {
        // Format in place
        cmd.arg(file_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Execute with timeout protection
        let output = execute_with_timeout(cmd, "stylua", None)
            .await
            .map_err(|e| {
                if e.to_string().contains("timed out") {
                    e
                } else {
                    RobloxMcpError::ToolNotInstalled {
                        tool: "stylua".to_string(),
                        install_hint: "Install via: cargo install stylua".to_string(),
                    }
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(RobloxMcpError::ToolExecutionError {
                tool: "stylua".to_string(),
                message: if stderr.is_empty() {
                    "Formatting failed".to_string()
                } else {
                    stderr
                },
            });
        }

        Ok(FormatResult {
            file_path: file_path.display().to_string(),
            formatted: true,
            diff: None,
            original: None,
            formatted_content: None,
        })
    }
}

// ============================================================================
// Mock Formatter for Testing
// ============================================================================

/// Mock formatter for testing without StyLua binary
///
/// Returns pre-configured results for testing various format scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockFormatter
    #[derive(Debug)]
    struct MockState {
        responses: VecDeque<Result<FormatResult, RobloxMcpError>>,
        calls: Vec<(String, Option<String>, bool)>,
    }

    /// Mock formatter for testing
    #[derive(Clone)]
    pub struct MockFormatter {
        state: Arc<Mutex<MockState>>,
    }

    impl MockFormatter {
        /// Create a new mock formatter
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    responses: VecDeque::new(),
                    calls: Vec::new(),
                })),
            }
        }

        /// Queue a response to be returned on the next format call
        pub fn queue_response(&self, response: Result<FormatResult, RobloxMcpError>) {
            self.state.lock().unwrap().responses.push_back(response);
        }

        /// Get all calls made to this formatter
        pub fn calls(&self) -> Vec<(String, Option<String>, bool)> {
            self.state.lock().unwrap().calls.clone()
        }
    }

    impl Default for MockFormatter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for MockFormatter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let state = self.state.lock().unwrap();
            f.debug_struct("MockFormatter")
                .field("queued_responses", &state.responses.len())
                .field("call_count", &state.calls.len())
                .finish()
        }
    }

    #[async_trait]
    impl Formatter for MockFormatter {
        async fn format(
            &self,
            file_path: &Path,
            config_path: Option<&Path>,
            check_only: bool,
        ) -> Result<FormatResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state.calls.push((
                file_path.display().to_string(),
                config_path.map(|p| p.display().to_string()),
                check_only,
            ));

            state.responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockFormatter: No response queued".into(),
                ))
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_formatter_returns_queued_response() {
            let mock = MockFormatter::new();
            mock.queue_response(Ok(FormatResult {
                file_path: "/test.luau".to_string(),
                formatted: true,
                diff: None,
                original: None,
                formatted_content: None,
            }));

            let result = mock
                .format(Path::new("/test.luau"), None, false)
                .await
                .unwrap();

            assert!(result.formatted);
            assert_eq!(result.file_path, "/test.luau");
        }

        #[tokio::test]
        async fn test_mock_formatter_records_calls() {
            let mock = MockFormatter::new();
            mock.queue_response(Ok(FormatResult {
                file_path: "/test.luau".to_string(),
                formatted: true,
                diff: None,
                original: None,
                formatted_content: None,
            }));

            mock.format(
                Path::new("/test.luau"),
                Some(Path::new("/config/stylua.toml")),
                true,
            )
            .await
            .unwrap();

            let calls = mock.calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "/test.luau");
            assert_eq!(calls[0].1, Some("/config/stylua.toml".to_string()));
            assert!(calls[0].2); // check_only
        }

        #[tokio::test]
        async fn test_mock_formatter_no_response_queued() {
            let mock = MockFormatter::new();

            let result = mock.format(Path::new("/test.luau"), None, false).await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_formatter_clone_shares_state() {
            let mock1 = MockFormatter::new();
            mock1.queue_response(Ok(FormatResult {
                file_path: "/test.luau".to_string(),
                formatted: true,
                diff: None,
                original: None,
                formatted_content: None,
            }));

            let mock2 = mock1.clone();

            // Use mock2 to make the call
            mock2
                .format(Path::new("/test.luau"), None, false)
                .await
                .unwrap();

            // Verify mock1 can see the call (shared state)
            assert_eq!(mock1.calls().len(), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_result_serialization() {
        let result = FormatResult {
            file_path: "/test.luau".to_string(),
            formatted: true,
            diff: Some("- old\n+ new".to_string()),
            original: None,
            formatted_content: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("file_path"));
        assert!(json.contains("formatted"));
        assert!(json.contains("diff"));
        // original should be skipped when None
        assert!(!json.contains("original"));
    }

    #[test]
    fn test_format_result_clone() {
        let result = FormatResult {
            file_path: "/test.luau".to_string(),
            formatted: true,
            diff: None,
            original: None,
            formatted_content: None,
        };

        let cloned = result.clone();
        assert_eq!(cloned.file_path, result.file_path);
        assert_eq!(cloned.formatted, result.formatted);
    }

    #[test]
    fn test_stylua_formatter_new() {
        let formatter = StyLuaFormatter::new();
        assert!(format!("{:?}", formatter).contains("StyLuaFormatter"));
    }

    #[test]
    fn test_stylua_formatter_default() {
        let formatter = StyLuaFormatter;
        assert!(format!("{:?}", formatter).contains("StyLuaFormatter"));
    }
}
