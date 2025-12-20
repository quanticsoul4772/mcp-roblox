# Test Utilities Design Document

## Overview

This document describes a plan to exercise the mock infrastructure API utilities that are currently marked with `#[allow(dead_code)]`. These utilities provide complete test coverage capabilities and should be exercised through targeted tests.

## Current State Analysis

### Mock Utilities Requiring Tests

| Location | Utility | Purpose |
|----------|---------|---------|
| `src/http/mock.rs:81` | `MockRequest.body` | Captured request body for assertion |
| `src/http/mock.rs:115` | `MockHttpClient::last_request()` | Convenience accessor for most recent request |
| `src/http/mock.rs:121` | `MockHttpClient::clear_requests()` | Reset recorded requests between test phases |
| `src/tools/linting.rs:187` | `MockLinter::was_called_with()` | File-specific call verification |
| `src/mcp/server.rs:140` | `with_mock_bridge_and_linter()` | Full dependency injection for server tests |
| `src/mcp/server.rs:1686` | `create_mock_server()` | Simplified server creation helper |

### Already Well-Tested Utilities (Reference)

The `MockBridge` in `src/bridge/mock.rs` demonstrates the pattern we should follow:
- `last_call()` - tested in `test_mock_bridge_last_call`
- `clear_calls()` - tested in `test_mock_bridge_clear_calls`
- `was_called()` - tested in `test_mock_bridge_was_called`
- `call_count()` - tested in `test_mock_bridge_call_count`

---

## Implementation Plan

### Phase 1: MockHttpClient Utility Tests

**File**: `src/http/mock.rs`

#### Test 1.1: `test_mock_request_body_captured`
```rust
#[tokio::test]
async fn test_mock_request_body_captured() {
    let mock = MockHttpClient::new();
    mock.queue_response(MockResponse::success(200, b"ok"));

    let body = serde_json::json!({"key": "value", "count": 42});
    mock.post_json("http://test.com/api", &[], body.clone()).await.unwrap();

    let requests = mock.requests();
    assert_eq!(requests.len(), 1);

    let captured_body = requests[0].body.as_ref().expect("body should be captured");
    let parsed: serde_json::Value = serde_json::from_slice(captured_body).unwrap();
    assert_eq!(parsed, body);
}
```

**Purpose**: Verifies that POST request bodies are properly captured for assertion.

#### Test 1.2: `test_mock_request_body_binary`
```rust
#[tokio::test]
async fn test_mock_request_body_binary() {
    let mock = MockHttpClient::new();
    mock.queue_response(MockResponse::success(200, b"ok"));

    let binary_data = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
    mock.post_binary("http://test.com/upload", &[], binary_data.clone(), None)
        .await
        .unwrap();

    let requests = mock.requests();
    let captured = requests[0].body.as_ref().unwrap();
    assert_eq!(captured, &binary_data);
}
```

**Purpose**: Verifies binary data capture works correctly.

#### Test 1.3: `test_mock_client_last_request`
```rust
#[tokio::test]
async fn test_mock_client_last_request() {
    let mock = MockHttpClient::new();
    mock.queue_responses([
        MockResponse::success(200, b"first"),
        MockResponse::success(200, b"second"),
        MockResponse::success(200, b"third"),
    ]);

    mock.get("http://test.com/1", &[]).await.unwrap();
    mock.get("http://test.com/2", &[]).await.unwrap();
    mock.get("http://test.com/3", &[]).await.unwrap();

    let last = mock.last_request().expect("should have last request");
    assert_eq!(last.url, "http://test.com/3");
    assert_eq!(last.method, "GET");
}
```

**Purpose**: Verifies `last_request()` returns the most recent request.

#### Test 1.4: `test_mock_client_last_request_empty`
```rust
#[test]
fn test_mock_client_last_request_empty() {
    let mock = MockHttpClient::new();
    assert!(mock.last_request().is_none());
}
```

**Purpose**: Verifies `last_request()` returns `None` when no requests made.

#### Test 1.5: `test_mock_client_clear_requests`
```rust
#[tokio::test]
async fn test_mock_client_clear_requests() {
    let mock = MockHttpClient::new();
    mock.queue_responses([
        MockResponse::success(200, b"1"),
        MockResponse::success(200, b"2"),
    ]);

    mock.get("http://test.com/1", &[]).await.unwrap();
    mock.get("http://test.com/2", &[]).await.unwrap();
    assert_eq!(mock.requests().len(), 2);

    mock.clear_requests();
    assert_eq!(mock.requests().len(), 0);
    assert!(mock.last_request().is_none());
}
```

**Purpose**: Verifies `clear_requests()` resets the recorded requests.

#### Test 1.6: `test_mock_client_clear_requests_preserves_responses`
```rust
#[tokio::test]
async fn test_mock_client_clear_requests_preserves_responses() {
    let mock = MockHttpClient::new();
    mock.queue_responses([
        MockResponse::success(200, b"1"),
        MockResponse::success(200, b"2"),
    ]);

    mock.get("http://test.com/1", &[]).await.unwrap();
    mock.clear_requests();

    // Queued responses should still be available
    let response = mock.get("http://test.com/2", &[]).await.unwrap();
    assert_eq!(response.body, b"2");
}
```

**Purpose**: Verifies `clear_requests()` only clears requests, not queued responses.

---

### Phase 2: MockLinter Utility Tests

**File**: `src/tools/linting.rs`

#### Test 2.1: `test_mock_linter_was_called_with_match`
```rust
#[test]
fn test_mock_linter_was_called_with_match() {
    let mock = MockLinter::clean();

    // Simulate lint being called (via the Linter trait)
    // We need to actually call lint() to record the call
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let _ = mock.lint("src/scripts/game.lua", None).await;
    });

    assert!(mock.was_called_with("src/scripts/game.lua"));
    assert!(!mock.was_called_with("src/scripts/other.lua"));
}
```

**Purpose**: Verifies `was_called_with()` correctly matches file paths.

#### Test 2.2: `test_mock_linter_was_called_with_multiple_files`
```rust
#[tokio::test]
async fn test_mock_linter_was_called_with_multiple_files() {
    let mock = MockLinter::new();
    mock.queue_result(LintResult {
        file_path: String::new(),
        diagnostics: vec![],
        error_count: 0,
        warning_count: 0
    });
    mock.queue_result(LintResult {
        file_path: String::new(),
        diagnostics: vec![],
        error_count: 0,
        warning_count: 0
    });

    mock.lint("file1.lua", None).await.unwrap();
    mock.lint("file2.lua", None).await.unwrap();

    assert!(mock.was_called_with("file1.lua"));
    assert!(mock.was_called_with("file2.lua"));
    assert!(!mock.was_called_with("file3.lua"));
}
```

**Purpose**: Verifies `was_called_with()` works across multiple files.

---

### Phase 3: Server Test Utility Tests

**File**: `src/mcp/server.rs` (in `#[cfg(test)]` module)

#### Test 3.1: `test_with_mock_bridge_and_linter`
```rust
#[tokio::test]
async fn test_with_mock_bridge_and_linter() {
    let temp_dir = tempfile::tempdir().unwrap();
    let project_root = temp_dir.path().to_path_buf();

    // Create test file for linting
    let script_path = project_root.join("test.lua");
    std::fs::write(&script_path, "local x = 1").unwrap();

    let mock_bridge = Arc::new(MockBridge::new());
    let mock_linter = MockLinter::with_warnings(vec![
        ("unused_variable", "x is never used", 1)
    ]);

    let server = RobloxMcpServer::with_mock_bridge_and_linter(
        mock_bridge.clone(),
        project_root,
        mock_linter.clone(),
    );

    // Verify linter injection works
    let params = FsLintScriptParams {
        file_path: "test.lua".to_string(),
        config_path: None,
    };
    let result = server.fs_lint_script_impl(params).await.unwrap();

    // Verify custom linter was used
    assert!(mock_linter.was_called_with("test.lua") || mock_linter.call_count() > 0);
}
```

**Purpose**: Validates that `with_mock_bridge_and_linter()` correctly injects both dependencies.

#### Test 3.2: `test_create_mock_server_basic`
```rust
#[tokio::test]
async fn test_create_mock_server_basic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let server = create_mock_server(temp_dir.path().to_path_buf());

    // Verify server is functional
    let info = server.get_info();
    assert_eq!(info.name, "roblox-studio-mcp");
}
```

**Purpose**: Verifies the simplified `create_mock_server()` helper produces a working server.

---

## Implementation Order

```
┌─────────────────────────────────────────────────────────┐
│  Phase 1: MockHttpClient Tests (6 tests)                │
│  └─ body, last_request, clear_requests                  │
│     Location: src/http/mock.rs                          │
│     Dependencies: None                                  │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  Phase 2: MockLinter Tests (2 tests)                    │
│  └─ was_called_with                                     │
│     Location: src/tools/linting.rs                      │
│     Dependencies: Phase 1 (pattern established)         │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  Phase 3: Server Test Utilities (2 tests)               │
│  └─ with_mock_bridge_and_linter, create_mock_server     │
│     Location: src/mcp/server.rs                         │
│     Dependencies: Phase 1, Phase 2                      │
└─────────────────────────────────────────────────────────┘
```

---

## Success Criteria

After implementation:

1. **All `#[allow(dead_code)]` annotations can be removed** from:
   - `MockRequest.body`
   - `MockHttpClient::last_request()`
   - `MockHttpClient::clear_requests()`
   - `MockLinter::was_called_with()`
   - `with_mock_bridge_and_linter()`
   - `create_mock_server()`

2. **`cargo test` produces 0 warnings**

3. **Test count increases by 10 tests** (6 + 2 + 2)

4. **All new tests pass** and provide meaningful coverage

---

## Estimated Effort

| Phase | Tests | Complexity | Estimate |
|-------|-------|------------|----------|
| Phase 1 | 6 | Low | 10 min |
| Phase 2 | 2 | Low | 5 min |
| Phase 3 | 2 | Medium | 10 min |
| **Total** | **10** | | **25 min** |

---

## Notes

- Tests follow existing patterns in the codebase
- All tests are unit tests (no integration dependencies)
- Tests validate both success and edge cases
- After implementation, remove `#[allow(dead_code)]` annotations to verify tests exercise the code
