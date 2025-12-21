//! Luau linting integration via Selene
//!
//! Provides code quality analysis for Luau scripts using the Selene linter.
//!
//! This module provides a trait-based abstraction for linting to enable testing
//! without requiring the external Selene binary.

use crate::error::RobloxMcpError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Result from linting a Luau script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResult {
    /// Path to the linted file
    pub file_path: String,
    /// List of lint diagnostics
    pub diagnostics: Vec<LintDiagnostic>,
    /// Number of errors found
    pub error_count: usize,
    /// Number of warnings found
    pub warning_count: usize,
}

/// Individual lint diagnostic from Selene
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintDiagnostic {
    /// Severity level ("error" or "warning")
    pub severity: String,
    /// Lint rule code (e.g., "unused_variable")
    pub code: String,
    /// Human-readable message
    pub message: String,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
    /// End line (if span available)
    #[serde(default)]
    pub end_line: Option<u32>,
    /// End column (if span available)
    #[serde(default)]
    pub end_column: Option<u32>,
}

/// Selene JSON output format (simplified)
#[derive(Debug, Deserialize)]
struct SeleneOutput {
    #[serde(default)]
    diagnostics: Vec<SeleneDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct SeleneDiagnostic {
    severity: String,
    code: String,
    message: String,
    primary_label: SeleneLabel,
}

#[derive(Debug, Deserialize)]
struct SeleneLabel {
    span: SeleneSpan,
}

#[derive(Debug, Deserialize)]
struct SeleneSpan {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

// ============================================================================
// Linter Trait Abstraction
// ============================================================================

/// Abstraction over linting operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external Selene binary to be installed.
#[async_trait]
pub trait Linter: Send + Sync {
    /// Lint a Luau script file
    ///
    /// # Arguments
    /// * `file_path` - Path to the .luau file to lint
    /// * `config_path` - Optional path to selene.toml configuration file
    ///
    /// # Returns
    /// LintResult containing diagnostics
    async fn lint(
        &self,
        file_path: &Path,
        config_path: Option<&Path>,
    ) -> Result<LintResult, RobloxMcpError>;
}

/// Production linter using Selene
///
/// Requires the `selene` binary to be installed and available in PATH.
/// Install via: `cargo install selene`
#[derive(Debug, Default, Clone)]
pub struct SeleneLinter;

impl SeleneLinter {
    /// Create a new SeleneLinter instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Linter for SeleneLinter {
    async fn lint(
        &self,
        file_path: &Path,
        config_path: Option<&Path>,
    ) -> Result<LintResult, RobloxMcpError> {
        lint_script(file_path, config_path).await
    }
}

// ============================================================================
// Mock Linter for Testing
// ============================================================================

/// Mock linter for testing without Selene binary
///
/// Returns pre-configured results for testing various lint scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockLinter
    #[derive(Debug)]
    struct MockState {
        /// Queue of results to return (FIFO)
        results: std::collections::VecDeque<Result<LintResult, RobloxMcpError>>,
        /// Record of lint calls for verification
        calls: Vec<MockLintCall>,
    }

    /// Mock linter that returns pre-configured results
    ///
    /// Clone is cheap - all clones share the same internal state via Arc.
    #[derive(Debug, Clone)]
    pub struct MockLinter {
        state: Arc<Mutex<MockState>>,
    }

    /// Recorded lint call for verification
    #[derive(Debug, Clone)]
    pub struct MockLintCall {
        pub file_path: String,
        pub config_path: Option<String>,
    }

    impl Default for MockLinter {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockLinter {
        /// Create a new mock linter
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    results: std::collections::VecDeque::new(),
                    calls: Vec::new(),
                })),
            }
        }

        /// Queue a successful result to be returned
        pub fn queue_result(&self, result: LintResult) {
            self.state.lock().unwrap().results.push_back(Ok(result));
        }

        /// Queue an error result to be returned
        pub fn queue_error(&self, error: RobloxMcpError) {
            self.state.lock().unwrap().results.push_back(Err(error));
        }

        /// Get all recorded lint calls
        pub fn calls(&self) -> Vec<MockLintCall> {
            self.state.lock().unwrap().calls.clone()
        }

        /// Check if lint was called for a specific file
        pub fn was_called_with(&self, file_path: &str) -> bool {
            self.state
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c.file_path == file_path)
        }

        /// Get the number of times lint was called
        pub fn call_count(&self) -> usize {
            self.state.lock().unwrap().calls.len()
        }

        /// Create a mock linter pre-configured with a clean result (no diagnostics)
        pub fn clean() -> Self {
            let mock = Self::new();
            mock.queue_result(LintResult {
                file_path: String::new(),
                diagnostics: vec![],
                error_count: 0,
                warning_count: 0,
            });
            mock
        }

        /// Create a mock linter pre-configured with warnings
        pub fn with_warnings(warnings: Vec<(&str, &str, u32)>) -> Self {
            let mock = Self::new();
            let diagnostics: Vec<LintDiagnostic> = warnings
                .into_iter()
                .map(|(code, message, line)| LintDiagnostic {
                    severity: "warning".to_string(),
                    code: code.to_string(),
                    message: message.to_string(),
                    line,
                    column: 1,
                    end_line: Some(line),
                    end_column: None,
                })
                .collect();

            let warning_count = diagnostics.len();
            mock.queue_result(LintResult {
                file_path: String::new(),
                diagnostics,
                error_count: 0,
                warning_count,
            });
            mock
        }

        /// Create a mock linter pre-configured with errors
        pub fn with_errors(errors: Vec<(&str, &str, u32)>) -> Self {
            let mock = Self::new();
            let diagnostics: Vec<LintDiagnostic> = errors
                .into_iter()
                .map(|(code, message, line)| LintDiagnostic {
                    severity: "error".to_string(),
                    code: code.to_string(),
                    message: message.to_string(),
                    line,
                    column: 1,
                    end_line: Some(line),
                    end_column: None,
                })
                .collect();

            let error_count = diagnostics.len();
            mock.queue_result(LintResult {
                file_path: String::new(),
                diagnostics,
                error_count,
                warning_count: 0,
            });
            mock
        }
    }

    #[async_trait]
    impl Linter for MockLinter {
        async fn lint(
            &self,
            file_path: &Path,
            config_path: Option<&Path>,
        ) -> Result<LintResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();

            // Record the call
            state.calls.push(MockLintCall {
                file_path: file_path.display().to_string(),
                config_path: config_path.map(|p| p.display().to_string()),
            });

            // Return queued result or error if none queued
            state.results.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockLinter: No result queued".into(),
                ))
            })
        }
    }
}

// ============================================================================
// Core Linting Implementation
// ============================================================================

/// Run Selene linter on a Luau file
///
/// # Arguments
/// * `file_path` - Path to the .luau file to lint
/// * `config_path` - Optional path to selene.toml configuration file
///
/// # Errors
/// Returns error if:
/// - Selene is not installed
/// - File cannot be read
/// - JSON parsing fails
pub async fn lint_script(
    file_path: &Path,
    config_path: Option<&Path>,
) -> Result<LintResult, RobloxMcpError> {
    let mut cmd = Command::new("selene");

    // Use JSON output format for structured results
    cmd.arg("--display-style=json2");

    // Add config if provided
    if let Some(config) = config_path {
        cmd.arg("--config").arg(config);
    }

    cmd.arg(file_path);

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd.output().await.map_err(|e| {
        RobloxMcpError::ConfigError(format!(
            "Failed to run selene: {}. Is selene installed? Install with: cargo install selene",
            e
        ))
    })?;

    // Selene returns non-zero on lint errors, but we still want the output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // If stderr has content and stdout is empty, selene failed to run
    if stdout.is_empty() && !stderr.is_empty() {
        return Err(RobloxMcpError::ConfigError(format!(
            "Selene error: {}",
            stderr.trim()
        )));
    }

    // Parse JSON output - each line is a separate JSON object
    let mut diagnostics = Vec::new();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Try to parse as Selene JSON format
        if let Ok(output) = serde_json::from_str::<SeleneOutput>(line) {
            for diag in output.diagnostics {
                diagnostics.push(LintDiagnostic {
                    severity: diag.severity,
                    code: diag.code,
                    message: diag.message,
                    line: diag.primary_label.span.start_line,
                    column: diag.primary_label.span.start_column,
                    end_line: Some(diag.primary_label.span.end_line),
                    end_column: Some(diag.primary_label.span.end_column),
                });
            }
        }
    }

    let error_count = diagnostics.iter().filter(|d| d.severity == "error").count();
    let warning_count = diagnostics
        .iter()
        .filter(|d| d.severity == "warning")
        .count();

    Ok(LintResult {
        file_path: file_path.display().to_string(),
        diagnostics,
        error_count,
        warning_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_result_serialization() {
        let result = LintResult {
            file_path: "test.luau".to_string(),
            diagnostics: vec![LintDiagnostic {
                severity: "warning".to_string(),
                code: "unused_variable".to_string(),
                message: "x is unused".to_string(),
                line: 5,
                column: 10,
                end_line: Some(5),
                end_column: Some(11),
            }],
            error_count: 0,
            warning_count: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("unused_variable"));
        assert!(json.contains("warning_count"));
    }

    #[test]
    fn test_lint_diagnostic_default_end_positions() {
        let json = r#"{
            "severity": "error",
            "code": "parse_error",
            "message": "Syntax error",
            "line": 1,
            "column": 1
        }"#;

        let diag: LintDiagnostic = serde_json::from_str(json).unwrap();
        assert!(diag.end_line.is_none());
        assert!(diag.end_column.is_none());
    }

    #[test]
    fn test_lint_result_with_multiple_diagnostics() {
        let result = LintResult {
            file_path: "script.luau".to_string(),
            diagnostics: vec![
                LintDiagnostic {
                    severity: "error".to_string(),
                    code: "syntax_error".to_string(),
                    message: "Unexpected token".to_string(),
                    line: 1,
                    column: 5,
                    end_line: Some(1),
                    end_column: Some(10),
                },
                LintDiagnostic {
                    severity: "warning".to_string(),
                    code: "unused_variable".to_string(),
                    message: "Variable 'x' is never used".to_string(),
                    line: 3,
                    column: 7,
                    end_line: Some(3),
                    end_column: Some(8),
                },
            ],
            error_count: 1,
            warning_count: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: LintResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.diagnostics.len(), 2);
        assert_eq!(parsed.error_count, 1);
        assert_eq!(parsed.warning_count, 1);
    }

    #[test]
    fn test_lint_result_empty_diagnostics() {
        let result = LintResult {
            file_path: "clean.luau".to_string(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: LintResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.error_count, 0);
    }

    #[test]
    fn test_selene_output_parsing() {
        let json = r#"{"diagnostics": []}"#;
        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn test_selene_output_with_diagnostics() {
        let json = r#"{
            "diagnostics": [{
                "severity": "warning",
                "code": "unused_variable",
                "message": "x is unused",
                "primary_label": {
                    "span": {
                        "start_line": 5,
                        "start_column": 10,
                        "end_line": 5,
                        "end_column": 11
                    }
                }
            }]
        }"#;
        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].code, "unused_variable");
        assert_eq!(output.diagnostics[0].primary_label.span.start_line, 5);
    }

    #[test]
    fn test_lint_diagnostic_deserialization() {
        let json = r#"{
            "severity": "warning",
            "code": "global_usage",
            "message": "Using global variable",
            "line": 10,
            "column": 1,
            "end_line": 10,
            "end_column": 5
        }"#;

        let diag: LintDiagnostic = serde_json::from_str(json).unwrap();
        assert_eq!(diag.severity, "warning");
        assert_eq!(diag.code, "global_usage");
        assert_eq!(diag.line, 10);
        assert_eq!(diag.end_line, Some(10));
    }

    #[test]
    fn test_selene_span_parsing() {
        let json = r#"{
            "start_line": 1,
            "start_column": 5,
            "end_line": 1,
            "end_column": 10
        }"#;
        let span: SeleneSpan = serde_json::from_str(json).unwrap();
        assert_eq!(span.start_line, 1);
        assert_eq!(span.start_column, 5);
        assert_eq!(span.end_line, 1);
        assert_eq!(span.end_column, 10);
    }

    #[test]
    fn test_selene_label_parsing() {
        let json = r#"{
            "span": {
                "start_line": 3,
                "start_column": 1,
                "end_line": 3,
                "end_column": 8
            }
        }"#;
        let label: SeleneLabel = serde_json::from_str(json).unwrap();
        assert_eq!(label.span.start_line, 3);
    }

    // ========================================
    // MockLinter Tests
    // ========================================
    use mock::MockLinter;

    #[tokio::test]
    async fn test_mock_linter_returns_queued_result() {
        let mock = MockLinter::new();
        mock.queue_result(LintResult {
            file_path: "test.luau".to_string(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        });

        let result = mock.lint(Path::new("test.luau"), None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().file_path, "test.luau");
    }

    #[tokio::test]
    async fn test_mock_linter_returns_queued_error() {
        let mock = MockLinter::new();
        mock.queue_error(RobloxMcpError::ConfigError("Test error".into()));

        let result = mock.lint(Path::new("test.luau"), None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_linter_records_calls() {
        let mock = MockLinter::new();
        mock.queue_result(LintResult {
            file_path: String::new(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        });

        mock.lint(
            Path::new("/path/to/script.luau"),
            Some(Path::new("/path/to/selene.toml")),
        )
        .await
        .unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].file_path.contains("script.luau"));
        assert!(calls[0]
            .config_path
            .as_ref()
            .unwrap()
            .contains("selene.toml"));
    }

    #[tokio::test]
    async fn test_mock_linter_was_called_with() {
        let mock = MockLinter::new();
        mock.queue_result(LintResult {
            file_path: String::new(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        });
        mock.queue_result(LintResult {
            file_path: String::new(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        });

        mock.lint(Path::new("src/game.luau"), None).await.unwrap();
        mock.lint(Path::new("src/utils.luau"), None).await.unwrap();

        // was_called_with should match the file path as displayed
        assert!(mock.was_called_with("src/game.luau") || mock.was_called_with("src\\game.luau"));
        assert!(mock.was_called_with("src/utils.luau") || mock.was_called_with("src\\utils.luau"));
        assert!(!mock.was_called_with("src/other.luau"));
        assert!(!mock.was_called_with("nonexistent.luau"));
    }

    #[tokio::test]
    async fn test_mock_linter_fifo_order() {
        let mock = MockLinter::new();
        mock.queue_result(LintResult {
            file_path: "first.luau".to_string(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        });
        mock.queue_result(LintResult {
            file_path: "second.luau".to_string(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        });

        let r1 = mock.lint(Path::new("a.luau"), None).await.unwrap();
        let r2 = mock.lint(Path::new("b.luau"), None).await.unwrap();

        assert_eq!(r1.file_path, "first.luau");
        assert_eq!(r2.file_path, "second.luau");
    }

    #[tokio::test]
    async fn test_mock_linter_clean_helper() {
        let mock = MockLinter::clean();

        let result = mock.lint(Path::new("clean.luau"), None).await.unwrap();

        assert!(result.diagnostics.is_empty());
        assert_eq!(result.error_count, 0);
        assert_eq!(result.warning_count, 0);
    }

    #[tokio::test]
    async fn test_mock_linter_with_warnings_helper() {
        let mock = MockLinter::with_warnings(vec![
            ("unused_variable", "x is unused", 5),
            ("shadowing", "y shadows outer variable", 10),
        ]);

        let result = mock.lint(Path::new("script.luau"), None).await.unwrap();

        assert_eq!(result.diagnostics.len(), 2);
        assert_eq!(result.warning_count, 2);
        assert_eq!(result.error_count, 0);
        assert_eq!(result.diagnostics[0].code, "unused_variable");
        assert_eq!(result.diagnostics[0].line, 5);
        assert_eq!(result.diagnostics[1].code, "shadowing");
        assert_eq!(result.diagnostics[1].line, 10);
    }

    #[tokio::test]
    async fn test_mock_linter_with_errors_helper() {
        let mock = MockLinter::with_errors(vec![
            ("syntax_error", "Unexpected token", 1),
            ("parse_error", "Failed to parse", 3),
        ]);

        let result = mock.lint(Path::new("script.luau"), None).await.unwrap();

        assert_eq!(result.diagnostics.len(), 2);
        assert_eq!(result.error_count, 2);
        assert_eq!(result.warning_count, 0);
        assert_eq!(result.diagnostics[0].severity, "error");
    }

    #[tokio::test]
    async fn test_mock_linter_no_result_queued() {
        let mock = MockLinter::new();

        let result = mock.lint(Path::new("test.luau"), None).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::ConfigError(msg) => {
                assert!(msg.contains("No result queued"));
            }
            e => panic!("Expected ConfigError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_mock_linter_call_count() {
        let mock = MockLinter::new();
        for _ in 0..5 {
            mock.queue_result(LintResult {
                file_path: String::new(),
                diagnostics: vec![],
                error_count: 0,
                warning_count: 0,
            });
        }

        for i in 0..5 {
            mock.lint(Path::new(&format!("script{i}.luau")), None)
                .await
                .unwrap();
        }

        assert_eq!(mock.call_count(), 5);
    }

    #[test]
    fn test_selene_linter_new() {
        let linter = SeleneLinter::new();
        let debug = format!("{:?}", linter);
        assert!(debug.contains("SeleneLinter"));
    }

    #[test]
    fn test_selene_linter_default() {
        let linter = SeleneLinter::default();
        let _ = format!("{:?}", linter);
    }

    #[test]
    fn test_selene_linter_clone() {
        let linter = SeleneLinter::new();
        let cloned = linter.clone();
        let _ = format!("{:?}", cloned);
    }

    // ========================================
    // SeleneOutput Edge Cases
    // ========================================

    #[test]
    fn test_selene_output_missing_diagnostics_field() {
        // When diagnostics field is missing, #[serde(default)] should make it empty
        let json = r#"{}"#;
        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn test_selene_output_multiple_diagnostics() {
        let json = r#"{
            "diagnostics": [
                {
                    "severity": "error",
                    "code": "syntax_error",
                    "message": "Unexpected token",
                    "primary_label": {
                        "span": { "start_line": 1, "start_column": 1, "end_line": 1, "end_column": 5 }
                    }
                },
                {
                    "severity": "warning",
                    "code": "unused_variable",
                    "message": "x is unused",
                    "primary_label": {
                        "span": { "start_line": 10, "start_column": 7, "end_line": 10, "end_column": 8 }
                    }
                }
            ]
        }"#;
        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.diagnostics.len(), 2);
        assert_eq!(output.diagnostics[0].severity, "error");
        assert_eq!(output.diagnostics[1].severity, "warning");
    }

    #[test]
    fn test_selene_diagnostic_parsing() {
        let json = r#"{
            "severity": "Warning",
            "code": "shadowing",
            "message": "Variable shadows outer scope",
            "primary_label": {
                "span": {
                    "start_line": 5,
                    "start_column": 11,
                    "end_line": 5,
                    "end_column": 12
                }
            }
        }"#;
        let diag: SeleneDiagnostic = serde_json::from_str(json).unwrap();
        assert_eq!(diag.severity, "Warning");
        assert_eq!(diag.code, "shadowing");
        assert_eq!(diag.primary_label.span.start_line, 5);
        assert_eq!(diag.primary_label.span.end_column, 12);
    }

    #[test]
    fn test_lint_result_round_trip() {
        let result = LintResult {
            file_path: "path/to/script.luau".to_string(),
            diagnostics: vec![LintDiagnostic {
                severity: "error".to_string(),
                code: "E001".to_string(),
                message: "Error message".to_string(),
                line: 42,
                column: 10,
                end_line: Some(42),
                end_column: Some(20),
            }],
            error_count: 1,
            warning_count: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: LintResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.file_path, result.file_path);
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, 42);
        assert_eq!(parsed.error_count, 1);
    }

    #[test]
    fn test_lint_diagnostic_with_all_optional_fields() {
        let diag = LintDiagnostic {
            severity: "warning".to_string(),
            code: "test_code".to_string(),
            message: "Test message".to_string(),
            line: 1,
            column: 1,
            end_line: Some(2),
            end_column: Some(10),
        };

        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"end_line\":2"));
        assert!(json.contains("\"end_column\":10"));
    }

    #[test]
    fn test_lint_diagnostic_without_optional_fields() {
        let diag = LintDiagnostic {
            severity: "error".to_string(),
            code: "parse_error".to_string(),
            message: "Parse failed".to_string(),
            line: 1,
            column: 1,
            end_line: None,
            end_column: None,
        };

        let json = serde_json::to_string(&diag).unwrap();
        // Optional fields should still be present but null
        let parsed: LintDiagnostic = serde_json::from_str(&json).unwrap();
        assert!(parsed.end_line.is_none());
        assert!(parsed.end_column.is_none());
    }
}
