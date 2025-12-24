//! Luau-LSP static analysis integration
//!
//! Provides type checking for Luau scripts using the luau-lsp analyze command.
//!
//! This module provides a trait-based abstraction for static analysis to enable testing
//! without requiring the external luau-lsp binary.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::error::RobloxMcpError;
use crate::tools::timeout::execute_with_timeout;

/// Result from analyzing Luau scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeResult {
    /// Root path that was analyzed
    pub path: String,
    /// List of diagnostics found
    pub diagnostics: Vec<AnalyzeDiagnostic>,
    /// Total error count
    pub error_count: usize,
    /// Total warning count
    pub warning_count: usize,
    /// Files analyzed count
    pub files_analyzed: usize,
}

/// Individual diagnostic from luau-lsp analyze
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeDiagnostic {
    /// Severity: "Error", "Warning", "Information", "Hint"
    pub severity: String,
    /// Diagnostic code (e.g., "TypeError", "UnknownGlobal")
    pub code: String,
    /// Human-readable message
    pub message: String,
    /// File path where diagnostic occurred
    pub file: String,
    /// Start line (1-indexed)
    pub start_line: u32,
    /// Start column (1-indexed)
    pub start_column: u32,
    /// End line (1-indexed)
    #[serde(default)]
    pub end_line: Option<u32>,
    /// End column (1-indexed)
    #[serde(default)]
    pub end_column: Option<u32>,
}

// ============================================================================
// Trait Abstraction
// ============================================================================

/// Abstraction over luau-lsp analyze operations for testability
///
/// This trait allows tests to inject mock implementations without requiring
/// the external luau-lsp binary to be installed.
#[async_trait]
pub trait LuauLspRunner: Send + Sync {
    /// Analyze Luau scripts for type errors and warnings
    ///
    /// # Arguments
    /// * `path` - Path to file or directory to analyze
    /// * `sourcemap_path` - Optional path to Rojo sourcemap.json
    /// * `definitions` - Optional paths to definition files (API types)
    async fn analyze(
        &self,
        path: &Path,
        sourcemap_path: Option<&Path>,
        definitions: &[&Path],
    ) -> Result<AnalyzeResult, RobloxMcpError>;
}

/// Production runner using luau-lsp CLI
///
/// Requires the `luau-lsp` binary to be installed and available in PATH.
/// Install via: `aftman add johnnymorganz/luau-lsp`
#[derive(Debug, Default, Clone)]
pub struct DefaultLuauLspRunner;

impl DefaultLuauLspRunner {
    /// Create a new DefaultLuauLspRunner instance
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LuauLspRunner for DefaultLuauLspRunner {
    async fn analyze(
        &self,
        path: &Path,
        sourcemap_path: Option<&Path>,
        definitions: &[&Path],
    ) -> Result<AnalyzeResult, RobloxMcpError> {
        analyze_path(path, sourcemap_path, definitions).await
    }
}

// ============================================================================
// Implementation
// ============================================================================

/// Run luau-lsp analyze on a path
///
/// # Arguments
/// * `path` - Path to file or directory to analyze
/// * `sourcemap_path` - Optional path to Rojo sourcemap.json for require resolution
/// * `definitions` - Optional paths to definition files (API types)
///
/// # Errors
/// Returns error if:
/// - luau-lsp is not installed
/// - Tool execution times out (default: 30 seconds)
async fn analyze_path(
    path: &Path,
    sourcemap_path: Option<&Path>,
    definitions: &[&Path],
) -> Result<AnalyzeResult, RobloxMcpError> {
    let mut cmd = Command::new("luau-lsp");
    cmd.arg("analyze");

    // Add sourcemap if provided (enables proper require resolution)
    if let Some(sourcemap) = sourcemap_path {
        cmd.arg(format!("--sourcemap={}", sourcemap.display()));
    }

    // Add definition files
    for def in definitions {
        cmd.arg(format!("--definitions={}", def.display()));
    }

    // Add the target path
    cmd.arg(path);

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Execute with timeout protection to prevent hanging
    let output = execute_with_timeout(cmd, "luau-lsp", None)
        .await
        .map_err(|e| {
            if e.to_string().contains("timed out") {
                e
            } else {
                RobloxMcpError::ToolNotInstalled {
                    tool: "luau-lsp".to_string(),
                    install_hint: "Install via: aftman add johnnymorganz/luau-lsp".to_string(),
                }
            }
        })?;

    parse_luau_lsp_output(&output.stdout, &output.stderr, path)
}

/// Parse luau-lsp output into AnalyzeResult
///
/// luau-lsp analyze outputs diagnostics in a plain text format by default:
/// `file.luau(line,col): severity: message`
///
/// This is extracted as a separate function to enable testing without
/// requiring the actual luau-lsp binary.
pub fn parse_luau_lsp_output(
    stdout: &[u8],
    stderr: &[u8],
    path: &Path,
) -> Result<AnalyzeResult, RobloxMcpError> {
    let stdout_str = String::from_utf8_lossy(stdout);
    let stderr_str = String::from_utf8_lossy(stderr);

    // Combine stdout and stderr - luau-lsp may output to either
    let combined_output = if stdout_str.is_empty() {
        stderr_str.to_string()
    } else {
        stdout_str.to_string()
    };

    let mut diagnostics = Vec::new();
    let mut files_seen = std::collections::HashSet::new();

    // Parse each line of output
    // Format: file.luau(line,col-endcol): severity: message
    // Or:     file.luau(line,col): severity: message
    for line in combined_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Try to parse as diagnostic line
        if let Some(diag) = parse_diagnostic_line(line) {
            files_seen.insert(diag.file.clone());
            diagnostics.push(diag);
        }
    }

    let error_count = diagnostics
        .iter()
        .filter(|d| d.severity.to_lowercase() == "error")
        .count();
    let warning_count = diagnostics
        .iter()
        .filter(|d| d.severity.to_lowercase() == "warning")
        .count();

    Ok(AnalyzeResult {
        path: path.display().to_string(),
        diagnostics,
        error_count,
        warning_count,
        files_analyzed: files_seen.len(),
    })
}

/// Parse a single diagnostic line from luau-lsp output
///
/// Format: `file.luau(line,col-endcol): severity: message`
/// Or:     `file.luau(line,col): severity: message`
fn parse_diagnostic_line(line: &str) -> Option<AnalyzeDiagnostic> {
    // Find the position info: (line,col) or (line,col-endcol)
    let paren_start = line.find('(')?;
    let paren_end = line.find(')')?;

    if paren_end <= paren_start {
        return None;
    }

    let file = line[..paren_start].to_string();
    let position_str = &line[paren_start + 1..paren_end];

    // Parse position: "line,col" or "line,col-endcol"
    let parts: Vec<&str> = position_str.split(',').collect();
    if parts.len() < 2 {
        return None;
    }

    let start_line: u32 = parts[0].trim().parse().ok()?;

    // Column might be "col" or "col-endcol"
    let col_part = parts[1].trim();
    let (start_column, end_column) = if col_part.contains('-') {
        let col_parts: Vec<&str> = col_part.split('-').collect();
        let start: u32 = col_parts[0].trim().parse().ok()?;
        let end: u32 = col_parts.get(1).and_then(|s| s.trim().parse().ok())?;
        (start, Some(end))
    } else {
        let col: u32 = col_part.parse().ok()?;
        (col, None)
    };

    // Find the severity and message after the closing paren
    // Format: "): severity: message"
    let rest = &line[paren_end + 1..];
    let rest = rest.trim_start_matches(':').trim();

    // Find severity (first word before the colon)
    let colon_pos = rest.find(':')?;
    let severity = rest[..colon_pos].trim().to_string();
    let message = rest[colon_pos + 1..].trim().to_string();

    // Extract code from message if present (e.g., "[TypeError]")
    let (code, clean_message) = if message.starts_with('[') {
        if let Some(bracket_end) = message.find(']') {
            let code = message[1..bracket_end].to_string();
            let msg = message[bracket_end + 1..].trim().to_string();
            (code, msg)
        } else {
            ("Unknown".to_string(), message)
        }
    } else {
        ("Unknown".to_string(), message)
    };

    Some(AnalyzeDiagnostic {
        severity,
        code,
        message: clean_message,
        file,
        start_line,
        start_column,
        end_line: Some(start_line), // luau-lsp doesn't provide end line in plain format
        end_column,
    })
}

// ============================================================================
// Mock for Testing
// ============================================================================

/// Mock luau-lsp runner for testing without the luau-lsp binary
///
/// Returns pre-configured results for testing various analysis scenarios.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Internal shared state for MockLuauLspRunner
    #[derive(Debug)]
    struct MockState {
        responses: VecDeque<Result<AnalyzeResult, RobloxMcpError>>,
        calls: Vec<MockAnalyzeCall>,
    }

    /// Recorded analyze call for verification
    #[derive(Debug, Clone)]
    pub struct MockAnalyzeCall {
        pub path: String,
        pub sourcemap_path: Option<String>,
        pub definitions: Vec<String>,
    }

    /// Mock luau-lsp runner for testing
    ///
    /// Clone is cheap - all clones share the same internal state via Arc.
    #[derive(Clone)]
    pub struct MockLuauLspRunner {
        state: Arc<Mutex<MockState>>,
    }

    impl MockLuauLspRunner {
        /// Create a new mock runner
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    responses: VecDeque::new(),
                    calls: Vec::new(),
                })),
            }
        }

        /// Queue a response to be returned
        pub fn queue_response(&self, response: Result<AnalyzeResult, RobloxMcpError>) {
            self.state.lock().unwrap().responses.push_back(response);
        }

        /// Get all recorded analyze calls
        pub fn calls(&self) -> Vec<MockAnalyzeCall> {
            self.state.lock().unwrap().calls.clone()
        }

        /// Get the number of times analyze was called
        pub fn call_count(&self) -> usize {
            self.state.lock().unwrap().calls.len()
        }

        /// Create a mock runner pre-configured with a clean result (no diagnostics)
        pub fn clean() -> Self {
            let mock = Self::new();
            mock.queue_response(Ok(AnalyzeResult {
                path: String::new(),
                diagnostics: vec![],
                error_count: 0,
                warning_count: 0,
                files_analyzed: 0,
            }));
            mock
        }

        /// Create a mock runner pre-configured with errors
        pub fn with_errors(errors: Vec<(&str, &str, &str, u32)>) -> Self {
            let mock = Self::new();
            let diagnostics: Vec<AnalyzeDiagnostic> = errors
                .into_iter()
                .map(|(file, code, message, line)| AnalyzeDiagnostic {
                    severity: "Error".to_string(),
                    code: code.to_string(),
                    message: message.to_string(),
                    file: file.to_string(),
                    start_line: line,
                    start_column: 1,
                    end_line: Some(line),
                    end_column: None,
                })
                .collect();

            let error_count = diagnostics.len();
            mock.queue_response(Ok(AnalyzeResult {
                path: String::new(),
                diagnostics,
                error_count,
                warning_count: 0,
                files_analyzed: 1,
            }));
            mock
        }

        /// Create a mock runner pre-configured with warnings
        pub fn with_warnings(warnings: Vec<(&str, &str, &str, u32)>) -> Self {
            let mock = Self::new();
            let diagnostics: Vec<AnalyzeDiagnostic> = warnings
                .into_iter()
                .map(|(file, code, message, line)| AnalyzeDiagnostic {
                    severity: "Warning".to_string(),
                    code: code.to_string(),
                    message: message.to_string(),
                    file: file.to_string(),
                    start_line: line,
                    start_column: 1,
                    end_line: Some(line),
                    end_column: None,
                })
                .collect();

            let warning_count = diagnostics.len();
            mock.queue_response(Ok(AnalyzeResult {
                path: String::new(),
                diagnostics,
                error_count: 0,
                warning_count,
                files_analyzed: 1,
            }));
            mock
        }
    }

    impl Default for MockLuauLspRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    impl std::fmt::Debug for MockLuauLspRunner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let state = self.state.lock().unwrap();
            f.debug_struct("MockLuauLspRunner")
                .field("queued_responses", &state.responses.len())
                .field("calls", &state.calls.len())
                .finish()
        }
    }

    #[async_trait]
    impl LuauLspRunner for MockLuauLspRunner {
        async fn analyze(
            &self,
            path: &Path,
            sourcemap_path: Option<&Path>,
            definitions: &[&Path],
        ) -> Result<AnalyzeResult, RobloxMcpError> {
            let mut state = self.state.lock().unwrap();

            // Record the call
            state.calls.push(MockAnalyzeCall {
                path: path.display().to_string(),
                sourcemap_path: sourcemap_path.map(|p| p.display().to_string()),
                definitions: definitions.iter().map(|p| p.display().to_string()).collect(),
            });

            // Return queued response or error if none queued
            state.responses.pop_front().unwrap_or_else(|| {
                Err(RobloxMcpError::ConfigError(
                    "MockLuauLspRunner: No response queued".into(),
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // AnalyzeResult Tests
    // ========================================

    #[test]
    fn test_analyze_result_serialization() {
        let result = AnalyzeResult {
            path: "src/".to_string(),
            diagnostics: vec![AnalyzeDiagnostic {
                severity: "Error".to_string(),
                code: "TypeError".to_string(),
                message: "Type mismatch".to_string(),
                file: "src/main.luau".to_string(),
                start_line: 10,
                start_column: 5,
                end_line: Some(10),
                end_column: Some(15),
            }],
            error_count: 1,
            warning_count: 0,
            files_analyzed: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("TypeError"));
        assert!(json.contains("error_count"));
    }

    #[test]
    fn test_analyze_result_deserialization() {
        let json = r#"{
            "path": "src/",
            "diagnostics": [],
            "error_count": 0,
            "warning_count": 0,
            "files_analyzed": 5
        }"#;

        let result: AnalyzeResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.path, "src/");
        assert_eq!(result.files_analyzed, 5);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_analyze_diagnostic_optional_fields() {
        let json = r#"{
            "severity": "Warning",
            "code": "UnusedVariable",
            "message": "Variable x is unused",
            "file": "test.luau",
            "start_line": 5,
            "start_column": 10
        }"#;

        let diag: AnalyzeDiagnostic = serde_json::from_str(json).unwrap();
        assert!(diag.end_line.is_none());
        assert!(diag.end_column.is_none());
    }

    // ========================================
    // parse_luau_lsp_output Tests
    // ========================================

    #[test]
    fn test_parse_empty_output() {
        let result = parse_luau_lsp_output(b"", b"", Path::new("src/"));
        assert!(result.is_ok());
        let res = result.unwrap();
        assert!(res.diagnostics.is_empty());
        assert_eq!(res.error_count, 0);
        assert_eq!(res.warning_count, 0);
    }

    #[test]
    fn test_parse_single_error() {
        let output = b"src/main.luau(10,5): Error: Type 'string' could not be converted into 'number'";
        let result = parse_luau_lsp_output(output, b"", Path::new("src/"));
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);
        assert_eq!(res.error_count, 1);
        assert_eq!(res.warning_count, 0);

        let diag = &res.diagnostics[0];
        assert_eq!(diag.severity, "Error");
        assert_eq!(diag.file, "src/main.luau");
        assert_eq!(diag.start_line, 10);
        assert_eq!(diag.start_column, 5);
    }

    #[test]
    fn test_parse_warning() {
        let output = b"src/utils.luau(25,1): Warning: Variable 'x' is unused";
        let result = parse_luau_lsp_output(output, b"", Path::new("src/"));
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);
        assert_eq!(res.error_count, 0);
        assert_eq!(res.warning_count, 1);
    }

    #[test]
    fn test_parse_multiple_diagnostics() {
        let output = b"src/a.luau(1,1): Error: First error\nsrc/b.luau(2,2): Warning: A warning\nsrc/a.luau(3,3): Error: Second error";
        let result = parse_luau_lsp_output(output, b"", Path::new("src/"));
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 3);
        assert_eq!(res.error_count, 2);
        assert_eq!(res.warning_count, 1);
        assert_eq!(res.files_analyzed, 2); // Two unique files
    }

    #[test]
    fn test_parse_with_column_range() {
        let output = b"test.luau(15,10-25): Error: Invalid syntax";
        let result = parse_luau_lsp_output(output, b"", Path::new("test.luau"));
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);

        let diag = &res.diagnostics[0];
        assert_eq!(diag.start_column, 10);
        assert_eq!(diag.end_column, Some(25));
    }

    #[test]
    fn test_parse_with_code_in_message() {
        let output = b"test.luau(1,1): Error: [TypeError] Type mismatch";
        let result = parse_luau_lsp_output(output, b"", Path::new("test.luau"));
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);

        let diag = &res.diagnostics[0];
        assert_eq!(diag.code, "TypeError");
        assert_eq!(diag.message, "Type mismatch");
    }

    #[test]
    fn test_parse_stderr_output() {
        // luau-lsp may output to stderr
        let result = parse_luau_lsp_output(
            b"",
            b"src/main.luau(1,1): Error: Parse error",
            Path::new("src/"),
        );
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);
    }

    #[test]
    fn test_parse_ignores_invalid_lines() {
        let output = b"Some random text\nsrc/main.luau(5,1): Error: Real error\nAnother random line";
        let result = parse_luau_lsp_output(output, b"", Path::new("src/"));
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);
    }

    #[test]
    fn test_parse_path_preserved() {
        let result = parse_luau_lsp_output(b"", b"", Path::new("my/custom/path"));
        assert!(result.is_ok());
        assert!(result.unwrap().path.contains("my"));
    }

    // ========================================
    // parse_diagnostic_line Tests
    // ========================================

    #[test]
    fn test_parse_diagnostic_line_basic() {
        let line = "file.luau(10,5): Error: Something went wrong";
        let diag = parse_diagnostic_line(line);
        assert!(diag.is_some());

        let d = diag.unwrap();
        assert_eq!(d.file, "file.luau");
        assert_eq!(d.start_line, 10);
        assert_eq!(d.start_column, 5);
        assert_eq!(d.severity, "Error");
        assert_eq!(d.message, "Something went wrong");
    }

    #[test]
    fn test_parse_diagnostic_line_with_path() {
        let line = "src/game/main.luau(1,1): Warning: Unused import";
        let diag = parse_diagnostic_line(line);
        assert!(diag.is_some());

        let d = diag.unwrap();
        assert_eq!(d.file, "src/game/main.luau");
    }

    #[test]
    fn test_parse_diagnostic_line_invalid() {
        assert!(parse_diagnostic_line("random text").is_none());
        assert!(parse_diagnostic_line("file.luau: no position").is_none());
        assert!(parse_diagnostic_line("").is_none());
    }

    // ========================================
    // DefaultLuauLspRunner Tests
    // ========================================

    #[test]
    fn test_default_runner_new() {
        let runner = DefaultLuauLspRunner::new();
        assert!(format!("{:?}", runner).contains("DefaultLuauLspRunner"));
    }

    #[test]
    fn test_default_runner_default() {
        let runner = DefaultLuauLspRunner;
        assert!(format!("{:?}", runner).contains("DefaultLuauLspRunner"));
    }

    #[test]
    fn test_default_runner_clone() {
        let runner = DefaultLuauLspRunner::new();
        let _ = runner.clone();
    }

    // ========================================
    // MockLuauLspRunner Tests
    // ========================================

    use mock::MockLuauLspRunner;

    #[tokio::test]
    async fn test_mock_runner_returns_queued_result() {
        let mock = MockLuauLspRunner::new();
        mock.queue_response(Ok(AnalyzeResult {
            path: "test/".to_string(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
            files_analyzed: 5,
        }));

        let result = mock.analyze(Path::new("test/"), None, &[]).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().files_analyzed, 5);
    }

    #[tokio::test]
    async fn test_mock_runner_records_calls() {
        let mock = MockLuauLspRunner::new();
        mock.queue_response(Ok(AnalyzeResult {
            path: String::new(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
            files_analyzed: 0,
        }));

        let sourcemap = Path::new("sourcemap.json");
        let defs = [Path::new("types/roblox.d.luau")];
        let def_refs: Vec<&Path> = defs.to_vec();

        mock.analyze(Path::new("src/"), Some(sourcemap), &def_refs)
            .await
            .unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].path.contains("src"));
        assert!(calls[0].sourcemap_path.is_some());
        assert_eq!(calls[0].definitions.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_runner_no_response_queued() {
        let mock = MockLuauLspRunner::new();

        let result = mock.analyze(Path::new("src/"), None, &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_runner_clean_helper() {
        let mock = MockLuauLspRunner::clean();

        let result = mock.analyze(Path::new("src/"), None, &[]).await;
        assert!(result.is_ok());

        let res = result.unwrap();
        assert!(res.diagnostics.is_empty());
        assert_eq!(res.error_count, 0);
    }

    #[tokio::test]
    async fn test_mock_runner_with_errors_helper() {
        let mock = MockLuauLspRunner::with_errors(vec![(
            "main.luau",
            "TypeError",
            "Type mismatch",
            10,
        )]);

        let result = mock.analyze(Path::new("src/"), None, &[]).await;
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 1);
        assert_eq!(res.error_count, 1);
        assert_eq!(res.diagnostics[0].severity, "Error");
    }

    #[tokio::test]
    async fn test_mock_runner_with_warnings_helper() {
        let mock = MockLuauLspRunner::with_warnings(vec![
            ("a.luau", "UnusedVar", "x is unused", 5),
            ("b.luau", "UnusedVar", "y is unused", 10),
        ]);

        let result = mock.analyze(Path::new("src/"), None, &[]).await;
        assert!(result.is_ok());

        let res = result.unwrap();
        assert_eq!(res.diagnostics.len(), 2);
        assert_eq!(res.warning_count, 2);
    }

    #[tokio::test]
    async fn test_mock_runner_call_count() {
        let mock = MockLuauLspRunner::new();
        for _ in 0..3 {
            mock.queue_response(Ok(AnalyzeResult {
                path: String::new(),
                diagnostics: vec![],
                error_count: 0,
                warning_count: 0,
                files_analyzed: 0,
            }));
        }

        for _ in 0..3 {
            mock.analyze(Path::new("src/"), None, &[]).await.unwrap();
        }

        assert_eq!(mock.call_count(), 3);
    }

    #[tokio::test]
    async fn test_mock_runner_clone_shares_state() {
        let mock1 = MockLuauLspRunner::new();
        mock1.queue_response(Ok(AnalyzeResult {
            path: String::new(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
            files_analyzed: 0,
        }));

        let mock2 = mock1.clone();

        // Use mock2 to make the call
        mock2.analyze(Path::new("src/"), None, &[]).await.unwrap();

        // Verify mock1 can see the call (shared state)
        assert_eq!(mock1.call_count(), 1);
    }

    #[test]
    fn test_mock_runner_debug() {
        let mock = MockLuauLspRunner::new();
        let debug = format!("{:?}", mock);
        assert!(debug.contains("MockLuauLspRunner"));
    }

    // ========================================
    // Round-trip Tests
    // ========================================

    #[test]
    fn test_analyze_result_round_trip() {
        let original = AnalyzeResult {
            path: "src/game/".to_string(),
            diagnostics: vec![
                AnalyzeDiagnostic {
                    severity: "Error".to_string(),
                    code: "E001".to_string(),
                    message: "Error message".to_string(),
                    file: "main.luau".to_string(),
                    start_line: 10,
                    start_column: 5,
                    end_line: Some(10),
                    end_column: Some(20),
                },
                AnalyzeDiagnostic {
                    severity: "Warning".to_string(),
                    code: "W001".to_string(),
                    message: "Warning message".to_string(),
                    file: "utils.luau".to_string(),
                    start_line: 25,
                    start_column: 1,
                    end_line: None,
                    end_column: None,
                },
            ],
            error_count: 1,
            warning_count: 1,
            files_analyzed: 2,
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: AnalyzeResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.path, original.path);
        assert_eq!(parsed.diagnostics.len(), original.diagnostics.len());
        assert_eq!(parsed.error_count, original.error_count);
        assert_eq!(parsed.warning_count, original.warning_count);
        assert_eq!(parsed.files_analyzed, original.files_analyzed);
    }
}
