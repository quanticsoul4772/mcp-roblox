//! Wally package manager integration
//!
//! Provides Wally package installation and update operations for Roblox projects.
//!
//! This module provides a trait-based abstraction for Wally operations to enable testing
//! without requiring the external Wally binary.

use crate::error::RobloxMcpError;
use crate::tools::timeout::execute_with_timeout;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Result from installing Wally packages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallyInstallResult {
    /// Directory where packages were installed
    pub packages_dir: String,
    /// Whether the installation succeeded
    pub success: bool,
    /// Stdout from the install process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Stderr from the install process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

/// Result from updating Wally packages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallyUpdateResult {
    /// Whether the update succeeded
    pub success: bool,
    /// Stdout from the update process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Stderr from the update process
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

// ============================================================================
// Wally Trait Abstraction
// ============================================================================

/// Abstraction over Wally operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external Wally binary to be installed.
#[async_trait]
pub trait WallyRunner: Send + Sync {
    /// Install packages from wally.toml
    ///
    /// # Arguments
    /// * `project_path` - Path to the project directory containing wally.toml
    async fn install(&self, project_path: &Path) -> Result<WallyInstallResult, RobloxMcpError>;

    /// Update packages to latest compatible versions
    ///
    /// # Arguments
    /// * `project_path` - Path to the project directory containing wally.toml
    async fn update(&self, project_path: &Path) -> Result<WallyUpdateResult, RobloxMcpError>;
}

/// Production Wally runner using the wally CLI
///
/// Requires the `wally` binary to be installed and available in PATH.
/// Install via: `aftman install UpliftGames/wally`
#[derive(Debug, Default, Clone)]
pub struct DefaultWallyRunner;

impl DefaultWallyRunner {
    /// Create a new DefaultWallyRunner instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl WallyRunner for DefaultWallyRunner {
    async fn install(&self, project_path: &Path) -> Result<WallyInstallResult, RobloxMcpError> {
        install_packages(project_path).await
    }

    async fn update(&self, project_path: &Path) -> Result<WallyUpdateResult, RobloxMcpError> {
        update_packages(project_path).await
    }
}

// ============================================================================
// Implementation Functions
// ============================================================================

/// Install Wally packages using the wally CLI
///
/// # Errors
/// Returns error if:
/// - Wally is not installed
/// - Tool execution times out (default: 30 seconds)
async fn install_packages(project_path: &Path) -> Result<WallyInstallResult, RobloxMcpError> {
    let mut cmd = Command::new("wally");
    cmd.arg("install")
        .current_dir(project_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Execute with timeout protection
    let output = execute_with_timeout(cmd, "wally", None).await.map_err(|e| {
        if e.to_string().contains("timed out") {
            e
        } else {
            RobloxMcpError::ToolNotInstalled {
                tool: "wally".to_string(),
                install_hint: "Install via: aftman install UpliftGames/wally".to_string(),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Default packages directory is "Packages" in project root
    let packages_dir = project_path.join("Packages").display().to_string();

    Ok(WallyInstallResult {
        packages_dir,
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

/// Update Wally packages using the wally CLI
///
/// # Errors
/// Returns error if:
/// - Wally is not installed
/// - Tool execution times out (default: 30 seconds)
async fn update_packages(project_path: &Path) -> Result<WallyUpdateResult, RobloxMcpError> {
    let mut cmd = Command::new("wally");
    cmd.arg("update")
        .current_dir(project_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Execute with timeout protection
    let output = execute_with_timeout(cmd, "wally", None).await.map_err(|e| {
        if e.to_string().contains("timed out") {
            e
        } else {
            RobloxMcpError::ToolNotInstalled {
                tool: "wally".to_string(),
                install_hint: "Install via: aftman install UpliftGames/wally".to_string(),
            }
        }
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(WallyUpdateResult {
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
// Mock Wally Runner for Testing
// ============================================================================

/// Mock Wally runner for testing without the wally binary
///
/// Returns pre-configured results for testing various Wally scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockWallyRunner
    #[derive(Debug)]
    struct MockState {
        install_responses: VecDeque<Result<WallyInstallResult, RobloxMcpError>>,
        update_responses: VecDeque<Result<WallyUpdateResult, RobloxMcpError>>,
        install_calls: Vec<String>,
        update_calls: Vec<String>,
    }

    /// Mock Wally runner for testing
    #[derive(Clone)]
    pub struct MockWallyRunner {
        state: Arc<Mutex<MockState>>,
    }

    impl MockWallyRunner {
        /// Create a new mock runner
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    install_responses: VecDeque::new(),
                    update_responses: VecDeque::new(),
                    install_calls: Vec::new(),
                    update_calls: Vec::new(),
                })),
            }
        }

        /// Queue an install response
        pub fn queue_install_response(
            &self,
            response: Result<WallyInstallResult, RobloxMcpError>,
        ) {
            self.state
                .lock()
                .unwrap()
                .install_responses
                .push_back(response);
        }

        /// Queue an update response
        pub fn queue_update_response(&self, response: Result<WallyUpdateResult, RobloxMcpError>) {
            self.state
                .lock()
                .unwrap()
                .update_responses
                .push_back(response);
        }

        /// Get all install calls made
        pub fn install_calls(&self) -> Vec<String> {
            self.state.lock().unwrap().install_calls.clone()
        }

        /// Get all update calls made
        pub fn update_calls(&self) -> Vec<String> {
            self.state.lock().unwrap().update_calls.clone()
        }
    }

    impl Default for MockWallyRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for MockWallyRunner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let state = self.state.lock().unwrap();
            f.debug_struct("MockWallyRunner")
                .field("queued_install_responses", &state.install_responses.len())
                .field("queued_update_responses", &state.update_responses.len())
                .field("install_calls", &state.install_calls.len())
                .field("update_calls", &state.update_calls.len())
                .finish()
        }
    }

    #[async_trait]
    impl WallyRunner for MockWallyRunner {
        async fn install(&self, project_path: &Path) -> Result<WallyInstallResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state
                .install_calls
                .push(project_path.display().to_string());

            state.install_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockWallyRunner: No install response queued".into(),
                ))
            })
        }

        async fn update(&self, project_path: &Path) -> Result<WallyUpdateResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();
            state.update_calls.push(project_path.display().to_string());

            state.update_responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockWallyRunner: No update response queued".into(),
                ))
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_runner_install() {
            let mock = MockWallyRunner::new();
            mock.queue_install_response(Ok(WallyInstallResult {
                packages_dir: "/project/Packages".to_string(),
                success: true,
                stdout: Some("Installed 3 packages".to_string()),
                stderr: None,
            }));

            let result = mock.install(Path::new("/project")).await.unwrap();

            assert!(result.success);
            assert_eq!(result.packages_dir, "/project/Packages");
            assert_eq!(mock.install_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_update() {
            let mock = MockWallyRunner::new();
            mock.queue_update_response(Ok(WallyUpdateResult {
                success: true,
                stdout: Some("Updated 2 packages".to_string()),
                stderr: None,
            }));

            let result = mock.update(Path::new("/project")).await.unwrap();

            assert!(result.success);
            assert_eq!(mock.update_calls().len(), 1);
        }

        #[tokio::test]
        async fn test_mock_runner_no_response_queued() {
            let mock = MockWallyRunner::new();

            let result = mock.install(Path::new("/project")).await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_mock_runner_clone_shares_state() {
            let mock1 = MockWallyRunner::new();
            mock1.queue_install_response(Ok(WallyInstallResult {
                packages_dir: "/project/Packages".to_string(),
                success: true,
                stdout: None,
                stderr: None,
            }));

            let mock2 = mock1.clone();

            // Use mock2 to make the call
            mock2.install(Path::new("/project")).await.unwrap();

            // Verify mock1 can see the call (shared state)
            assert_eq!(mock1.install_calls().len(), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wally_install_result_serialization() {
        let result = WallyInstallResult {
            packages_dir: "/project/Packages".to_string(),
            success: true,
            stdout: Some("Installed 3 packages".to_string()),
            stderr: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("packages_dir"));
        assert!(json.contains("success"));
        assert!(json.contains("stdout"));
        // stderr should be skipped when None
        assert!(!json.contains("stderr"));
    }

    #[test]
    fn test_wally_update_result_serialization() {
        let result = WallyUpdateResult {
            success: true,
            stdout: Some("Updated 2 packages".to_string()),
            stderr: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("stdout"));
        // stderr should be skipped when None
        assert!(!json.contains("stderr"));
    }

    #[test]
    fn test_default_wally_runner_new() {
        let runner = DefaultWallyRunner::new();
        assert!(format!("{:?}", runner).contains("DefaultWallyRunner"));
    }

    #[test]
    fn test_wally_install_result_clone() {
        let result = WallyInstallResult {
            packages_dir: "/project/Packages".to_string(),
            success: true,
            stdout: None,
            stderr: None,
        };

        let cloned = result.clone();
        assert_eq!(cloned.packages_dir, result.packages_dir);
        assert_eq!(cloned.success, result.success);
    }

    #[test]
    fn test_wally_update_result_clone() {
        let result = WallyUpdateResult {
            success: true,
            stdout: None,
            stderr: None,
        };

        let cloned = result.clone();
        assert_eq!(cloned.success, result.success);
    }
}
