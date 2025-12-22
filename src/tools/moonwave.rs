//! Moonwave documentation generator integration
//!
//! Provides Moonwave documentation generation for Roblox/Luau projects.
//!
//! This module provides a trait-based abstraction for Moonwave operations to enable testing
//! without requiring the external Moonwave binary.

use crate::error::RobloxMcpError;
use crate::tools::timeout::execute_with_timeout;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Result from building Moonwave documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoonwaveBuildResult {
    /// Directory where documentation was generated
    pub output_dir: String,
    /// Whether the build succeeded
    pub success: bool,
    /// Stdout from the build process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Stderr from the build process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

// ============================================================================
// Moonwave Trait Abstraction
// ============================================================================

/// Abstraction over Moonwave operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external Moonwave binary to be installed.
#[async_trait]
pub trait MoonwaveRunner: Send + Sync {
    /// Build documentation from source files
    ///
    /// # Arguments
    /// * `project_path` - Path to the project directory containing moonwave.toml
    /// * `output_dir` - Optional output directory for the generated docs
    async fn build(
        &self,
        project_path: &Path,
        output_dir: Option<&Path>,
    ) -> Result<MoonwaveBuildResult, RobloxMcpError>;
}

/// Production Moonwave runner using the moonwave CLI
///
/// Requires the `moonwave` binary to be installed and available in PATH.
/// Install via: `npm install -g moonwave`
#[derive(Debug, Default, Clone)]
pub struct DefaultMoonwaveRunner;

impl DefaultMoonwaveRunner {
    /// Create a new DefaultMoonwaveRunner instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MoonwaveRunner for DefaultMoonwaveRunner {
    async fn build(
        &self,
        project_path: &Path,
        output_dir: Option<&Path>,
    ) -> Result<MoonwaveBuildResult, RobloxMcpError> {
        build_docs(project_path, output_dir).await
    }
}

// ============================================================================
// Implementation Functions
// ============================================================================

/// Build Moonwave documentation using the moonwave CLI
///
/// # Errors
/// Returns error if:
/// - Moonwave is not installed
/// - Tool execution times out (default: 30 seconds)
async fn build_docs(
    project_path: &Path,
    output_dir: Option<&Path>,
) -> Result<MoonwaveBuildResult, RobloxMcpError> {
    let mut cmd = Command::new("moonwave");
    cmd.arg("build").current_dir(project_path);

    // Add output directory if specified
    if let Some(out) = output_dir {
        cmd.arg("--out").arg(out);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Execute with timeout protection
    let output = execute_with_timeout(cmd, "moonwave", None).await.map_err(|e| {
        if e.to_string().contains("timed out") {
            e
        } else {
            RobloxMcpError::ToolNotInstalled {
                tool: "moonwave".to_string(),
                install_hint: "Install via: npm install -g moonwave".to_string(),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Default output directory is "build" in project root
    let output_path = output_dir
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| project_path.join("build").display().to_string());

    Ok(MoonwaveBuildResult {
        output_dir: output_path,
        success: output.status.success(),
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
    })
}

// ============================================================================
// Mock Moonwave Runner for Testing
// ============================================================================

/// Mock Moonwave runner for testing without the moonwave binary
///
/// Returns pre-configured results for testing various Moonwave scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockMoonwaveRunner
    #[derive(Debug)]
    struct MockState {
        build_responses: VecDeque<Result<MoonwaveBuildResult, RobloxMcpError>>,
        build_calls: Vec<(String, Option<String>)>,
    }

    /// Mock Moonwave runner for testing
    #[derive(Clone)]
    pub struct MockMoonwaveRunner {
        state: Arc<Mutex<MockState>>,
    }

    impl MockMoonwaveRunner {
        /// Create a new mock runner
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    build_responses: VecDeque::new(),
                    build_calls: Vec::new(),
                })),
            }
        }

        /// Queue a build response
        pub fn queue_build_response(&self, response: Result<MoonwaveBuildResult, RobloxMcpError>) {
            self.state
                .lock()
                .unwrap()
                .build_responses
                .push_back(response);
        }

        /// Get all build calls made
        pub fn build_calls(&self) -> Vec<(String, Option<String>)> {
            self.state.lock().unwrap().build_calls.clone()
        }
    }

    impl Default for MockMoonwaveRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for MockMoonwaveRunner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let state = self.state.lock().unwrap();
            f.debug_struct("MockMoonwaveRunner")
                .field("queued_build_responses", &state.build_responses.len())
                .field("build_calls", &state.build_calls.len())
                .finish()
        }
    }

    #[async_trait]
    impl MoonwaveRunner for MockMoonwaveRunner {
        async fn build(
            &self,
            project_path: &Path,
            output_dir: Option<&Path>,
        ) -> Result<MoonwaveBuildResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state.build_calls.push((
                project_path.display().to_string(),
                output_dir.map(|p| p.display().to_string()),
            ));

            state.build_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockMoonwaveRunner: No build response queued".into(),
                ))
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_runner_build() {
            let mock = MockMoonwaveRunner::new();
            mock.queue_build_response(Ok(MoonwaveBuildResult {
                output_dir: "/project/build".to_string(),
                success: true,
                stdout: Some("Documentation generated".to_string()),
                stderr: None,
            }));

            let result = mock.build(Path::new("/project"), None).await.unwrap();

            assert!(result.success);
            assert_eq!(result.output_dir, "/project/build");
            assert_eq!(mock.build_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_build_with_output() {
            let mock = MockMoonwaveRunner::new();
            mock.queue_build_response(Ok(MoonwaveBuildResult {
                output_dir: "/custom/output".to_string(),
                success: true,
                stdout: None,
                stderr: None,
            }));

            let result = mock
                .build(Path::new("/project"), Some(Path::new("/custom/output")))
                .await
                .unwrap();

            assert!(result.success);
            let calls = mock.build_calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].1, Some("/custom/output".to_string()));
        }

        #[tokio::test]
        async fn test_mock_runner_no_response_queued() {
            let mock = MockMoonwaveRunner::new();

            let result = mock.build(Path::new("/project"), None).await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_runner_clone_shares_state() {
            let mock1 = MockMoonwaveRunner::new();
            mock1.queue_build_response(Ok(MoonwaveBuildResult {
                output_dir: "/project/build".to_string(),
                success: true,
                stdout: None,
                stderr: None,
            }));

            let mock2 = mock1.clone();

            // Use mock2 to make the call
            mock2.build(Path::new("/project"), None).await.unwrap();

            // Verify mock1 can see the call (shared state)
            assert_eq!(mock1.build_calls().len(), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moonwave_build_result_serialization() {
        let result = MoonwaveBuildResult {
            output_dir: "/project/build".to_string(),
            success: true,
            stdout: Some("Documentation generated".to_string()),
            stderr: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("output_dir"));
        assert!(json.contains("success"));
        assert!(json.contains("stdout"));
        // stderr should be skipped when None
        assert!(!json.contains("stderr"));
    }

    #[test]
    fn test_default_moonwave_runner_new() {
        let runner = DefaultMoonwaveRunner::new();
        assert!(format!("{:?}", runner).contains("DefaultMoonwaveRunner"));
    }

    #[test]
    fn test_moonwave_build_result_clone() {
        let result = MoonwaveBuildResult {
            output_dir: "/project/build".to_string(),
            success: true,
            stdout: None,
            stderr: None,
        };

        let cloned = result.clone();
        assert_eq!(cloned.output_dir, result.output_dir);
        assert_eq!(cloned.success, result.success);
    }
}
