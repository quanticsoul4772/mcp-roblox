//! Lune runtime integration
//!
//! Provides Lune script execution for testing Luau code outside Roblox Studio.
//!
//! This module provides a trait-based abstraction for Lune operations to enable testing
//! without requiring the external Lune binary.

use crate::error::RobloxMcpError;
use crate::tools::timeout::execute_with_timeout;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Result from running a Lune script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuneRunResult {
    /// Whether the script executed successfully (exit code 0)
    pub success: bool,
    /// Exit code from the script (None if terminated by signal)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Signal that terminated the process (Unix only, None if exited normally)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// Stdout from the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Stderr from the script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Execution time in milliseconds
    pub duration_ms: u64,
}

// ============================================================================
// Lune Trait Abstraction
// ============================================================================

/// Abstraction over Lune operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external Lune binary to be installed.
#[async_trait]
pub trait LuneRunner: Send + Sync {
    /// Run a Luau script file
    ///
    /// # Arguments
    /// * `script_path` - Path to the .luau script file
    /// * `args` - Command-line arguments to pass to the script
    /// * `timeout` - Optional timeout duration (default: 30 seconds)
    async fn run(
        &self,
        script_path: &Path,
        args: &[String],
        timeout: Option<Duration>,
    ) -> Result<LuneRunResult, RobloxMcpError>;

    /// Evaluate inline Luau code
    ///
    /// Creates a temporary file with the code and executes it.
    ///
    /// # Arguments
    /// * `code` - Luau code to evaluate
    /// * `timeout` - Optional timeout duration (default: 10 seconds)
    async fn eval(&self, code: &str, timeout: Option<Duration>)
        -> Result<LuneRunResult, RobloxMcpError>;
}

/// Production Lune runner using the lune CLI
///
/// Requires the `lune` binary to be installed and available in PATH.
/// Install via: `rokit add lune-org/lune` or `cargo install lune`
#[derive(Debug, Default, Clone)]
pub struct DefaultLuneRunner;

impl DefaultLuneRunner {
    /// Create a new DefaultLuneRunner instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LuneRunner for DefaultLuneRunner {
    async fn run(
        &self,
        script_path: &Path,
        args: &[String],
        timeout: Option<Duration>,
    ) -> Result<LuneRunResult, RobloxMcpError> {
        run_script(script_path, args, timeout).await
    }

    async fn eval(
        &self,
        code: &str,
        timeout: Option<Duration>,
    ) -> Result<LuneRunResult, RobloxMcpError> {
        eval_code(code, timeout).await
    }
}

// ============================================================================
// Implementation Functions
// ============================================================================

/// Run a Luau script using the lune CLI
///
/// # Errors
/// Returns error if:
/// - Lune is not installed
/// - Tool execution times out
async fn run_script(
    script_path: &Path,
    args: &[String],
    timeout: Option<Duration>,
) -> Result<LuneRunResult, RobloxMcpError> {
    let start = std::time::Instant::now();

    let mut cmd = Command::new("lune");
    cmd.arg("run").arg(script_path);

    // Add script arguments after "--" separator
    if !args.is_empty() {
        cmd.arg("--");
        for arg in args {
            cmd.arg(arg);
        }
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Execute with timeout protection (default 30 seconds for scripts)
    let output = execute_with_timeout(cmd, "lune", timeout).await.map_err(|e| {
        if e.to_string().contains("timed out") {
            e
        } else {
            RobloxMcpError::ToolNotInstalled {
                tool: "lune".to_string(),
                install_hint: "Install via: rokit add lune-org/lune".to_string(),
            }
        }
    })?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Extract exit code and signal information
    let exit_code = output.status.code();

    // On Unix, get the signal that terminated the process (if any)
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        output.status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;

    Ok(LuneRunResult {
        success: output.status.success(),
        exit_code,
        signal,
        stdout: if stdout.is_empty() {
            None
        } else {
            Some(stdout)
        },
        stderr: if stderr.is_empty() {
            None
        } else {
            Some(stderr)
        },
        duration_ms,
    })
}

/// Evaluate inline Luau code using the lune CLI
///
/// Creates a temporary file with the code and executes it.
///
/// # Errors
/// Returns error if:
/// - Lune is not installed
/// - Tool execution times out
/// - Failed to create temp file
async fn eval_code(code: &str, timeout: Option<Duration>) -> Result<LuneRunResult, RobloxMcpError> {
    use std::io::Write;

    // Create a temporary file for the code
    let mut temp_file = tempfile::Builder::new()
        .prefix("lune_eval_")
        .suffix(".luau")
        .tempfile()
        .map_err(|e| RobloxMcpError::FileSystemError {
            operation: "create temp file".to_string(),
            path: "/tmp/lune_eval_*.luau".to_string(),
            source: e,
        })?;

    // Write the code to the temp file
    temp_file
        .write_all(code.as_bytes())
        .map_err(|e| RobloxMcpError::FileSystemError {
            operation: "write temp file".to_string(),
            path: temp_file.path().display().to_string(),
            source: e,
        })?;

    // Flush to ensure content is written
    temp_file.flush().map_err(|e| RobloxMcpError::FileSystemError {
        operation: "flush temp file".to_string(),
        path: temp_file.path().display().to_string(),
        source: e,
    })?;

    // Run the temp file with lune (default 10 seconds for eval)
    let timeout = timeout.or(Some(Duration::from_secs(10)));
    let result = run_script(temp_file.path(), &[], timeout).await;

    // Temp file is automatically cleaned up when temp_file goes out of scope
    result
}

// ============================================================================
// Mock Lune Runner for Testing
// ============================================================================

/// Mock Lune runner for testing without the lune binary
///
/// Returns pre-configured results for testing various Lune scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockLuneRunner
    #[derive(Debug)]
    struct MockState {
        run_responses: VecDeque<Result<LuneRunResult, RobloxMcpError>>,
        eval_responses: VecDeque<Result<LuneRunResult, RobloxMcpError>>,
        run_calls: Vec<(String, Vec<String>)>,
        eval_calls: Vec<String>,
    }

    /// Mock Lune runner for testing
    #[derive(Clone)]
    pub struct MockLuneRunner {
        state: Arc<Mutex<MockState>>,
    }

    impl MockLuneRunner {
        /// Create a new mock runner
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    run_responses: VecDeque::new(),
                    eval_responses: VecDeque::new(),
                    run_calls: Vec::new(),
                    eval_calls: Vec::new(),
                })),
            }
        }

        /// Queue a run response
        pub fn queue_run_response(&self, response: Result<LuneRunResult, RobloxMcpError>) {
            self.state
                .lock()
                .unwrap()
                .run_responses
                .push_back(response);
        }

        /// Queue an eval response
        pub fn queue_eval_response(&self, response: Result<LuneRunResult, RobloxMcpError>) {
            self.state
                .lock()
                .unwrap()
                .eval_responses
                .push_back(response);
        }

        /// Get all run calls made
        pub fn run_calls(&self) -> Vec<(String, Vec<String>)> {
            self.state.lock().unwrap().run_calls.clone()
        }

        /// Get all eval calls made
        pub fn eval_calls(&self) -> Vec<String> {
            self.state.lock().unwrap().eval_calls.clone()
        }
    }

    impl Default for MockLuneRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for MockLuneRunner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let state = self.state.lock().unwrap();
            f.debug_struct("MockLuneRunner")
                .field("queued_run_responses", &state.run_responses.len())
                .field("queued_eval_responses", &state.eval_responses.len())
                .field("run_calls", &state.run_calls.len())
                .field("eval_calls", &state.eval_calls.len())
                .finish()
        }
    }

    #[async_trait]
    impl LuneRunner for MockLuneRunner {
        async fn run(
            &self,
            script_path: &Path,
            args: &[String],
            _timeout: Option<Duration>,
        ) -> Result<LuneRunResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state
                .run_calls
                .push((script_path.display().to_string(), args.to_vec()));

            state.run_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockLuneRunner: No run response queued".into(),
                ))
            })
        }

        async fn eval(
            &self,
            code: &str,
            _timeout: Option<Duration>,
        ) -> Result<LuneRunResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state.eval_calls.push(code.to_string());

            state.eval_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockLuneRunner: No eval response queued".into(),
                ))
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_runner_run() {
            let mock = MockLuneRunner::new();
            mock.queue_run_response(Ok(LuneRunResult {
                success: true,
                exit_code: Some(0),
                signal: None,
                stdout: Some("Hello, World!".to_string()),
                stderr: None,
                duration_ms: 42,
            }));

            let result = mock
                .run(Path::new("/test/script.luau"), &[], None)
                .await
                .unwrap();

            assert!(result.success);
            assert_eq!(result.exit_code, Some(0));
            assert_eq!(result.stdout, Some("Hello, World!".to_string()));
            assert_eq!(mock.run_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_eval() {
            let mock = MockLuneRunner::new();
            mock.queue_eval_response(Ok(LuneRunResult {
                success: true,
                exit_code: Some(0),
                signal: None,
                stdout: Some("1024".to_string()),
                stderr: None,
                duration_ms: 5,
            }));

            let result = mock.eval("print(2^10)", None).await.unwrap();

            assert!(result.success);
            assert_eq!(result.stdout, Some("1024".to_string()));
            assert_eq!(mock.eval_calls(), vec!["print(2^10)"]);
        }

        #[tokio::test]
        async fn test_mock_runner_no_response_queued() {
            let mock = MockLuneRunner::new();

            let result = mock.run(Path::new("/test/script.luau"), &[], None).await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_runner_clone_shares_state() {
            let mock1 = MockLuneRunner::new();
            mock1.queue_run_response(Ok(LuneRunResult {
                success: true,
                exit_code: Some(0),
                signal: None,
                stdout: None,
                stderr: None,
                duration_ms: 1,
            }));

            let mock2 = mock1.clone();

            // Use mock2 to make the call
            mock2
                .run(Path::new("/test/script.luau"), &[], None)
                .await
                .unwrap();

            // Verify mock1 can see the call (shared state)
            assert_eq!(mock1.run_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_with_args() {
            let mock = MockLuneRunner::new();
            mock.queue_run_response(Ok(LuneRunResult {
                success: true,
                exit_code: Some(0),
                signal: None,
                stdout: None,
                stderr: None,
                duration_ms: 1,
            }));

            let args = vec!["--flag".to_string(), "value".to_string()];
            mock.run(Path::new("/test/script.luau"), &args, None)
                .await
                .unwrap();

            let calls = mock.run_calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].1, vec!["--flag", "value"]);
        }

        #[tokio::test]
        async fn test_mock_runner_failure_response() {
            let mock = MockLuneRunner::new();
            mock.queue_run_response(Ok(LuneRunResult {
                success: false,
                exit_code: Some(1),
                signal: None,
                stdout: None,
                stderr: Some("Error: undefined variable 'foo'".to_string()),
                duration_ms: 10,
            }));

            let result = mock
                .run(Path::new("/test/failing.luau"), &[], None)
                .await
                .unwrap();

            assert!(!result.success);
            assert_eq!(result.exit_code, Some(1));
            assert!(result.stderr.unwrap().contains("undefined variable"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lune_run_result_serialization() {
        let result = LuneRunResult {
            success: true,
            exit_code: Some(0),
            signal: None,
            stdout: Some("Hello, World!".to_string()),
            stderr: None,
            duration_ms: 42,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("exit_code"));
        assert!(json.contains("stdout"));
        assert!(json.contains("duration_ms"));
        // stderr and signal should be skipped when None
        assert!(!json.contains("stderr"));
        assert!(!json.contains("signal"));
    }

    #[test]
    fn test_lune_run_result_deserialization() {
        let json = r#"{
            "success": true,
            "exit_code": 0,
            "stdout": "output",
            "duration_ms": 100
        }"#;

        let result: LuneRunResult = serde_json::from_str(json).unwrap();
        assert!(result.success);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.signal, None);
        assert_eq!(result.stdout, Some("output".to_string()));
        assert_eq!(result.stderr, None);
        assert_eq!(result.duration_ms, 100);
    }

    #[test]
    fn test_lune_run_result_clone() {
        let result = LuneRunResult {
            success: true,
            exit_code: Some(0),
            signal: None,
            stdout: Some("test".to_string()),
            stderr: None,
            duration_ms: 50,
        };

        let cloned = result.clone();
        assert_eq!(cloned.success, result.success);
        assert_eq!(cloned.exit_code, result.exit_code);
        assert_eq!(cloned.signal, result.signal);
        assert_eq!(cloned.stdout, result.stdout);
        assert_eq!(cloned.duration_ms, result.duration_ms);
    }

    #[test]
    fn test_default_lune_runner_new() {
        let runner = DefaultLuneRunner::new();
        assert!(format!("{:?}", runner).contains("DefaultLuneRunner"));
    }

    #[test]
    fn test_default_lune_runner_default() {
        let runner = DefaultLuneRunner::default();
        assert!(format!("{:?}", runner).contains("DefaultLuneRunner"));
    }

    #[test]
    fn test_lune_run_result_with_error() {
        let result = LuneRunResult {
            success: false,
            exit_code: Some(1),
            signal: None,
            stdout: None,
            stderr: Some("Error: something went wrong".to_string()),
            duration_ms: 10,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("stderr"));
        assert!(json.contains("Error: something went wrong"));
        // stdout and signal should be skipped when None
        assert!(!json.contains("stdout"));
        assert!(!json.contains("signal"));
    }

    #[test]
    fn test_lune_run_result_roundtrip() {
        let original = LuneRunResult {
            success: true,
            exit_code: Some(0),
            signal: None,
            stdout: Some("Hello".to_string()),
            stderr: Some("Warning".to_string()),
            duration_ms: 123,
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: LuneRunResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.success, original.success);
        assert_eq!(parsed.exit_code, original.exit_code);
        assert_eq!(parsed.signal, original.signal);
        assert_eq!(parsed.stdout, original.stdout);
        assert_eq!(parsed.stderr, original.stderr);
        assert_eq!(parsed.duration_ms, original.duration_ms);
    }

    #[test]
    fn test_lune_run_result_with_signal() {
        // Test case for signal termination (e.g., SIGKILL = 9)
        let result = LuneRunResult {
            success: false,
            exit_code: None,
            signal: Some(9),
            stdout: None,
            stderr: None,
            duration_ms: 100,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("signal"));
        assert!(json.contains("9"));
        // exit_code should be skipped when None
        assert!(!json.contains("exit_code"));
    }
}
