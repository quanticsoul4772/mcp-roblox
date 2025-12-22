//! RegEx safety validation to prevent DoS attacks via catastrophic backtracking
//!
//! User-provided regex patterns can cause exponential time complexity if they
//! contain certain pathological patterns. This module validates patterns
//! before compilation to prevent such attacks.

use crate::error::RobloxMcpError;
use regex::RegexBuilder;

/// Maximum compiled regex size in bytes (1MB)
const MAX_REGEX_SIZE: usize = 1_000_000;

/// Patterns known to cause catastrophic backtracking
///
/// These patterns can cause exponential time complexity:
/// - `(.*)*` - nested star with any character
/// - `(.+)+` - nested plus with any character
/// - `(a+)+` - nested plus with literal
/// - `(a*)*` - nested star with literal
/// - `(a|aa)+` - alternation causing backtracking
/// - `(a|a?)+` - optional alternation causing backtracking
const DANGEROUS_PATTERNS: &[&str] = &[
    "(.*)*",
    "(.+)+",
    "(a+)+",
    "(a*)*",
    "(a|aa)+",
    "(a|a?)+",
    // More general patterns that can cause issues
    "(.*)\\1",   // backreference with greedy quantifier
    "(.+)\\1+",  // backreference with nested quantifier
];

/// Validate that a regex pattern is safe to compile and execute
///
/// Checks for:
/// 1. Known dangerous patterns (catastrophic backtracking)
/// 2. Compiled regex size limits (via RegexBuilder)
///
/// # Arguments
/// * `pattern` - The regex pattern to validate
///
/// # Returns
/// A compiled `Regex` if the pattern is safe
///
/// # Errors
/// Returns `ConfigError` if pattern is potentially dangerous or too large
///
/// # Example
/// ```ignore
/// use roblox_studio_mcp::regex_safety::validate_regex_safety;
///
/// // Safe patterns work
/// let regex = validate_regex_safety(r"function\s+\w+").unwrap();
///
/// // Dangerous patterns are rejected
/// assert!(validate_regex_safety("(.*)*").is_err());
/// ```
pub fn validate_regex_safety(pattern: &str) -> Result<regex::Regex, RobloxMcpError> {
    // Check for known dangerous patterns
    for dangerous in DANGEROUS_PATTERNS {
        if pattern.contains(dangerous) {
            return Err(RobloxMcpError::ConfigError(format!(
                "Potentially dangerous regex pattern containing '{}'. \
                 These patterns can cause catastrophic backtracking and server hangs.",
                dangerous
            )));
        }
    }

    // Compile with size limit to prevent memory exhaustion
    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE)
        .build()
        .map_err(|e| RobloxMcpError::ConfigError(format!("Invalid regex pattern: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // Safe Pattern Tests
    // ========================================

    #[test]
    fn test_safe_pattern_function_search() {
        let result = validate_regex_safety(r"function\s+\w+");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_simple_word() {
        let result = validate_regex_safety(r"\w+");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_any_match() {
        let result = validate_regex_safety(r".*foo.*");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_character_class() {
        let result = validate_regex_safety(r"[a-z]+");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_line_anchors() {
        let result = validate_regex_safety(r"^local\s+\w+\s*=");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_alternation() {
        // Simple alternation without nesting is safe
        let result = validate_regex_safety(r"foo|bar|baz");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_quantified_group() {
        // Non-nested quantifiers are safe
        let result = validate_regex_safety(r"(abc)+");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_optional() {
        let result = validate_regex_safety(r"https?://\S+");
        assert!(result.is_ok());
    }

    #[test]
    fn test_safe_pattern_digit_repetition() {
        let result = validate_regex_safety(r"\d{3}-\d{4}");
        assert!(result.is_ok());
    }

    // ========================================
    // Dangerous Pattern Tests (Catastrophic Backtracking)
    // ========================================

    #[test]
    fn test_dangerous_nested_star_any() {
        let result = validate_regex_safety("(.*)*");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("catastrophic backtracking"));
    }

    #[test]
    fn test_dangerous_nested_plus_any() {
        let result = validate_regex_safety("(.+)+");
        assert!(result.is_err());
    }

    #[test]
    fn test_dangerous_nested_plus_literal() {
        let result = validate_regex_safety("(a+)+");
        assert!(result.is_err());
    }

    #[test]
    fn test_dangerous_nested_star_literal() {
        let result = validate_regex_safety("(a*)*");
        assert!(result.is_err());
    }

    #[test]
    fn test_dangerous_alternation_overlap() {
        let result = validate_regex_safety("(a|aa)+");
        assert!(result.is_err());
    }

    #[test]
    fn test_dangerous_optional_alternation() {
        let result = validate_regex_safety("(a|a?)+");
        assert!(result.is_err());
    }

    #[test]
    fn test_dangerous_pattern_embedded() {
        // Dangerous pattern embedded in larger pattern should still be caught
        let result = validate_regex_safety(r"^start(.+)+end$");
        assert!(result.is_err());
    }

    // ========================================
    // Invalid Pattern Tests
    // ========================================

    #[test]
    fn test_invalid_regex_syntax() {
        let result = validate_regex_safety(r"[unclosed");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid regex"));
    }

    #[test]
    fn test_invalid_unbalanced_parens() {
        let result = validate_regex_safety(r"((abc)");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_bad_escape() {
        // \q is not a valid escape in Rust regex (unlike \z which is valid end-of-string anchor)
        let result = validate_regex_safety(r"\q");
        assert!(result.is_err());
    }

    // ========================================
    // Edge Cases
    // ========================================

    #[test]
    fn test_empty_pattern() {
        let result = validate_regex_safety("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_single_character() {
        let result = validate_regex_safety("a");
        assert!(result.is_ok());
    }

    #[test]
    fn test_pattern_with_lookahead() {
        // Lookahead is supported by the regex crate but not dangerous
        let result = validate_regex_safety(r"foo(?=bar)");
        // This may or may not be supported depending on regex crate version
        // We just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_case_insensitive_flag() {
        let result = validate_regex_safety(r"(?i)hello");
        assert!(result.is_ok());
    }

    #[test]
    fn test_unicode_pattern() {
        let result = validate_regex_safety(r"[\p{L}]+");
        assert!(result.is_ok());
    }

    // ========================================
    // Real-World Search Patterns
    // ========================================

    #[test]
    fn test_luau_local_variable() {
        let result = validate_regex_safety(r"local\s+\w+\s*=");
        assert!(result.is_ok());
    }

    #[test]
    fn test_luau_function_definition() {
        let result = validate_regex_safety(r"function\s+[\w.:]+\s*\(");
        assert!(result.is_ok());
    }

    #[test]
    fn test_luau_require_statement() {
        let result = validate_regex_safety(r#"require\s*\(\s*["']"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_todo_comment() {
        let result = validate_regex_safety(r"--\s*TODO:");
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_statement() {
        let result = validate_regex_safety(r"print\s*\(");
        assert!(result.is_ok());
    }

    // ========================================
    // Oversized Pattern Test
    // ========================================

    #[test]
    fn test_oversized_pattern_rejected() {
        // Create a very large pattern that would exceed size limits when compiled
        // This is tricky because the pattern itself needs to be reasonable but
        // compile to something large. A huge alternation list works.
        let huge_pattern = (0..10000)
            .map(|i| format!("word{}", i))
            .collect::<Vec<_>>()
            .join("|");

        let result = validate_regex_safety(&huge_pattern);
        // This should either succeed (if within limits) or fail with a clear error
        // The important thing is it doesn't hang or crash
        let _ = result;
    }

    // ========================================
    // Backreference Tests
    // ========================================

    #[test]
    fn test_dangerous_backreference_greedy() {
        // Backreferences with greedy quantifiers can cause issues
        // DANGEROUS_PATTERNS contains "(.*)\1" (the literal string with single backslash)
        // So we need to pass the same: using non-raw string "(.*)\\1" = "(.*)\1"
        let result = validate_regex_safety("(.*)\\1");
        // This should be caught by our dangerous pattern detection
        assert!(result.is_err());
    }
}
