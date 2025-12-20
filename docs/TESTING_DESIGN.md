# Testing Infrastructure Design: Mocking and Dependency Injection

## Executive Summary

This document outlines a plan to achieve 80%+ test coverage by introducing trait-based dependency injection and mocking. The current architecture has hard dependencies on:

1. **HTTP Client** (`reqwest::Client`) - For Open Cloud API calls
2. **Plugin Bridge** (`PluginBridge`) - For Roblox Studio communication
3. **External Process** (`selene`) - For Luau linting
4. **File Watcher** (`notify`) - For filesystem monitoring

## Current Architecture Analysis

### Dependencies That Block Testing

```
RobloxMcpServer
├── OpenCloudClient (cloud_client: Option<Arc<OpenCloudClient>>)
│   └── reqwest::Client (internal, makes HTTP calls)
│       ├── publish_place() -> HTTP POST to Roblox API
│       ├── upload_asset() -> HTTP POST multipart
│       └── datastore_get() -> HTTP GET
├── PluginBridge (bridge: Arc<PluginBridge>)
│   └── reqwest::Client (internal, HTTP to Studio plugin)
│       └── execute_command() -> HTTP POST to localhost:8080
├── FileWatcher (file_watcher: Option<Arc<FileWatcher>>)
│   └── notify::RecommendedWatcher
└── lint_script() calls external `selene` process
```

### Coverage Blockers by Module

| Module | Current Coverage | Blocker | Lines Affected |
|--------|-----------------|---------|----------------|
| `main.rs` | 0% | Entry point | 40 lines |
| `cloud/client.rs` | 15% | HTTP calls | 20 lines |
| `cloud/datastores.rs` | 0% | HTTP calls | 31 lines |
| `cloud/assets.rs` | 33% | HTTP calls | 37 lines |
| `mcp/server.rs` | 54% | Bridge calls | ~200 lines |
| `tools/linting.rs` | 0% | External process | 39 lines |
| `bridge/http.rs` | 67% | HTTP client | 18 lines |

## Design Options

### Option A: Trait-Based Injection with mockall

**Pros:**
- Compile-time type safety
- Zero runtime overhead in production
- Excellent IDE support
- Well-established pattern in Rust

**Cons:**
- Requires code restructuring
- Additional trait definitions
- Slightly more complex setup

### Option B: HTTP Mocking with wiremock

**Pros:**
- Tests against actual HTTP behavior
- No code changes to production
- Catches HTTP-level bugs

**Cons:**
- Runtime overhead
- Port management complexity
- Slower tests
- Already have `mockito` in dev-dependencies

### Option C: Hybrid Approach (Recommended)

Combine trait-based injection for internal components with `mockito` (already in Cargo.toml) for HTTP testing.

## Recommended Design: Hybrid Trait + Mockito

### Phase 1: Define Abstraction Traits

#### 1.1 HTTP Client Trait

```rust
// src/http/mod.rs (new module)

use async_trait::async_trait;
use crate::error::RobloxMcpError;

/// Abstraction over HTTP operations for testability
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str, headers: Vec<(&str, &str)>)
        -> Result<HttpResponse, RobloxMcpError>;

    async fn post_json(&self, url: &str, headers: Vec<(&str, &str)>, body: serde_json::Value)
        -> Result<HttpResponse, RobloxMcpError>;

    async fn post_binary(&self, url: &str, headers: Vec<(&str, &str)>, body: Vec<u8>)
        -> Result<HttpResponse, RobloxMcpError>;

    async fn post_multipart(&self, url: &str, headers: Vec<(&str, &str)>, form: MultipartForm)
        -> Result<HttpResponse, RobloxMcpError>;
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Vec<u8>,
}

pub struct MultipartForm {
    pub parts: Vec<FormPart>,
}

pub struct FormPart {
    pub name: String,
    pub content: FormContent,
}

pub enum FormContent {
    Text(String),
    File { filename: String, content_type: String, data: Vec<u8> },
}
```

#### 1.2 Production Implementation

```rust
// src/http/reqwest_client.rs

pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, RobloxMcpError> {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(5)
            .pool_idle_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(RobloxMcpError::from_reqwest)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn get(&self, url: &str, headers: Vec<(&str, &str)>)
        -> Result<HttpResponse, RobloxMcpError> {
        let mut req = self.client.get(url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.map_err(RobloxMcpError::from_reqwest)?;
        // ... convert to HttpResponse
    }
    // ... other methods
}
```

#### 1.3 Mock Implementation for Tests

```rust
// src/http/mock.rs (cfg(test) only)

#[cfg(test)]
pub struct MockHttpClient {
    responses: std::sync::Mutex<Vec<HttpResponse>>,
}

#[cfg(test)]
impl MockHttpClient {
    pub fn new() -> Self {
        Self { responses: std::sync::Mutex::new(vec![]) }
    }

    pub fn queue_response(&self, response: HttpResponse) {
        self.responses.lock().unwrap().push(response);
    }
}

#[cfg(test)]
#[async_trait]
impl HttpClient for MockHttpClient {
    async fn get(&self, _url: &str, _headers: Vec<(&str, &str)>)
        -> Result<HttpResponse, RobloxMcpError> {
        self.responses.lock().unwrap()
            .pop()
            .ok_or_else(|| RobloxMcpError::ConfigError("No mock response".into()))
    }
    // ... other methods return queued responses
}
```

### Phase 2: Refactor OpenCloudClient

#### 2.1 Inject HTTP Client

```rust
// src/cloud/client.rs

pub struct OpenCloudClient<H: HttpClient = ReqwestHttpClient> {
    http: H,
    api_key: String,
    base_url: String,
}

impl OpenCloudClient<ReqwestHttpClient> {
    /// Production constructor - uses real HTTP
    pub fn new() -> Result<Self, RobloxMcpError> {
        let api_key = std::env::var("ROBLOX_OPEN_CLOUD_API_KEY")
            .map_err(|_| RobloxMcpError::ConfigError("API key not set".into()))?;

        Ok(Self {
            http: ReqwestHttpClient::new()?,
            api_key,
            base_url: "https://apis.roblox.com".into(),
        })
    }
}

impl<H: HttpClient> OpenCloudClient<H> {
    /// Test constructor - accepts any HttpClient implementation
    #[cfg(test)]
    pub fn with_http(http: H, api_key: String) -> Self {
        Self {
            http,
            api_key,
            base_url: "https://apis.roblox.com".into(),
        }
    }

    pub async fn publish_place(&self, ...) -> Result<PublishResult, RobloxMcpError> {
        let content = tokio::fs::read(rbxl_path).await?;

        let response = self.http.post_binary(
            &format!("{}/universes/v1/{}/places/{}/versions",
                self.base_url, universe_id, place_id),
            vec![
                ("x-api-key", &self.api_key),
                ("Content-Type", "application/octet-stream"),
            ],
            content,
        ).await?;

        // Parse response...
    }
}
```

#### 2.2 Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttpClient;

    #[tokio::test]
    async fn test_publish_place_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(HttpResponse {
            status: 200,
            headers: Default::default(),
            body: br#"{"versionNumber": 42}"#.to_vec(),
        });

        let client = OpenCloudClient::with_http(mock, "test-key".into());

        // Create temp file
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), b"fake rbxl").unwrap();

        let result = client.publish_place(123, 456, temp.path()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().version_number, 42);
    }

    #[tokio::test]
    async fn test_publish_place_api_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(HttpResponse {
            status: 401,
            headers: Default::default(),
            body: b"Unauthorized".to_vec(),
        });

        let client = OpenCloudClient::with_http(mock, "bad-key".into());
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), b"fake").unwrap();

        let result = client.publish_place(123, 456, temp.path()).await;

        assert!(matches!(result, Err(RobloxMcpError::OpenCloudError { status: 401, .. })));
    }
}
```

### Phase 3: Refactor PluginBridge

#### 3.1 Define Bridge Trait

```rust
// src/bridge/mod.rs

#[async_trait]
pub trait StudioBridge: Send + Sync {
    async fn execute_command(&self, action: &str, params: serde_json::Value)
        -> Result<serde_json::Value, RobloxMcpError>;

    fn is_connected(&self) -> bool;
}
```

#### 3.2 Implement for PluginBridge

```rust
// src/bridge/http.rs

#[async_trait]
impl StudioBridge for PluginBridge {
    async fn execute_command(&self, action: &str, params: serde_json::Value)
        -> Result<serde_json::Value, RobloxMcpError> {
        // Existing implementation
    }

    fn is_connected(&self) -> bool {
        // Existing heartbeat check
    }
}
```

#### 3.3 Mock Bridge for Tests

```rust
// src/bridge/mock.rs

#[cfg(test)]
pub struct MockBridge {
    responses: std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>,
    connected: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MockBridge {
    pub fn new() -> Self {
        Self {
            responses: Default::default(),
            connected: std::sync::atomic::AtomicBool::new(true),
        }
    }

    pub fn set_response(&self, action: &str, response: serde_json::Value) {
        self.responses.lock().unwrap().insert(action.to_string(), response);
    }

    pub fn set_disconnected(&self) {
        self.connected.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
#[async_trait]
impl StudioBridge for MockBridge {
    async fn execute_command(&self, action: &str, _params: serde_json::Value)
        -> Result<serde_json::Value, RobloxMcpError> {
        if !self.connected.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(RobloxMcpError::PluginTimeout(Duration::from_secs(10)));
        }

        self.responses.lock().unwrap()
            .get(action)
            .cloned()
            .ok_or_else(|| RobloxMcpError::ConfigError(format!("No mock for {}", action)))
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }
}
```

### Phase 4: Refactor RobloxMcpServer

#### 4.1 Generic Server with Trait Bounds

```rust
// src/mcp/server.rs

pub struct RobloxMcpServer<B: StudioBridge = PluginBridge, C: HttpClient = ReqwestHttpClient> {
    tool_router: ToolRouter<Self>,
    bridge: Arc<B>,
    project_root: PathBuf,
    cloud_client: Option<Arc<OpenCloudClient<C>>>,
    file_watcher: Option<Arc<FileWatcher>>,
    metrics: Arc<ServerMetrics>,
}

impl RobloxMcpServer<PluginBridge, ReqwestHttpClient> {
    /// Production constructor
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        // Existing implementation
    }
}

impl<B: StudioBridge, C: HttpClient> RobloxMcpServer<B, C> {
    /// Test constructor
    #[cfg(test)]
    pub fn with_mocks(
        bridge: Arc<B>,
        project_root: PathBuf,
        cloud_client: Option<Arc<OpenCloudClient<C>>>,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client,
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
        }
    }
}
```

### Phase 5: Linting Abstraction

#### 5.1 Linter Trait

```rust
// src/tools/linting.rs

#[async_trait]
pub trait Linter: Send + Sync {
    async fn lint(&self, file_path: &Path, config: Option<&Path>)
        -> Result<LintResult, RobloxMcpError>;
}

pub struct SeleneLinter;

#[async_trait]
impl Linter for SeleneLinter {
    async fn lint(&self, file_path: &Path, config: Option<&Path>)
        -> Result<LintResult, RobloxMcpError> {
        lint_script(file_path, config).await
    }
}

#[cfg(test)]
pub struct MockLinter {
    result: std::sync::Mutex<Option<LintResult>>,
}

#[cfg(test)]
impl MockLinter {
    pub fn with_result(result: LintResult) -> Self {
        Self { result: std::sync::Mutex::new(Some(result)) }
    }
}

#[cfg(test)]
#[async_trait]
impl Linter for MockLinter {
    async fn lint(&self, file_path: &Path, _config: Option<&Path>)
        -> Result<LintResult, RobloxMcpError> {
        self.result.lock().unwrap().take()
            .ok_or_else(|| RobloxMcpError::ConfigError("No mock".into()))
    }
}
```

## Implementation Plan

### Step 1: Add Dependencies (1 hour)
```toml
# Cargo.toml
[dependencies]
async-trait = "0.1"

[dev-dependencies]
mockall = "0.13"  # Optional, for auto-mock generation
```

### Step 2: Create HTTP Abstraction (2-3 hours)
1. Create `src/http/mod.rs` with trait definition
2. Create `src/http/reqwest_client.rs` with production impl
3. Create `src/http/mock.rs` with test impl
4. Add module to `src/lib.rs`

### Step 3: Refactor Cloud Clients (3-4 hours)
1. Update `OpenCloudClient` to use generic `HttpClient`
2. Refactor `publish_place`, `upload_asset`, `datastore_get`
3. Add tests using `MockHttpClient`
4. Verify existing tests still pass

### Step 4: Refactor Plugin Bridge (2-3 hours)
1. Define `StudioBridge` trait
2. Implement for `PluginBridge`
3. Create `MockBridge`
4. Update server to use trait bound

### Step 5: Refactor Server (2-3 hours)
1. Add generic parameters to `RobloxMcpServer`
2. Create `with_mocks` constructor
3. Update tool implementations to use trait methods
4. Add comprehensive Studio tool tests

### Step 6: Linting Abstraction (1-2 hours)
1. Create `Linter` trait
2. Implement `SeleneLinter`
3. Create `MockLinter`
4. Add lint tests

### Step 7: Integration Testing (2-3 hours)
1. Use `mockito` for HTTP endpoint simulation
2. Test full request/response cycles
3. Test error scenarios

## Estimated Coverage Impact

| Module | Before | After | Lines Testable |
|--------|--------|-------|----------------|
| `cloud/client.rs` | 15% | 90%+ | All but new() |
| `cloud/datastores.rs` | 0% | 90%+ | All |
| `cloud/assets.rs` | 33% | 90%+ | All |
| `mcp/server.rs` | 54% | 85%+ | Studio tools |
| `tools/linting.rs` | 0% | 80%+ | All but spawn |
| `bridge/http.rs` | 67% | 85%+ | All handlers |
| **Total** | ~52% | **80%+** | Target achieved |

## Alternative: Using mockall for Auto-Mocking

If manual mock implementations become tedious, `mockall` can auto-generate mocks:

```rust
use mockall::automock;

#[automock]
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn get(&self, url: &str) -> Result<HttpResponse, Error>;
}

// In tests:
let mut mock = MockHttpClient::new();
mock.expect_get()
    .with(eq("https://api.roblox.com/test"))
    .returning(|_| Ok(HttpResponse { status: 200, body: vec![] }));
```

## Migration Strategy

1. **Phase 1**: Add traits alongside existing code (non-breaking)
2. **Phase 2**: Implement traits for existing types
3. **Phase 3**: Update server to use traits
4. **Phase 4**: Add comprehensive tests
5. **Phase 5**: Remove any deprecated code paths

This approach ensures zero production impact while enabling full testability.

## Files to Create/Modify

### New Files
- `src/http/mod.rs`
- `src/http/reqwest_client.rs`
- `src/http/mock.rs`
- `src/bridge/mock.rs`
- `docs/TESTING_DESIGN.md` (this file)

### Modified Files
- `Cargo.toml` (add async-trait)
- `src/lib.rs` (add http module)
- `src/cloud/client.rs` (generics)
- `src/cloud/assets.rs` (use trait)
- `src/cloud/datastores.rs` (use trait)
- `src/bridge/mod.rs` (add trait)
- `src/bridge/http.rs` (implement trait)
- `src/mcp/server.rs` (generic params)
- `src/tools/linting.rs` (add trait)
