//! Rojo project management integration
//!
//! Provides Rojo build and sourcemap generation for Roblox projects.
//!
//! This module provides a trait-based abstraction for Rojo operations to enable testing
//! without requiring the external Rojo binary.

use crate::error::RobloxMcpError;
use crate::tools::timeout::execute_with_timeout;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Result from building a Rojo project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RojoBuildResult {
    /// Path to the output file
    pub output_path: String,
    /// Whether the build succeeded
    pub success: bool,
    /// Stdout from the build process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Stderr from the build process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

/// Result from generating a Rojo sourcemap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RojoSourcemapResult {
    /// The sourcemap content (JSON)
    pub sourcemap: String,
    /// Path to the sourcemap file if saved
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    /// Whether the sourcemap generation succeeded
    pub success: bool,
}

// ============================================================================
// Rojo Trait Abstraction
// ============================================================================

/// Abstraction over Rojo operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external Rojo binary to be installed.
#[async_trait]
pub trait RojoRunner: Send + Sync {
    /// Build a Rojo project to an output file
    ///
    /// # Arguments
    /// * `project_path` - Path to the project directory or *.project.json file
    /// * `output_path` - Path where the built .rbxl/.rbxlx file should be saved
    async fn build(
        &self,
        project_path: &Path,
        output_path: &Path,
    ) -> Result<RojoBuildResult, RobloxMcpError>;

    /// Generate a sourcemap for a Rojo project
    ///
    /// # Arguments
    /// * `project_path` - Path to the project directory or *.project.json file
    /// * `output_path` - Optional path to save the sourcemap file
    async fn sourcemap(
        &self,
        project_path: &Path,
        output_path: Option<&Path>,
    ) -> Result<RojoSourcemapResult, RobloxMcpError>;
}

/// Production Rojo runner using the rojo CLI
///
/// Requires the `rojo` binary to be installed and available in PATH.
/// Install via: `aftman install rojo-rbx/rojo`
#[derive(Debug, Default, Clone)]
pub struct DefaultRojoRunner;

impl DefaultRojoRunner {
    /// Create a new DefaultRojoRunner instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RojoRunner for DefaultRojoRunner {
    async fn build(
        &self,
        project_path: &Path,
        output_path: &Path,
    ) -> Result<RojoBuildResult, RobloxMcpError> {
        build_project(project_path, output_path).await
    }

    async fn sourcemap(
        &self,
        project_path: &Path,
        output_path: Option<&Path>,
    ) -> Result<RojoSourcemapResult, RobloxMcpError> {
        generate_sourcemap(project_path, output_path).await
    }
}

// ============================================================================
// Implementation Functions
// ============================================================================

/// Build a Rojo project using the rojo CLI
///
/// # Errors
/// Returns error if:
/// - Rojo is not installed
/// - Tool execution times out (default: 30 seconds)
async fn build_project(
    project_path: &Path,
    output_path: &Path,
) -> Result<RojoBuildResult, RobloxMcpError> {
    let mut cmd = Command::new("rojo");
    cmd.arg("build")
        .arg(project_path)
        .arg("--output")
        .arg(output_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Execute with timeout protection
    let output = execute_with_timeout(cmd, "rojo", None).await.map_err(|e| {
        if e.to_string().contains("timed out") {
            e
        } else {
            RobloxMcpError::ToolNotInstalled {
                tool: "rojo".to_string(),
                install_hint: "Install via: aftman install rojo-rbx/rojo".to_string(),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(RojoBuildResult {
        output_path: output_path.display().to_string(),
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

/// Generate a sourcemap using the rojo CLI
///
/// # Errors
/// Returns error if:
/// - Rojo is not installed
/// - Tool execution times out (default: 30 seconds)
async fn generate_sourcemap(
    project_path: &Path,
    output_path: Option<&Path>,
) -> Result<RojoSourcemapResult, RobloxMcpError> {
    let mut cmd = Command::new("rojo");
    cmd.arg("sourcemap").arg(project_path);

    if let Some(out) = output_path {
        cmd.arg("--output").arg(out);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Execute with timeout protection
    let output = execute_with_timeout(cmd, "rojo", None).await.map_err(|e| {
        if e.to_string().contains("timed out") {
            e
        } else {
            RobloxMcpError::ToolNotInstalled {
                tool: "rojo".to_string(),
                install_hint: "Install via: aftman install rojo-rbx/rojo".to_string(),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(RobloxMcpError::ToolExecutionError {
            tool: "rojo sourcemap".to_string(),
            message: if stderr.is_empty() {
                "Sourcemap generation failed".to_string()
            } else {
                stderr
            },
        });
    }

    Ok(RojoSourcemapResult {
        sourcemap: stdout,
        output_path: output_path.map(|p| p.display().to_string()),
        success: true,
    })
}

// ============================================================================
// Mock Rojo Runner for Testing
// ============================================================================

/// Mock Rojo runner for testing without the rojo binary
///
/// Returns pre-configured results for testing various Rojo scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockRojoRunner
    #[derive(Debug)]
    struct MockState {
        build_responses: VecDeque<Result<RojoBuildResult, RobloxMcpError>>,
        sourcemap_responses: VecDeque<Result<RojoSourcemapResult, RobloxMcpError>>,
        build_calls: Vec<(String, String)>,
        sourcemap_calls: Vec<(String, Option<String>)>,
    }

    /// Mock Rojo runner for testing
    #[derive(Clone)]
    pub struct MockRojoRunner {
        state: Arc<Mutex<MockState>>,
    }

    impl MockRojoRunner {
        /// Create a new mock runner
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    build_responses: VecDeque::new(),
                    sourcemap_responses: VecDeque::new(),
                    build_calls: Vec::new(),
                    sourcemap_calls: Vec::new(),
                })),
            }
        }

        /// Queue a build response
        pub fn queue_build_response(&self, response: Result<RojoBuildResult, RobloxMcpError>) {
            self.state
                .lock()
                .unwrap()
                .build_responses
                .push_back(response);
        }

        /// Queue a sourcemap response
        pub fn queue_sourcemap_response(
            &self,
            response: Result<RojoSourcemapResult, RobloxMcpError>,
        ) {
            self.state
                .lock()
                .unwrap()
                .sourcemap_responses
                .push_back(response);
        }

        /// Get all build calls made
        pub fn build_calls(&self) -> Vec<(String, String)> {
            self.state.lock().unwrap().build_calls.clone()
        }

        /// Get all sourcemap calls made
        pub fn sourcemap_calls(&self) -> Vec<(String, Option<String>)> {
            self.state.lock().unwrap().sourcemap_calls.clone()
        }
    }

    impl Default for MockRojoRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for MockRojoRunner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let state = self.state.lock().unwrap();
            f.debug_struct("MockRojoRunner")
                .field("queued_build_responses", &state.build_responses.len())
                .field(
                    "queued_sourcemap_responses",
                    &state.sourcemap_responses.len(),
                )
                .field("build_calls", &state.build_calls.len())
                .field("sourcemap_calls", &state.sourcemap_calls.len())
                .finish()
        }
    }

    #[async_trait]
    impl RojoRunner for MockRojoRunner {
        async fn build(
            &self,
            project_path: &Path,
            output_path: &Path,
        ) -> Result<RojoBuildResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state.build_calls.push((
                project_path.display().to_string(),
                output_path.display().to_string(),
            ));

            state.build_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockRojoRunner: No build response queued".into(),
                ))
            })
        }

        async fn sourcemap(
            &self,
            project_path: &Path,
            output_path: Option<&Path>,
        ) -> Result<RojoSourcemapResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state.sourcemap_calls.push((
                project_path.display().to_string(),
                output_path.map(|p| p.display().to_string()),
            ));

            state.sourcemap_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockRojoRunner: No sourcemap response queued".into(),
                ))
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_runner_build() {
            let mock = MockRojoRunner::new();
            mock.queue_build_response(Ok(RojoBuildResult {
                output_path: "/output/game.rbxl".to_string(),
                success: true,
                stdout: Some("Built successfully".to_string()),
                stderr: None,
            }));

            let result = mock
                .build(Path::new("/project"), Path::new("/output/game.rbxl"))
                .await
                .unwrap();

            assert!(result.success);
            assert_eq!(result.output_path, "/output/game.rbxl");
            assert_eq!(mock.build_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_sourcemap() {
            let mock = MockRojoRunner::new();
            mock.queue_sourcemap_response(Ok(RojoSourcemapResult {
                sourcemap: r#"{"name": "test"}"#.to_string(),
                output_path: None,
                success: true,
            }));

            let result = mock.sourcemap(Path::new("/project"), None).await.unwrap();

            assert!(result.success);
            assert_eq!(result.sourcemap, r#"{"name": "test"}"#);
            assert_eq!(mock.sourcemap_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_no_response_queued() {
            let mock = MockRojoRunner::new();

            let result = mock
                .build(Path::new("/project"), Path::new("/output/game.rbxl"))
                .await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_runner_clone_shares_state() {
            let mock1 = MockRojoRunner::new();
            mock1.queue_build_response(Ok(RojoBuildResult {
                output_path: "/output/game.rbxl".to_string(),
                success: true,
                stdout: None,
                stderr: None,
            }));

            let mock2 = mock1.clone();

            // Use mock2 to make the call
            mock2
                .build(Path::new("/project"), Path::new("/output/game.rbxl"))
                .await
                .unwrap();

            // Verify mock1 can see the call (shared state)
            assert_eq!(mock1.build_calls().len(), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rojo_build_result_serialization() {
        let result = RojoBuildResult {
            output_path: "/output/game.rbxl".to_string(),
            success: true,
            stdout: Some("Built successfully".to_string()),
            stderr: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("output_path"));
        assert!(json.contains("success"));
        assert!(json.contains("stdout"));
        // stderr should be skipped when None
        assert!(!json.contains("stderr"));
    }

    #[test]
    fn test_rojo_sourcemap_result_serialization() {
        let result = RojoSourcemapResult {
            sourcemap: r#"{"name": "test"}"#.to_string(),
            output_path: Some("/output/sourcemap.json".to_string()),
            success: true,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("sourcemap"));
        assert!(json.contains("output_path"));
        assert!(json.contains("success"));
    }

    #[test]
    fn test_default_rojo_runner_new() {
        let runner = DefaultRojoRunner::new();
        assert!(format!("{:?}", runner).contains("DefaultRojoRunner"));
    }

    #[test]
    fn test_rojo_build_result_clone() {
        let result = RojoBuildResult {
            output_path: "/output/game.rbxl".to_string(),
            success: true,
            stdout: None,
            stderr: None,
        };

        let cloned = result.clone();
        assert_eq!(cloned.output_path, result.output_path);
        assert_eq!(cloned.success, result.success);
    }

    #[test]
    fn test_rojo_sourcemap_result_clone() {
        let result = RojoSourcemapResult {
            sourcemap: r#"{"name": "test"}"#.to_string(),
            output_path: None,
            success: true,
        };

        let cloned = result.clone();
        assert_eq!(cloned.sourcemap, result.sourcemap);
        assert_eq!(cloned.success, result.success);
    }
}
