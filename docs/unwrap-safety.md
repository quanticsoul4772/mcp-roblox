# Unwrap Safety Guidelines

This document describes the error handling philosophy used in roblox-studio-mcp and when various patterns are acceptable.

## Overview

This project follows a strict "no silent failures" philosophy. All error handling is explicit, and potentially fallible operations use proper Result propagation.

## Safe Patterns (Acceptable)

### 1. `unwrap_or(default)` for Optional Parameters

All optional tool parameters use `.unwrap_or(default)` which **cannot panic**:

```rust
// Safe: provides documented default value
let max_depth = params.max_depth.unwrap_or(5);
let limit = params.limit.unwrap_or(100);
let check_only = params.check_only.unwrap_or(false);
```

**Locations**: `server.rs`, tool modules in `mcp/tools/`

### 2. `unwrap_or_default()` for Non-Critical Defaults

Used when an empty/default value is acceptable:

```rust
// Safe: empty string if parsing fails
let name: String = row.get("name").unwrap_or_default();
```

**Locations**: `ai/knowledge_graph.rs`

### 3. `.unwrap()` in Test Code

Tests appropriately use `.unwrap()` for assertions:

```rust
#[test]
fn test_something() {
    let result = function_under_test().unwrap(); // Panics on failure = test fails
    assert_eq!(result, expected);
}
```

**Locations**: All `#[cfg(test)]` modules, `tests/` directory

## Unsafe Patterns (Never Used in Production)

### 1. Raw `.unwrap()` on Fallible Operations

**NEVER** use in production code:

```rust
// BAD: Will panic if file doesn't exist
let content = fs::read_to_string(path).unwrap();

// GOOD: Propagate error to caller
let content = fs::read_to_string(path)
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
```

### 2. `.expect("message")` on Runtime Operations

**NEVER** use for operations that can legitimately fail:

```rust
// BAD: Panics with message
let client = client.expect("client should be configured");

// GOOD: Return clear error
let client = self.cloud_client.as_ref().ok_or_else(|| {
    ErrorData::internal_error(
        "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY not set".to_string(),
        None,
    )
})?;
```

## Error Handling Patterns

### Tool Implementations

All tools follow this pattern:

```rust
pub(crate) async fn tool_impl(&self, params: Params) -> Result<CallToolResult, ErrorData> {
    // Validate inputs
    let validated = validate_something(input)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    // Perform operation
    let result = do_something(validated).await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    // Return success
    Ok(CallToolResult::success(vec![Content::text(...)]))
}
```

### Cloud API Operations

All cloud operations wrap errors properly:

```rust
let response = client.get(url).await
    .map_err(|e| RobloxMcpError::CloudApiError(format!("Request failed: {}", e)))?;
```

### Optional Feature Access

For optional capabilities (cloud client, knowledge graph):

```rust
pub(crate) fn cloud(&self) -> Result<&Arc<dyn CloudClient>, ErrorData> {
    self.cloud_client.as_ref().ok_or_else(|| {
        ErrorData::internal_error(
            "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY not set".to_string(),
            None,
        )
    })
}
```

## Audit Summary

### Production Code Analysis (as of refactoring)

| File | Unwrap Calls | Pattern | Status |
|------|-------------|---------|--------|
| `server.rs` | 0 raw unwraps | Safe defaults | OK |
| `mcp/tools/*.rs` | 0 raw unwraps | Safe defaults | OK |
| `bridge/http.rs` | 0 production unwraps | Result propagation | OK |
| `cloud/client.rs` | 0 production unwraps | Result propagation | OK |
| `ai/knowledge_graph.rs` | `unwrap_or_default()` | Safe defaults | OK |
| `ai/mock.rs` | `unwrap()` in test helpers | Test context | OK |

### Test Code (Acceptable)

Test modules use `.unwrap()` appropriately for assertions. This is correct - failed unwraps in tests cause test failures, which is the desired behavior.

## Guidelines for Contributors

1. **Never use `.unwrap()` in production code** for operations that can fail
2. **Always use `.map_err()?`** to propagate errors with context
3. **Use `.unwrap_or(default)`** for optional parameters with documented defaults
4. **Use `.ok_or_else()?`** for converting Option to Result with clear error messages
5. **Test code may use `.unwrap()`** - failed tests should panic
6. **Add context to errors** - users should understand what went wrong

## Related Documentation

- `docs/TESTING_PATTERNS.md` - Mock infrastructure and testing patterns
- `docs/API_REFERENCE.md` - Public traits and error types
