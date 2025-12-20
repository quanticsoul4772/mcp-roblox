//! Luau linting integration via Selene
//!
//! Provides code quality analysis for Luau scripts using the Selene linter.

use crate::error::RobloxMcpError;
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
}
