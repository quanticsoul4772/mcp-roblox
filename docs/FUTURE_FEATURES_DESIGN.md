# Future Features Design: Utilizing Reserved Public APIs

## Executive Summary

This document specifies features that will utilize the currently unused public APIs:
- `StudioBridge::is_connected()` - Connection health checking
- `HttpClient::post_json()` - JSON POST requests for additional Open Cloud APIs

These APIs were intentionally included in the trait definitions for future extensibility.

---

## 1. Connection Health Features (`is_connected`)

### 1.1 MCP Health Check Tool

**Purpose**: Allow AI agents to check Studio connection status before attempting operations.

```rust
// src/mcp/server.rs - New tool

#[tool(description = "Check if Roblox Studio plugin is connected and responsive")]
async fn studio_health_check(&self) -> Result<CallToolResult, ErrorData> {
    let call = self.start_instrumentation("studio_health_check");

    let connected = self.bridge.is_connected().await;
    let result = json!({
        "connected": connected,
        "message": if connected {
            "Studio plugin is connected and responsive"
        } else {
            "Studio plugin is not connected or heartbeat timed out"
        }
    });

    call.finish_with(Ok(CallToolResult {
        content: vec![Content::text(serde_json::to_string_pretty(&result).unwrap())],
        is_error: Some(!connected),
    })).await
}
```

**Use Cases**:
- AI agents can check connectivity before batch operations
- Prevents wasted API calls when Studio is disconnected
- Enables graceful degradation in automation workflows

### 1.2 Connection-Aware Tool Execution

**Purpose**: Add optional pre-flight connection check to Studio tools.

```rust
// src/mcp/server.rs - Helper method

impl<B: StudioBridge + Clone, L: Linter + Clone> RobloxMcpServer<B, L> {
    /// Execute a Studio command with optional pre-flight connection check
    async fn execute_with_health_check(
        &self,
        action: &str,
        params: serde_json::Value,
        require_connection: bool,
    ) -> Result<serde_json::Value, RobloxMcpError> {
        if require_connection && !self.bridge.is_connected().await {
            return Err(RobloxMcpError::PluginTimeout(Duration::from_secs(0)));
        }
        self.bridge.execute_command(action, params).await
    }
}
```

### 1.3 Server Metrics Enhancement

**Purpose**: Expose connection status in server metrics.

```rust
// src/metrics/mod.rs - Addition

impl ServerMetrics {
    /// Record Studio connection status change
    pub fn record_connection_status(&self, connected: bool) {
        // Track connection uptime, disconnection events, etc.
    }
}

// src/mcp/server.rs - Background task

async fn connection_monitor(bridge: Arc<impl StudioBridge>, metrics: Arc<ServerMetrics>) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let connected = bridge.is_connected().await;
        metrics.record_connection_status(connected);
    }
}
```

---

## 2. JSON POST Features (`post_json`)

### 2.1 DataStore Write Operations

**Purpose**: Implement DataStore set/delete operations via Open Cloud API.

```rust
// src/cloud/datastores.rs - New methods

impl<H: HttpClient> OpenCloudClient<H> {
    /// Set a value in a DataStore
    pub async fn datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: serde_json::Value,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        let scope = scope.unwrap_or("global");
        let encoded_key = urlencoding::encode(key);
        let encoded_datastore = urlencoding::encode(datastore_name);

        let url = format!(
            "{}/cloud/v2/universes/{}/data-stores/{}/scopes/{}/entries/{}",
            self.base_url(),
            universe_id,
            encoded_datastore,
            scope,
            encoded_key
        );

        let response = self
            .http()
            .post_json(
                &url,
                &[
                    ("x-api-key", self.api_key()),
                    ("Content-Type", "application/json"),
                ],
                value.clone(),
            )
            .await?;

        if !response.is_success() {
            let body = response.text().unwrap_or_else(|_| "[failed to read body]".into());
            return Err(RobloxMcpError::OpenCloudError {
                status: response.status,
                message: body,
            });
        }

        Ok(DataStoreEntry {
            value,
            version: response.headers.get("roblox-entry-version").cloned().unwrap_or_default(),
            created_time: response.headers.get("roblox-entry-created-time").cloned().unwrap_or_default(),
            updated_time: response.headers.get("roblox-entry-version-created-time").cloned().unwrap_or_default(),
        })
    }

    /// Delete a DataStore entry
    pub async fn datastore_delete(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<(), RobloxMcpError> {
        // Uses DELETE HTTP method (would need trait extension)
        // Alternative: use post_json with special endpoint
        todo!("Implement when DELETE method added to HttpClient trait")
    }
}
```

**MCP Tool**:
```rust
#[tool(description = "Write a value to a Roblox DataStore via Open Cloud API")]
async fn cloud_datastore_set(
    &self,
    Parameters(params): Parameters<CloudDatastoreSetParams>,
) -> Result<CallToolResult, ErrorData> {
    // Implementation
}
```

### 2.2 Messaging Service Integration

**Purpose**: Send messages to MessagingService topics via Open Cloud.

```rust
// src/cloud/messaging.rs - New module

use crate::error::RobloxMcpError;
use crate::http::HttpClient;

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// Publish a message to a MessagingService topic
    pub async fn messaging_publish(
        &self,
        universe_id: u64,
        topic: &str,
        message: serde_json::Value,
    ) -> Result<(), RobloxMcpError> {
        let url = format!(
            "{}/cloud/v2/universes/{}/topics/{}/messages",
            self.base_url(),
            universe_id,
            urlencoding::encode(topic)
        );

        let body = serde_json::json!({
            "message": message.to_string()
        });

        let response = self
            .http()
            .post_json(
                &url,
                &[("x-api-key", self.api_key())],
                body,
            )
            .await?;

        if !response.is_success() {
            let body = response.text().unwrap_or_else(|_| "[failed to read body]".into());
            return Err(RobloxMcpError::OpenCloudError {
                status: response.status,
                message: body,
            });
        }

        Ok(())
    }
}
```

**MCP Tool**:
```rust
#[tool(description = "Publish a message to a Roblox MessagingService topic")]
async fn cloud_messaging_publish(
    &self,
    Parameters(params): Parameters<CloudMessagingPublishParams>,
) -> Result<CallToolResult, ErrorData> {
    // Implementation
}
```

### 2.3 Ordered DataStore Operations

**Purpose**: Support OrderedDataStore increment/update operations.

```rust
// src/cloud/ordered_datastores.rs - New module

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// Increment an OrderedDataStore entry
    pub async fn ordered_datastore_increment(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        increment_by: i64,
        scope: Option<&str>,
    ) -> Result<i64, RobloxMcpError> {
        let scope = scope.unwrap_or("global");

        let url = format!(
            "{}/cloud/v2/universes/{}/ordered-data-stores/{}/scopes/{}/entries/{}:increment",
            self.base_url(),
            universe_id,
            urlencoding::encode(datastore_name),
            scope,
            urlencoding::encode(key)
        );

        let body = serde_json::json!({
            "amount": increment_by
        });

        let response = self
            .http()
            .post_json(
                &url,
                &[("x-api-key", self.api_key())],
                body,
            )
            .await?;

        if !response.is_success() {
            let body = response.text().unwrap_or_else(|_| "[failed to read body]".into());
            return Err(RobloxMcpError::OpenCloudError {
                status: response.status,
                message: body,
            });
        }

        let result: serde_json::Value = response.json()?;
        result["value"].as_i64().ok_or_else(|| {
            RobloxMcpError::InvalidStudioData("Missing value in response".into())
        })
    }
}
```

---

## 3. Implementation Plan

### Phase 1: Connection Health (Priority: High)

| Task | Effort | Dependencies |
|------|--------|--------------|
| Add `studio_health_check` MCP tool | 1 hour | None |
| Add `execute_with_health_check` helper | 30 min | None |
| Add connection metrics tracking | 1 hour | None |
| Add tests for health check | 1 hour | MockBridge |

**Files Modified**:
- `src/mcp/server.rs` - Add tool + helper
- `src/mcp/params.rs` - No params needed
- `src/metrics/mod.rs` - Add connection tracking

### Phase 2: DataStore Write (Priority: High)

| Task | Effort | Dependencies |
|------|--------|--------------|
| Implement `datastore_set` | 1 hour | `post_json` |
| Add `cloud_datastore_set` MCP tool | 1 hour | datastore_set |
| Add params struct | 30 min | None |
| Add tests with MockHttpClient | 1 hour | MockHttpClient |

**Files Modified**:
- `src/cloud/datastores.rs` - Add set method
- `src/mcp/server.rs` - Add tool
- `src/mcp/params.rs` - Add params

### Phase 3: Messaging Service (Priority: Medium)

| Task | Effort | Dependencies |
|------|--------|--------------|
| Create `src/cloud/messaging.rs` | 1 hour | `post_json` |
| Add `cloud_messaging_publish` tool | 1 hour | messaging module |
| Add params struct | 30 min | None |
| Add tests | 1 hour | MockHttpClient |

**Files Created**:
- `src/cloud/messaging.rs`

**Files Modified**:
- `src/cloud/mod.rs` - Add module
- `src/mcp/server.rs` - Add tool
- `src/mcp/params.rs` - Add params

### Phase 4: Ordered DataStores (Priority: Low)

| Task | Effort | Dependencies |
|------|--------|--------------|
| Create `src/cloud/ordered_datastores.rs` | 2 hours | `post_json` |
| Add increment/update tools | 2 hours | ordered_datastores |
| Add tests | 1 hour | MockHttpClient |

---

## 4. API Extensions Required

### 4.1 HttpClient Trait Extensions

```rust
// src/http/mod.rs - Future additions

#[async_trait]
pub trait HttpClient: Send + Sync + 'static {
    // Existing methods...

    /// Perform a DELETE request (needed for datastore_delete)
    async fn delete(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, RobloxMcpError>;

    /// Perform a PATCH request (needed for partial updates)
    async fn patch_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<HttpResponse, RobloxMcpError>;
}
```

### 4.2 MockHttpClient Extensions

```rust
// src/http/mock.rs - Additions

impl MockHttpClient {
    /// Queue a response for DELETE requests
    pub fn queue_delete_response(&self, response: MockResponse) {
        // Implementation
    }

    /// Queue a response for PATCH requests
    pub fn queue_patch_response(&self, response: MockResponse) {
        // Implementation
    }
}
```

---

## 5. New MCP Tools Summary

| Tool Name | Category | Uses API | Priority |
|-----------|----------|----------|----------|
| `studio_health_check` | Studio | `is_connected` | High |
| `cloud_datastore_set` | Cloud | `post_json` | High |
| `cloud_datastore_delete` | Cloud | `delete` (new) | Medium |
| `cloud_messaging_publish` | Cloud | `post_json` | Medium |
| `cloud_ordered_datastore_increment` | Cloud | `post_json` | Low |
| `cloud_ordered_datastore_get_sorted` | Cloud | `get` | Low |

---

## 6. Test Coverage Expectations

| Feature | Unit Tests | Integration Tests | Mock Coverage |
|---------|------------|-------------------|---------------|
| Health Check | 5 | 1 | MockBridge |
| DataStore Set | 8 | 1 | MockHttpClient |
| Messaging | 5 | 1 | MockHttpClient |
| Ordered DS | 8 | 1 | MockHttpClient |

**Estimated Coverage Impact**: +3-5% (from 87.54% to ~91-92%)

---

## 7. Breaking Changes

None. All additions are:
- New methods on existing traits
- New MCP tools
- New modules

Existing functionality remains unchanged.

---

## 8. Dependencies

No new crate dependencies required. All features use existing:
- `async-trait` for trait definitions
- `serde_json` for JSON handling
- `urlencoding` for URL encoding
- `tokio` for async runtime

---

## 9. Suppressing Current Warnings

Until features are implemented, add `#[allow(dead_code)]` annotations:

```rust
// src/bridge/mod.rs
#[allow(dead_code)]  // Reserved for future health check features
async fn is_connected(&self) -> bool;

// src/http/mod.rs
#[allow(dead_code)]  // Reserved for future DataStore/Messaging features
async fn post_json(...) -> Result<HttpResponse, RobloxMcpError>;
```

---

## 10. Recommended Implementation Order

1. **Immediate** (removes warnings, adds value):
   - `studio_health_check` tool using `is_connected`

2. **Short-term** (high value for automation):
   - `cloud_datastore_set` using `post_json`

3. **Medium-term** (complete DataStore support):
   - `cloud_datastore_delete` (requires HttpClient extension)
   - `cloud_messaging_publish`

4. **Long-term** (advanced features):
   - Ordered DataStore operations
   - Connection monitoring background task
