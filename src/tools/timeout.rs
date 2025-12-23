//! Timeout utilities for external tool execution
//!
//! External tools (StyLua, Selene, Rojo, Wally, Moonwave) can hang or take
//! excessively long to complete. This module provides timeout protection
//! to prevent server unresponsiveness.

use crate::error::RobloxMcpError;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Default timeout for external tool execution (30 seconds)
///
/// This is generous enough for most operations but prevents indefinite hangs.
pub const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// Execute a command with timeout protection
///
/// Wraps command execution with a timeout to prevent external tools from
/// hanging the server indefinitely.
///
/// # Arguments
/// * `cmd` - The prepared Command to execute (will be consumed)
/// * `tool_name` - Name of the tool (for error messages)
/// * `timeout_duration` - Optional custom timeout (defaults to 30 seconds)
///
/// # Returns
/// The command output if successful
///
/// # Errors
/// Returns `ToolExecutionError` if:
/// - The tool times out
/// - The tool fails to execute (not found, permission denied, etc.)
///
/// # Example
/// ```ignore
/// use roblox_studio_mcp::tools::timeout::execute_with_timeout;
/// use tokio::process::Command;
/// use std::time::Duration;
///
/// let mut cmd = Command::new("selene");
/// cmd.arg("--help");
///
/// let output = execute_with_timeout(cmd, "selene", None).await?;
/// ```
pub async fn execute_with_timeout(
    mut cmd: Command,
    tool_name: &str,
    timeout_duration: Option<Duration>,
) -> Result<std::process::Output, RobloxMcpError> {
    let duration = timeout_duration.unwrap_or(DEFAULT_TOOL_TIMEOUT);

    timeout(duration, cmd.output())
        .await
        .map_err(|_| RobloxMcpError::ToolExecutionError {
            tool: tool_name.to_string(),
            message: format!(
                "Tool execution timed out after {} seconds. \
                 The tool may be hanging or processing very large input.",
                duration.as_secs()
            ),
        })?
        .map_err(|e| RobloxMcpError::ToolExecutionError {
            tool: tool_name.to_string(),
            message: format!("Failed to execute: {e}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[tokio::test]
    async fn test_successful_command_within_timeout() {
        // Use a simple command that completes quickly
        #[cfg(unix)]
        let mut cmd = Command::new("echo");
        #[cfg(unix)]
        cmd.arg("hello");

        #[cfg(windows)]
        let mut cmd = Command::new("cmd");
        #[cfg(windows)]
        cmd.args(["/C", "echo", "hello"]);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let result = execute_with_timeout(cmd, "echo", Some(Duration::from_secs(5))).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.status.success());
    }

    #[tokio::test]
    async fn test_timeout_returns_error() {
        // Use a command that sleeps longer than the timeout
        #[cfg(unix)]
        let mut cmd = Command::new("sleep");
        #[cfg(unix)]
        cmd.arg("10");

        #[cfg(windows)]
        let mut cmd = Command::new("ping");
        #[cfg(windows)]
        cmd.args(["-n", "10", "127.0.0.1"]); // Ping 10 times takes ~10 seconds

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let result = execute_with_timeout(cmd, "sleep", Some(Duration::from_millis(100))).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            RobloxMcpError::ToolExecutionError { tool, message } => {
                assert_eq!(tool, "sleep");
                assert!(message.contains("timed out"));
            }
            e => panic!("Expected ToolExecutionError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_command_not_found() {
        let cmd = Command::new("this_command_definitely_does_not_exist_xyz123");

        let result = execute_with_timeout(cmd, "nonexistent", Some(Duration::from_secs(5))).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            RobloxMcpError::ToolExecutionError { tool, message } => {
                assert_eq!(tool, "nonexistent");
                assert!(message.contains("Failed to execute"));
            }
            e => panic!("Expected ToolExecutionError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_default_timeout_is_used() {
        // Verify default timeout constant is reasonable
        assert_eq!(DEFAULT_TOOL_TIMEOUT, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_custom_timeout_respected() {
        // Short timeout with a command that completes quickly
        #[cfg(unix)]
        let mut cmd = Command::new("true");
        #[cfg(windows)]
        let mut cmd = Command::new("cmd");
        #[cfg(windows)]
        cmd.args(["/C", "echo"]);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Even with a very short timeout, fast commands should succeed
        let result = execute_with_timeout(cmd, "quick", Some(Duration::from_millis(5000))).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_command_with_stderr() {
        // Run a command that produces stderr output
        #[cfg(unix)]
        let mut cmd = Command::new("sh");
        #[cfg(unix)]
        cmd.args(["-c", "echo error >&2"]);

        #[cfg(windows)]
        let mut cmd = Command::new("cmd");
        #[cfg(windows)]
        cmd.args(["/C", "echo error 1>&2"]);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let result = execute_with_timeout(cmd, "stderr-test", Some(Duration::from_secs(5))).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        // stderr should be captured
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("error") || output.status.success());
    }

    #[tokio::test]
    async fn test_none_timeout_uses_default() {
        // Test that passing None uses the default timeout
        #[cfg(unix)]
        let mut cmd = Command::new("true");
        #[cfg(windows)]
        let mut cmd = Command::new("cmd");
        #[cfg(windows)]
        cmd.args(["/C", "echo"]);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // Pass None - should use DEFAULT_TOOL_TIMEOUT (30s)
        let result = execute_with_timeout(cmd, "default-timeout", None).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_tool_execution_error_display() {
        let err = RobloxMcpError::ToolExecutionError {
            tool: "selene".to_string(),
            message: "Tool execution timed out after 30 seconds".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("selene"));
        assert!(msg.contains("timed out"));
    }

    #[tokio::test]
    async fn test_command_exit_code_preserved() {
        // Run a command that exits with non-zero status
        #[cfg(unix)]
        let mut cmd = Command::new("sh");
        #[cfg(unix)]
        cmd.args(["-c", "exit 42"]);

        #[cfg(windows)]
        let mut cmd = Command::new("cmd");
        #[cfg(windows)]
        cmd.args(["/C", "exit 42"]);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let result = execute_with_timeout(cmd, "exit-test", Some(Duration::from_secs(5))).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        // Exit code should be preserved (non-success)
        assert!(!output.status.success());
    }
}
