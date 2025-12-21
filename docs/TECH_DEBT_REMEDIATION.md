# Technical Debt Remediation Plan

## Overview

This document outlines the implementation plan for addressing 4 technical debt items identified in the codebase analysis.

---

## Phase 1: Extract Timeout Constants

**Priority**: 🟡 Medium
**Effort**: 10 minutes
**Files**: `src/bridge/http.rs`, `src/bridge/mock.rs`

### Current State

Magic numbers scattered across `bridge/http.rs`:
```rust
// Line 46, 57, 163: heartbeat timeout
Duration::from_secs(10)

// Line 79, 86: command timeout
Duration::from_secs(30)
```

### Implementation

Add constants at module level in `src/bridge/http.rs`:

```rust
/// How long before a plugin heartbeat is considered stale
const PLUGIN_HEARTBEAT_TIMEOUT_SECS: u64 = 10;

/// Maximum time to wait for a plugin command response
const PLUGIN_COMMAND_TIMEOUT_SECS: u64 = 30;
```

Replace all usages:
- `Duration::from_secs(10)` → `Duration::from_secs(PLUGIN_HEARTBEAT_TIMEOUT_SECS)`
- `Duration::from_secs(30)` → `Duration::from_secs(PLUGIN_COMMAND_TIMEOUT_SECS)`

Update `src/bridge/mock.rs` to import and use the constant (or define its own for test isolation).

### Verification

```bash
cargo build && cargo test
grep -n "Duration::from_secs(10)\|Duration::from_secs(30)" src/bridge/
# Should return 0 matches in production code
```

---

## Phase 2: Resolve TODO Comment

**Priority**: 🟢 Low
**Effort**: 5 minutes
**Files**: `src/mcp/server.rs`

### Current State

Line 181 contains:
```rust
cloud_client: None, // TODO: would need type erasure for full generic support
```

### Analysis

The TODO describes a design limitation: `RobloxMcpServer<B, L>` is generic over `StudioBridge` and `Linter`, but `OpenCloudClient<H>` is also generic over `HttpClient`. Storing a mock cloud client would require either:

1. **Type erasure**: `Box<dyn CloudClient>` trait object (breaking change)
2. **Triple generic**: `RobloxMcpServer<B, L, C>` (complexity explosion)
3. **Separate test struct**: Test-only server variant (duplication)

### Decision: Document and Defer

The current design is acceptable because:
- Studio tools testing works fine without cloud client
- Cloud operations are tested via `OpenCloudClient` unit tests directly
- Full integration testing uses the production server with real HTTP

### Implementation

Replace TODO with documentation:

```rust
// Note: Cloud client is not injectable for testing because OpenCloudClient<H>
// would require a third generic parameter or trait object type erasure.
// Cloud operations are tested directly via OpenCloudClient unit tests.
// Studio tool testing works without cloud client injection.
cloud_client: None,
```

---

## Phase 3: Add Bootstrap Integration Test

**Priority**: 🟡 Medium
**Effort**: 15 minutes
**Files**: `tests/mcp_integration.rs`

### Current State

`main.rs` has 0% coverage (0/40 lines). The existing integration tests test MCP protocol but not the bootstrap sequence.

### Implementation

Add a new test that verifies server startup:

```rust
/// Test that the server starts up and responds to initialize
#[test]
#[ignore] // Requires compiled binary
fn test_server_bootstrap_and_initialize() {
    let mut client = McpTestClient::spawn().expect("Failed to spawn server");

    // Send initialize request (MCP protocol handshake)
    let response = client
        .send_request("initialize", Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })))
        .expect("Failed to send initialize");

    // Verify server responds with its info
    let result = response.result.expect("Expected result");
    assert!(result.get("serverInfo").is_some());
    assert!(result.get("protocolVersion").is_some());

    // Send initialized notification (completes handshake)
    client.send_notification("notifications/initialized", None)
        .expect("Failed to send initialized");

    // Verify we can list tools after initialization
    let tools_response = client
        .send_request("tools/list", None)
        .expect("Failed to list tools");

    let tools = tools_response.result
        .expect("Expected tools result")
        .get("tools")
        .expect("Expected tools array")
        .as_array()
        .expect("Tools should be array");

    // Should have all 24 tools
    assert!(tools.len() >= 20, "Expected at least 20 tools, got {}", tools.len());
}
```

Also add `send_notification` helper to `McpTestClient`:

```rust
fn send_notification(&mut self, method: &str, params: Option<Value>) -> Result<(), Box<dyn std::error::Error>> {
    let notification = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params.unwrap_or(json!({}))
    });

    let stdin = self.child.stdin.as_mut().ok_or("No stdin")?;
    writeln!(stdin, "{}", notification)?;
    stdin.flush()?;
    Ok(())
}
```

### Verification

```bash
cargo build && cargo test --test mcp_integration -- --ignored test_server_bootstrap
```

---

## Phase 4: Improve Linting Coverage

**Priority**: 🟡 Medium
**Effort**: 20 minutes
**Files**: `src/tools/linting.rs`

### Current State

`linting.rs` has only 4.88% coverage (2/41 lines). The `SeleneLinter` production implementation relies on subprocess execution which is hard to test without Selene installed.

### Strategy

Rather than mocking the subprocess (which would be complex), we add tests that:

1. Test the JSON parsing logic with fixture data
2. Test error handling paths
3. Verify `SeleneLinter` struct creation

### Implementation

Add tests to `src/tools/linting.rs` in the `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod selene_tests {
    use super::*;

    #[test]
    fn test_selene_linter_new() {
        let linter = SeleneLinter::new();
        // Verify it's Debug-printable
        let debug = format!("{:?}", linter);
        assert!(debug.contains("SeleneLinter"));
    }

    #[test]
    fn test_selene_linter_default() {
        let linter = SeleneLinter::default();
        let debug = format!("{:?}", linter);
        assert!(debug.contains("SeleneLinter"));
    }

    #[test]
    fn test_selene_linter_clone() {
        let linter = SeleneLinter::new();
        let cloned = linter.clone();
        // Both should be valid instances
        let _ = format!("{:?}", cloned);
    }

    #[test]
    fn test_selene_output_parsing() {
        // Test parsing actual Selene JSON output format
        let json = r#"{
            "diagnostics": [
                {
                    "severity": "Warning",
                    "code": "unused_variable",
                    "message": "x is assigned but never used",
                    "primary_label": {
                        "span": {
                            "start_line": 1,
                            "start_column": 7,
                            "end_line": 1,
                            "end_column": 8
                        }
                    }
                }
            ]
        }"#;

        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].code, "unused_variable");
        assert_eq!(output.diagnostics[0].primary_label.span.start_line, 1);
    }

    #[test]
    fn test_selene_output_empty() {
        let json = r#"{"diagnostics": []}"#;
        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn test_selene_output_missing_diagnostics() {
        // Default makes diagnostics vec empty if missing
        let json = r#"{}"#;
        let output: SeleneOutput = serde_json::from_str(json).unwrap();
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn test_lint_diagnostic_serialization() {
        let diag = LintDiagnostic {
            severity: "Warning".to_string(),
            code: "unused_variable".to_string(),
            message: "x is never used".to_string(),
            line: 10,
            column: 5,
            end_line: Some(10),
            end_column: Some(6),
        };

        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("unused_variable"));
        assert!(json.contains("Warning"));

        // Round-trip
        let parsed: LintDiagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.line, 10);
        assert_eq!(parsed.end_line, Some(10));
    }

    #[test]
    fn test_lint_result_serialization() {
        let result = LintResult {
            file_path: "test.luau".to_string(),
            diagnostics: vec![],
            error_count: 0,
            warning_count: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test.luau"));

        let parsed: LintResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.file_path, "test.luau");
    }
}
```

### Verification

```bash
cargo test linting::selene_tests
cargo tarpaulin --out Stdout | grep linting.rs
# Should show improved coverage
```

---

## Implementation Order

```
┌─────────────────────────────────────────────────────┐
│  Phase 1: Extract Timeout Constants (10 min)        │
│  └─ Low risk, immediate cleanup                     │
└────────────────────────┬────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│  Phase 2: Resolve TODO Comment (5 min)              │
│  └─ Documentation only, no code change              │
└────────────────────────┬────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│  Phase 3: Add Bootstrap Integration Test (15 min)   │
│  └─ New test file additions only                    │
└────────────────────────┬────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────┐
│  Phase 4: Improve Linting Coverage (20 min)         │
│  └─ New unit tests for parsing logic                │
└─────────────────────────────────────────────────────┘
```

---

## Success Criteria

After implementation:

| Metric | Before | After |
|--------|--------|-------|
| Magic number occurrences | 5 | 0 |
| TODO comments | 1 | 0 |
| `linting.rs` coverage | 4.88% | >50% |
| Bootstrap test | None | 1 integration test |

---

## Estimated Total Effort

| Phase | Effort |
|-------|--------|
| Phase 1 | 10 min |
| Phase 2 | 5 min |
| Phase 3 | 15 min |
| Phase 4 | 20 min |
| **Total** | **50 min** |
