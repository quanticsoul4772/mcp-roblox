# Phase 4 Design Specification: Production Hardening & Extended Features

## Current State Summary (Post-Phase 3)

### Completed Components

| Component | Status | Tools | Coverage |
|-----------|--------|-------|----------|
| Filesystem Module | ✅ Complete | 6 tools | 88 tests |
| Studio Bridge | ✅ Complete | 8 tools | Tested |
| Open Cloud | ✅ Complete | 1 tool | Tested |
| File Watcher | ✅ Complete | 1 tool | Tested |
| Metrics | ✅ Complete | 1 tool | Tested |
| Plugin (Luau) | ✅ Complete | Reconnection | 280+ lines |

### Current Tool Count: 17 MCP Tools

```
Filesystem (6):  fs_get_tree, fs_read_script, fs_write_script,
                 fs_delete_script, fs_search_content, fs_get_changes

Studio (8):      studio_get_selection, studio_get_script_source,
                 studio_modify_script, studio_get_datamodel,
                 studio_create_instance, studio_set_property,
                 studio_delete_instance, studio_find_instances

Cloud (1):       cloud_publish_place

Watcher (1):     fs_watch_changes

Metrics (1):     server_get_metrics
```

---

## Phase 4 Scope

```
┌─────────────────────────────────────────────────────────────────┐
│                    PHASE 4: PRODUCTION HARDENING                 │
├─────────────────────────────────────────────────────────────────┤
│  4.1 Extended Open Cloud Tools (Assets, DataStores)             │
│  4.2 Luau Linting Integration (Selene)                          │
│  4.3 Tool Instrumentation (Metrics Recording)                   │
│  4.4 Response Pagination for Large DataModels                   │
│  4.5 Comprehensive Test Suite Expansion                         │
│  4.6 Documentation & Examples                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4.1 Extended Open Cloud Tools

### Purpose
Add remaining Open Cloud capabilities for complete CI/CD automation: asset uploading and DataStore management.

### New Tools (2)

| Tool | Purpose | Parameters |
|------|---------|------------|
| `cloud_upload_asset` | Upload images/models/audio to Roblox | asset_type, file_path, name, description |
| `cloud_datastore_get` | Read from DataStore | universe_id, datastore_name, key, scope? |

### Implementation: `src/cloud/assets.rs`

```rust
use crate::error::RobloxMcpError;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUploadResult {
    pub asset_id: u64,
    pub operation_id: String,
}

#[derive(Debug, Clone, Copy)]
pub enum AssetType {
    Image,
    Model,
    Audio,
}

impl AssetType {
    pub fn content_type(&self) -> &'static str {
        match self {
            AssetType::Image => "image/png",
            AssetType::Model => "application/octet-stream",
            AssetType::Audio => "audio/ogg",
        }
    }

    pub fn api_type(&self) -> &'static str {
        match self {
            AssetType::Image => "Decal",
            AssetType::Model => "Model",
            AssetType::Audio => "Audio",
        }
    }
}

impl super::OpenCloudClient {
    /// Upload an asset to Roblox
    pub async fn upload_asset(
        &self,
        asset_type: AssetType,
        file_path: &Path,
        name: &str,
        description: &str,
        creator_id: u64,
    ) -> Result<AssetUploadResult, RobloxMcpError> {
        let content = tokio::fs::read(file_path).await
            .map_err(|e| RobloxMcpError::FileSystemError {
                path: file_path.display().to_string(),
                source: e,
            })?;

        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("asset");

        let form = Form::new()
            .text("request", serde_json::json!({
                "assetType": asset_type.api_type(),
                "displayName": name,
                "description": description,
                "creationContext": {
                    "creator": {
                        "userId": creator_id
                    }
                }
            }).to_string())
            .part("fileContent", Part::bytes(content)
                .file_name(file_name.to_string())
                .mime_str(asset_type.content_type())
                .map_err(|e| RobloxMcpError::ConfigError(e.to_string()))?);

        let url = format!("{}/assets/v1/assets", self.base_url);

        let response = self.client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RobloxMcpError::OpenCloudError {
                status: status.as_u16(),
                message: body,
            });
        }

        response.json().await
            .map_err(RobloxMcpError::from_reqwest)
    }
}
```

### Implementation: `src/cloud/datastores.rs`

```rust
use crate::error::RobloxMcpError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStoreEntry {
    pub value: serde_json::Value,
    pub version: String,
    pub created_time: String,
    pub updated_time: String,
}

impl super::OpenCloudClient {
    /// Get a value from DataStore
    pub async fn datastore_get(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        let scope = scope.unwrap_or("global");

        let url = format!(
            "{}/cloud/v2/universes/{}/data-stores/{}/scopes/{}/entries/{}",
            self.base_url, universe_id, datastore_name, scope, key
        );

        let response = self.client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RobloxMcpError::OpenCloudError {
                status: status.as_u16(),
                message: body,
            });
        }

        response.json().await
            .map_err(RobloxMcpError::from_reqwest)
    }
}
```

### New MCP Tool Params

```rust
// src/mcp/params.rs additions

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudUploadAssetParams {
    #[schemars(description = "Asset type: 'image', 'model', or 'audio'")]
    pub asset_type: String,
    #[schemars(description = "Path to asset file")]
    pub file_path: String,
    #[schemars(description = "Display name for the asset")]
    pub name: String,
    #[schemars(description = "Asset description")]
    pub description: String,
    #[schemars(description = "Creator user ID")]
    pub creator_id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloudDatastoreGetParams {
    #[schemars(description = "Universe ID")]
    pub universe_id: u64,
    #[schemars(description = "DataStore name")]
    pub datastore_name: String,
    #[schemars(description = "Entry key")]
    pub key: String,
    #[schemars(description = "Scope (default: 'global')")]
    pub scope: Option<String>,
}
```

---

## 4.2 Luau Linting Integration (Selene)

### Purpose
Integrate Selene linter to provide code quality feedback before scripts are synced to Studio.

### New Tool (1)

| Tool | Purpose | Parameters |
|------|---------|------------|
| `fs_lint_script` | Run Selene on a Luau file | file_path, config_path? |

### Implementation: `src/tools/linting.rs`

```rust
use crate::error::RobloxMcpError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResult {
    pub file_path: String,
    pub diagnostics: Vec<LintDiagnostic>,
    pub error_count: usize,
    pub warning_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintDiagnostic {
    pub severity: String,  // "error" | "warning"
    pub code: String,      // e.g., "unused_variable"
    pub message: String,
    pub line: u32,
    pub column: u32,
}

/// Run Selene linter on a Luau file
pub async fn lint_script(
    file_path: &Path,
    config_path: Option<&Path>,
) -> Result<LintResult, RobloxMcpError> {
    let mut cmd = Command::new("selene");

    cmd.arg("--display-style=json2");

    if let Some(config) = config_path {
        cmd.arg("--config").arg(config);
    }

    cmd.arg(file_path);

    cmd.stdout(Stdio::piped())
       .stderr(Stdio::piped());

    let output = cmd.output().await
        .map_err(|e| RobloxMcpError::ConfigError(
            format!("Failed to run selene: {}. Is selene installed?", e)
        ))?;

    // Selene returns non-zero on lint errors, but we still want the output
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse JSON output
    let diagnostics: Vec<LintDiagnostic> = if stdout.is_empty() {
        vec![]
    } else {
        serde_json::from_str(&stdout)
            .map_err(|e| RobloxMcpError::SerializationError(e))?
    };

    let error_count = diagnostics.iter()
        .filter(|d| d.severity == "error")
        .count();
    let warning_count = diagnostics.iter()
        .filter(|d| d.severity == "warning")
        .count();

    Ok(LintResult {
        file_path: file_path.display().to_string(),
        diagnostics,
        error_count,
        warning_count,
    })
}
```

### MCP Tool Implementation

```rust
#[tool(description = "Run Selene linter on a Luau script file")]
async fn fs_lint_script(
    &self,
    Parameters(params): Parameters<FsLintScriptParams>,
) -> Result<CallToolResult, ErrorData> {
    let path = PathBuf::from(&params.file_path);

    // Validate .luau extension
    if path.extension() != Some(std::ffi::OsStr::new("luau")) {
        return Err(ErrorData::invalid_params(
            "Only .luau files can be linted".to_string(),
            None,
        ));
    }

    let config_path = params.config_path.map(PathBuf::from);

    let result = lint_script(&path, config_path.as_deref()).await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    )]))
}
```

---

## 4.3 Tool Instrumentation (Metrics Recording)

### Purpose
The metrics module exists but tools don't record their execution. Add instrumentation to track tool performance.

### Implementation Strategy

Create a wrapper that instruments tool calls:

```rust
// src/mcp/instrumentation.rs

use std::time::Instant;
use crate::metrics::ServerMetrics;

pub struct InstrumentedCall<'a> {
    metrics: &'a ServerMetrics,
    tool_name: String,
    start: Instant,
}

impl<'a> InstrumentedCall<'a> {
    pub fn start(metrics: &'a ServerMetrics, tool_name: impl Into<String>) -> Self {
        Self {
            metrics,
            tool_name: tool_name.into(),
            start: Instant::now(),
        }
    }

    pub async fn finish(self, success: bool) {
        let duration = self.start.elapsed();
        let tool_metrics = self.metrics.get_tool(&self.tool_name).await;

        if success {
            tool_metrics.record_call(duration).await;
        } else {
            tool_metrics.record_call(duration).await;
            tool_metrics.record_error();
        }
    }
}
```

### Example Instrumented Tool

```rust
#[tool(description = "Read a Luau script file")]
async fn fs_read_script(
    &self,
    Parameters(params): Parameters<FsReadScriptParams>,
) -> Result<CallToolResult, ErrorData> {
    let call = InstrumentedCall::start(&self.metrics, "fs_read_script");

    let result = self.fs_read_script_inner(params).await;

    call.finish(result.is_ok()).await;

    result
}

async fn fs_read_script_inner(
    &self,
    params: FsReadScriptParams,
) -> Result<CallToolResult, ErrorData> {
    // ... actual implementation ...
}
```

---

## 4.4 Response Pagination for Large DataModels

### Purpose
Large DataModel queries can exceed context windows. Add pagination support.

### New Tool Variant

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StudioGetDataModelPaginatedParams {
    #[schemars(description = "Maximum depth to traverse (default: 3)")]
    pub max_depth: Option<usize>,
    #[schemars(description = "Starting path for pagination (e.g., 'game.Workspace')")]
    pub start_path: Option<String>,
    #[schemars(description = "Maximum instances to return (default: 500, max: 1000)")]
    pub limit: Option<usize>,
    #[schemars(description = "Cursor from previous response for continuation")]
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedDataModelResult {
    pub instances: Vec<InstanceInfo>,
    pub count: usize,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
```

### Implementation Strategy

```rust
#[tool(description = "Get DataModel with pagination to avoid context overflow")]
async fn studio_get_datamodel_paginated(
    &self,
    Parameters(params): Parameters<StudioGetDataModelPaginatedParams>,
) -> Result<CallToolResult, ErrorData> {
    let max_depth = params.max_depth.unwrap_or(3);
    let limit = params.limit.unwrap_or(500).min(1000);
    let start_path = params.start_path.unwrap_or_else(|| "game".to_string());

    // Send pagination parameters to plugin
    let response = self.bridge.execute_command(
        "getDataModelPaginated",
        json!({
            "startPath": start_path,
            "maxDepth": max_depth,
            "limit": limit,
            "cursor": params.cursor,
        }),
    ).await.map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&response)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    )]))
}
```

---

## 4.5 Comprehensive Test Suite Expansion

### Current Test Status

- 88 unit tests passing
- Integration tests require compiled binary
- Coverage: ~63% (dropped after Phase 3 additions)

### Test Expansion Goals

| Module | Current | Target | Focus Areas |
|--------|---------|--------|-------------|
| cloud/ | 2 tests | 15 tests | API mocking, error paths |
| watcher/ | 6 tests | 15 tests | Event simulation, edge cases |
| metrics/ | 6 tests | 12 tests | Boundary conditions |
| tools/ | 20+ tests | 30 tests | Linting, pagination |

### New Test Categories

#### 1. Open Cloud API Mocking

```rust
#[cfg(test)]
mod cloud_tests {
    use super::*;
    use mockito::{mock, server_url};

    #[tokio::test]
    async fn test_publish_place_success() {
        let _m = mock("POST", "/universes/v1/123/places/456/versions")
            .with_status(200)
            .with_body(r#"{"versionNumber": 42}"#)
            .create();

        // ... test implementation
    }

    #[tokio::test]
    async fn test_publish_place_rate_limited() {
        let _m = mock("POST", "/universes/v1/123/places/456/versions")
            .with_status(429)
            .with_body(r#"{"error": "Rate limited"}"#)
            .create();

        // ... verify proper error handling
    }

    #[tokio::test]
    async fn test_upload_asset_invalid_type() {
        // ... test error handling for invalid asset types
    }
}
```

#### 2. File Watcher Edge Cases

```rust
#[tokio::test]
async fn test_watcher_rapid_changes() {
    // Simulate rapid file modifications
    // Verify queue doesn't overflow
}

#[tokio::test]
async fn test_watcher_deleted_directory() {
    // Verify proper handling when watched directory is deleted
}

#[tokio::test]
async fn test_watcher_permission_denied() {
    // Verify error events are queued when permissions change
}
```

#### 3. Metrics Boundary Tests

```rust
#[tokio::test]
async fn test_metrics_concurrent_access() {
    // Multiple tools recording simultaneously
}

#[tokio::test]
async fn test_metrics_percentile_edge_cases() {
    // Empty samples, single sample, exactly at capacity
}
```

---

## 4.6 Documentation & Examples

### Documentation Deliverables

| Document | Purpose |
|----------|---------|
| `README.md` | Project overview, quick start |
| `TOOLS.md` | Complete tool reference with examples |
| `SETUP.md` | Detailed installation and configuration |
| `PLUGIN.md` | Studio plugin installation guide |
| `EXAMPLES.md` | Common workflow examples |

### Example Workflows

#### 1. Publish After Lint Check

```
User: "Lint all scripts in src/ and publish if clean"

Claude Code:
1. fs_get_tree(src/, max_depth=10)
2. For each .luau file:
   - fs_lint_script(file_path)
3. If all clean:
   - cloud_publish_place(universe_id, place_id, build/game.rbxl)
```

#### 2. Monitor and React to Changes

```
User: "Watch for changes and report modified scripts"

Claude Code:
1. fs_watch_changes(limit=50)
2. For each change:
   - fs_read_script(change.path)
   - Analyze content
   - Report findings
```

#### 3. DataStore Backup

```
User: "Backup player data from DataStore"

Claude Code:
1. cloud_datastore_get(universe_id, "PlayerData", "player_123")
2. fs_write_script("backups/player_123.json", data)
```

---

## Implementation Priority

| Task | Priority | Effort | Dependencies |
|------|----------|--------|--------------|
| Tool Instrumentation | HIGH | 1 day | None |
| Extended Open Cloud (assets) | HIGH | 2 days | None |
| Extended Open Cloud (datastores) | HIGH | 1 day | None |
| Luau Linting (Selene) | MEDIUM | 2 days | selene binary |
| Response Pagination | MEDIUM | 2 days | Plugin update |
| Test Suite Expansion | MEDIUM | 3 days | mockito |
| Documentation | LOW | 2 days | All features |

### Recommended Order

```
Week 1:
├── Day 1: Tool Instrumentation (connect existing metrics)
├── Day 2-3: Extended Open Cloud (assets + datastores)
├── Day 4-5: Luau Linting Integration

Week 2:
├── Day 1-2: Response Pagination (server + plugin)
├── Day 3-4: Test Suite Expansion
└── Day 5: Documentation
```

---

## File Structure After Phase 4

```
src/
├── bridge/
│   ├── mod.rs
│   └── http.rs
├── cloud/
│   ├── mod.rs
│   ├── client.rs
│   ├── assets.rs          # NEW
│   └── datastores.rs      # NEW
├── error.rs
├── main.rs
├── mcp/
│   ├── mod.rs
│   ├── params.rs          # + new params
│   ├── server.rs          # + new tools
│   └── instrumentation.rs # NEW
├── metrics/
│   └── mod.rs
├── tools/
│   ├── mod.rs
│   ├── filesystem.rs
│   └── linting.rs         # NEW
└── watcher/
    └── mod.rs

plugin/
└── init.lua               # + pagination support

docs/                       # NEW
├── README.md
├── TOOLS.md
├── SETUP.md
├── PLUGIN.md
└── EXAMPLES.md
```

---

## Success Criteria

- [ ] `cloud_upload_asset` successfully uploads a test image
- [ ] `cloud_datastore_get` retrieves test data from DataStore
- [ ] `fs_lint_script` returns valid Selene diagnostics
- [ ] All tools record metrics via instrumentation
- [ ] `studio_get_datamodel_paginated` handles large hierarchies
- [ ] Test coverage reaches 80%+
- [ ] All documentation complete and reviewed

---

## Security Considerations

| Risk | Mitigation |
|------|------------|
| Asset upload abuse | Rate limiting, file size validation |
| DataStore data exposure | Scope validation, read-only by default |
| Selene command injection | Validate file paths, no shell interpolation |
| Pagination cursor tampering | Server-side cursor validation |

---

## Estimated Timeline

**Total: 2 weeks**

- Week 1: Core features (instrumentation, Open Cloud, linting)
- Week 2: Polish (pagination, tests, documentation)

## New Tool Count After Phase 4: 21 MCP Tools

```
Filesystem (7):  +fs_lint_script
Cloud (3):       +cloud_upload_asset, +cloud_datastore_get
Studio (9):      +studio_get_datamodel_paginated
```
