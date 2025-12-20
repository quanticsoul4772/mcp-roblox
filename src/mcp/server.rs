use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use regex::Regex;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};

#[cfg(test)]
use rmcp::model::RawContent;
use serde_json::json;
use tokio::fs;
use walkdir::WalkDir;

use tracing::warn;

use crate::bridge::http::PluginBridge;
use crate::bridge::StudioBridge;
use crate::cloud::AssetType;
use crate::cloud::OpenCloudClient;
use crate::mcp::instrumentation::InstrumentedCall;
use crate::mcp::params::{
    // Cloud params
    CloudDatastoreGetParams, CloudDatastoreSetParams, CloudMessagingPublishParams,
    CloudPublishPlaceParams, CloudUploadAssetParams,
    // Filesystem params
    FsDeleteScriptParams, FsGetChangesParams, FsGetTreeParams, FsLintScriptParams,
    FsReadScriptParams, FsSearchContentParams, FsWatchChangesParams, FsWriteScriptParams,
    // Studio params
    StudioCreateInstanceParams, StudioDeleteInstanceParams, StudioFindInstancesParams,
    StudioGetDataModelPaginatedParams, StudioGetDataModelParams, StudioGetScriptSourceParams,
    StudioModifyScriptParams, StudioSetPropertyParams,
};
use crate::metrics::ServerMetrics;
use crate::tools::filesystem::{build_tree, read_script, validate_path, write_script};
use crate::tools::linting::{Linter, SeleneLinter};
use crate::watcher::FileWatcher;

/// Roblox MCP Server with injectable dependencies
///
/// Generic over:
/// - `B`: StudioBridge implementation (default: PluginBridge)
/// - `L`: Linter implementation (default: SeleneLinter)
///
/// This allows injecting mock implementations for testing while keeping
/// production code unchanged.
#[derive(Clone)]
pub struct RobloxMcpServer<B: StudioBridge + Clone = PluginBridge, L: Linter + Clone = SeleneLinter>
{
    tool_router: ToolRouter<Self>,
    bridge: Arc<B>,
    project_root: PathBuf,
    /// Open Cloud client for CI/CD operations (optional - only if API key configured)
    cloud_client: Option<Arc<OpenCloudClient>>,
    /// File watcher for real-time change detection (optional - may fail on some platforms)
    file_watcher: Option<Arc<FileWatcher>>,
    /// Server metrics for monitoring
    metrics: Arc<ServerMetrics>,
    /// Linter for Luau script analysis
    linter: L,
}

/// Production constructor for RobloxMcpServer with PluginBridge and SeleneLinter
impl RobloxMcpServer<PluginBridge, SeleneLinter> {
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        // Initialize cloud client with explicit logging on failure
        // Cloud tools will check availability and return clear error to users
        let cloud_client = match OpenCloudClient::new() {
            Ok(client) => Some(Arc::new(client)),
            Err(e) => {
                warn!(
                    "Open Cloud client unavailable: {}. Cloud tools (publish, assets, datastores) will be disabled.",
                    e
                );
                None
            }
        };

        // Initialize file watcher with explicit logging on failure
        // May fail on some platforms or if directory is inaccessible
        let file_watcher = match FileWatcher::new(project_root.clone()) {
            Ok(watcher) => Some(Arc::new(watcher)),
            Err(e) => {
                warn!(
                    "File watcher unavailable: {}. Real-time change detection will be disabled.",
                    e
                );
                None
            }
        };

        // Always create metrics
        let metrics = Arc::new(ServerMetrics::new());

        // Initialize Selene linter
        let linter = SeleneLinter::new();

        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client,
            file_watcher,
            metrics,
            linter,
        }
    }
}

/// Generic implementation for any StudioBridge with SeleneLinter default
impl<B: StudioBridge + Clone + 'static> RobloxMcpServer<B, SeleneLinter> {
    /// Create a test server with a mock bridge (uses SeleneLinter by default)
    ///
    /// This constructor is used for testing to inject mock bridge dependencies
    /// while using the production SeleneLinter.
    #[cfg(test)]
    pub fn with_mock_bridge(bridge: Arc<B>, project_root: PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client: None,
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
            linter: SeleneLinter::new(),
        }
    }
}

/// Generic implementation for any StudioBridge and Linter
impl<B: StudioBridge + Clone + 'static, L: Linter + Clone + 'static> RobloxMcpServer<B, L> {
    /// Create a test server with a mock bridge and custom linter
    ///
    /// This constructor is used for testing to inject both bridge and linter dependencies.
    #[cfg(test)]
    pub fn with_mock_bridge_and_linter(
        bridge: Arc<B>,
        project_root: PathBuf,
        linter: L,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client: None,
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
            linter,
        }
    }

    /// Create a test server with mock bridge, cloud client, and linter
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn with_mocks(
        bridge: Arc<B>,
        project_root: PathBuf,
        _cloud_client: Option<Arc<OpenCloudClient>>,
        linter: L,
    ) -> RobloxMcpServer<B, L> {
        // Note: We can't store generic cloud client, so for full mock testing
        // we'd need a separate test-only server struct. For now, we support
        // mock bridge without cloud client, which covers Studio tool testing.
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client: None, // TODO: would need type erasure for full generic support
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
            linter,
        }
    }

    /// Start instrumentation for a tool call
    fn start_instrumentation(&self, tool_name: &str) -> InstrumentedCall {
        InstrumentedCall::start(self.metrics.clone(), tool_name)
    }
}

#[tool_router]
impl<B: StudioBridge + Clone + 'static, L: Linter + Clone + 'static> RobloxMcpServer<B, L> {
    // === FILESYSTEM TOOLS (7) ===

    #[tool(description = "List project file structure with depth limits. Returns a tree of files and directories, plus any skipped entries.")]
    async fn fs_get_tree(
        &self,
        Parameters(params): Parameters<FsGetTreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_get_tree");

        let result = self.fs_get_tree_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_get_tree_impl(&self, params: FsGetTreeParams) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // max_depth is optional with documented default - this is acceptable
        let max_depth = params.max_depth.unwrap_or(5);

        // build_tree now returns TreeBuildResult with both tree and skipped entries
        let result = build_tree(&validated_path, 0, max_depth)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Return both tree AND skipped entries so caller knows what was excluded
        let json = serde_json::to_string_pretty(&json!({
            "tree": result.tree,
            "skipped": result.skipped,
            "skipped_count": result.skipped.len()
        }))
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Read a Luau script file. Only .luau files are supported.")]
    async fn fs_read_script(
        &self,
        Parameters(params): Parameters<FsReadScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_read_script");
        let result = self.fs_read_script_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_read_script_impl(
        &self,
        params: FsReadScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let script_content = read_script(&validated_path)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let json = serde_json::to_string_pretty(&script_content)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Write or create a Luau script file. Optionally create parent directories.")]
    async fn fs_write_script(
        &self,
        Parameters(params): Parameters<FsWriteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_write_script");
        let result = self.fs_write_script_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_write_script_impl(
        &self,
        params: FsWriteScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // For new files, validate the parent directory is within project root
        let parent = path.parent().ok_or_else(|| {
            ErrorData::internal_error("Invalid file path: no parent directory".to_string(), None)
        })?;

        // Check if parent exists or if we're allowed to create it
        let create_dirs = params.create_directories.unwrap_or(false);

        if !parent.exists() && !create_dirs {
            return Err(ErrorData::internal_error(
                format!(
                    "Parent directory does not exist: {}. Set create_directories=true to create it.",
                    parent.display()
                ),
                None,
            ));
        }

        // If parent exists, validate it's within project root
        if parent.exists() {
            validate_path(parent, &self.project_root)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        } else {
            // Validate that the target would be within project root
            // by checking if the path starts with project_root
            let abs_path = if path.is_absolute() {
                path.clone()
            } else {
                self.project_root.join(&path)
            };

            if !abs_path.starts_with(&self.project_root) {
                return Err(ErrorData::internal_error(
                    format!("Path traversal detected: {}", abs_path.display()),
                    None,
                ));
            }
        }

        // Validate .luau extension
        if path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::internal_error(
                "Only .luau files are supported".to_string(),
                None,
            ));
        }

        let write_result = write_script(&path, &params.content, create_dirs)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let json = serde_json::to_string_pretty(&write_result)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Delete a Luau script file. Only .luau files can be deleted.")]
    async fn fs_delete_script(
        &self,
        Parameters(params): Parameters<FsDeleteScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_delete_script");
        let result = self.fs_delete_script_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_delete_script_impl(
        &self,
        params: FsDeleteScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::internal_error(
                "Only .luau files can be deleted".to_string(),
                None,
            ));
        }

        // Check file exists
        if !validated_path.exists() {
            return Err(ErrorData::internal_error(
                format!("File not found: {}", validated_path.display()),
                None,
            ));
        }

        fs::remove_file(&validated_path).await.map_err(|e| {
            ErrorData::internal_error(format!("Failed to delete file: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "deleted": validated_path.display().to_string(),
                "success": true
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Search for patterns in script files using regex. Returns matching lines with context.")]
    async fn fs_search_content(
        &self,
        Parameters(params): Parameters<FsSearchContentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_search_content");
        let result = self.fs_search_content_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_search_content_impl(
        &self,
        params: FsSearchContentParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Compile regex pattern
        let regex = Regex::new(&params.pattern).map_err(|e| {
            ErrorData::internal_error(format!("Invalid regex pattern: {e}"), None)
        })?;

        // Extension is REQUIRED (enforced by schema)
        let extension = params.extension.as_str();

        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();

        // Walk directory and search files - REPORT ALL ERRORS
        for entry in WalkDir::new(&validated_path).into_iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(json!({
                        "type": "enumeration_error",
                        "path": e.path().map(|p| p.display().to_string()),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let entry_path = entry.path();

            // Skip directories
            if entry_path.is_dir() {
                continue;
            }

            // Check extension
            if entry_path.extension() != Some(std::ffi::OsStr::new(extension)) {
                continue;
            }

            // Skip hidden files - but REPORT that we're skipping
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    errors.push(json!({
                        "type": "skipped_hidden",
                        "path": entry_path.display().to_string()
                    }));
                    continue;
                }
            }

            // Read and search file - REPORT READ FAILURES
            let content = match fs::read_to_string(entry_path).await {
                Ok(c) => c,
                Err(e) => {
                    errors.push(json!({
                        "type": "read_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            for (line_num, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(json!({
                        "file": entry_path.display().to_string(),
                        "line": line_num + 1,
                        "content": line.trim()
                    }));
                }
            }
        }

        // Always include errors in response so caller knows what was skipped
        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "matches": results.len(),
                "results": results,
                "errors": errors,
                "error_count": errors.len()
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Get file modification times for change detection. Returns a map of file paths to modification timestamps.")]
    async fn fs_get_changes(
        &self,
        Parameters(params): Parameters<FsGetChangesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_get_changes");
        let result = self.fs_get_changes_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_get_changes_impl(
        &self,
        params: FsGetChangesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let mut mtimes: HashMap<String, u64> = HashMap::new();
        let mut errors: Vec<serde_json::Value> = Vec::new();

        // Walk directory and collect mtimes - REPORT ALL ERRORS
        for entry in WalkDir::new(&validated_path).into_iter() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    errors.push(json!({
                        "type": "enumeration_error",
                        "path": e.path().map(|p| p.display().to_string()),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let entry_path = entry.path();

            // Skip directories
            if entry_path.is_dir() {
                continue;
            }

            // Only track .luau files
            if entry_path.extension() != Some(std::ffi::OsStr::new("luau")) {
                continue;
            }

            // Skip hidden files - but REPORT
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    errors.push(json!({
                        "type": "skipped_hidden",
                        "path": entry_path.display().to_string()
                    }));
                    continue;
                }
            }

            // Get modification time - REPORT ALL FAILURES
            let metadata = match entry_path.metadata() {
                Ok(m) => m,
                Err(e) => {
                    errors.push(json!({
                        "type": "metadata_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let modified = match metadata.modified() {
                Ok(m) => m,
                Err(e) => {
                    errors.push(json!({
                        "type": "modified_time_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            let duration = match modified.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(json!({
                        "type": "timestamp_error",
                        "path": entry_path.display().to_string(),
                        "error": e.to_string()
                    }));
                    continue;
                }
            };

            mtimes.insert(entry_path.display().to_string(), duration.as_secs());
        }

        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "file_count": mtimes.len(),
                "files": mtimes,
                "errors": errors,
                "error_count": errors.len()
            })
            .to_string(),
        )]))
    }

    #[tool(description = "Run Selene linter on a Luau script file. Returns diagnostics with errors and warnings. Requires 'selene' to be installed (cargo install selene).")]
    async fn fs_lint_script(
        &self,
        Parameters(params): Parameters<FsLintScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_lint_script");
        let result = self.fs_lint_script_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_lint_script_impl(
        &self,
        params: FsLintScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let path = PathBuf::from(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(&path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension() != Some(std::ffi::OsStr::new("luau")) {
            return Err(ErrorData::internal_error(
                "Only .luau files can be linted".to_string(),
                None,
            ));
        }

        let config_path = params.config_path.map(PathBuf::from);

        // Use injected linter for testability
        let result = self
            .linter
            .lint(&validated_path, config_path.as_deref())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === STUDIO TOOLS (9) ===
    // These tools communicate with Roblox Studio via the HTTP plugin bridge.
    // The plugin must be connected for these tools to work.

    #[tool(description = "Check if Roblox Studio plugin is connected and responsive. Use this before batch operations to avoid timeout errors.")]
    async fn studio_health_check(&self) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_health_check");
        let result = self.studio_health_check_impl().await;
        call.finish_with(result).await
    }

    async fn studio_health_check_impl(&self) -> Result<CallToolResult, ErrorData> {
        let connected = self.bridge.is_connected().await;

        // Record connection status in metrics
        self.metrics.record_connection_status(connected);

        // Get connection metrics snapshot for detailed stats
        let connection_stats = self.metrics.connection_snapshot();

        let result = json!({
            "connected": connected,
            "message": if connected {
                "Studio plugin is connected and responsive"
            } else {
                "Studio plugin is not connected or heartbeat timed out"
            },
            "stats": connection_stats
        });

        // Use the raw CallToolResult to set is_error based on connection status
        let call_result = CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]);

        Ok(CallToolResult {
            is_error: Some(!connected),
            ..call_result
        })
    }

    #[tool(description = "Get currently selected instances in Roblox Studio. Returns array of selected instances with Name, ClassName, and Path.")]
    async fn studio_get_selection(&self) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_selection");
        let result = self.studio_get_selection_impl().await;
        call.finish_with(result).await
    }

    async fn studio_get_selection_impl(&self) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command("getSelection", json!({}))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Explore the live Studio DataModel hierarchy. Returns nested structure of instances with Name, ClassName, Path, and Children.")]
    async fn studio_get_datamodel(
        &self,
        Parameters(params): Parameters<StudioGetDataModelParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_datamodel");
        let result = self.studio_get_datamodel_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_get_datamodel_impl(
        &self,
        params: StudioGetDataModelParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "getDataModel",
                json!({ "maxDepth": params.max_depth.unwrap_or(3) }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Get DataModel with pagination to avoid context overflow for large hierarchies. Returns instances with a cursor for continuation.")]
    async fn studio_get_datamodel_paginated(
        &self,
        Parameters(params): Parameters<StudioGetDataModelPaginatedParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_datamodel_paginated");
        let result = self.studio_get_datamodel_paginated_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_get_datamodel_paginated_impl(
        &self,
        params: StudioGetDataModelPaginatedParams,
    ) -> Result<CallToolResult, ErrorData> {
        let max_depth = params.max_depth.unwrap_or(3);
        let limit = params.limit.unwrap_or(500).min(1000);
        let start_path = params.start_path.unwrap_or_else(|| "game".to_string());

        let result = self
            .bridge
            .execute_command(
                "getDataModelPaginated",
                json!({
                    "startPath": start_path,
                    "maxDepth": max_depth,
                    "limit": limit,
                    "cursor": params.cursor,
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Read script source from a script instance in Studio. Works with Script, LocalScript, and ModuleScript.")]
    async fn studio_get_script_source(
        &self,
        Parameters(params): Parameters<StudioGetScriptSourceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_script_source");
        let result = self.studio_get_script_source_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_get_script_source_impl(
        &self,
        params: StudioGetScriptSourceParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command("getScriptSource", json!({ "path": params.path }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Modify script source in Studio with undo support. Creates a waypoint for undo/redo functionality.")]
    async fn studio_modify_script(
        &self,
        Parameters(params): Parameters<StudioModifyScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_modify_script");
        let result = self.studio_modify_script_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_modify_script_impl(
        &self,
        params: StudioModifyScriptParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "modifyScript",
                json!({
                    "path": params.path,
                    "newSource": params.new_source,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Create a new instance in Studio. Supports setting initial properties and creates an undo waypoint.")]
    async fn studio_create_instance(
        &self,
        Parameters(params): Parameters<StudioCreateInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_create_instance");
        let result = self.studio_create_instance_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_create_instance_impl(
        &self,
        params: StudioCreateInstanceParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "createInstance",
                json!({
                    "className": params.class_name,
                    "parent": params.parent,
                    "name": params.name,
                    "properties": params.properties,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Set a property on an instance in Studio. Supports common property types and creates an undo waypoint.")]
    async fn studio_set_property(
        &self,
        Parameters(params): Parameters<StudioSetPropertyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_set_property");
        let result = self.studio_set_property_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_set_property_impl(
        &self,
        params: StudioSetPropertyParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "setProperty",
                json!({
                    "path": params.path,
                    "property": params.property,
                    "value": params.value,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Delete an instance from Studio. Creates an undo waypoint for recovery.")]
    async fn studio_delete_instance(
        &self,
        Parameters(params): Parameters<StudioDeleteInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_delete_instance");
        let result = self.studio_delete_instance_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_delete_instance_impl(
        &self,
        params: StudioDeleteInstanceParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "deleteInstance",
                json!({
                    "path": params.path,
                    "recordUndo": params.record_undo.unwrap_or(true)
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Find all instances of a specific class in Studio. Searches descendants from the specified root.")]
    async fn studio_find_instances(
        &self,
        Parameters(params): Parameters<StudioFindInstancesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_find_instances");
        let result = self.studio_find_instances_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_find_instances_impl(
        &self,
        params: StudioFindInstancesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "findInstances",
                json!({
                    "className": params.class_name,
                    "root": params.root
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === CLOUD TOOLS (3) ===
    // These tools use the Roblox Open Cloud API for CI/CD automation.
    // Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable to be set.

    #[tool(description = "Publish a place file (.rbxl) to Roblox via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable.")]
    async fn cloud_publish_place(
        &self,
        Parameters(params): Parameters<CloudPublishPlaceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_publish_place");
        let result = self.cloud_publish_place_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_publish_place_impl(
        &self,
        params: CloudPublishPlaceParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if cloud client is available
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let path = PathBuf::from(&params.rbxl_path);
        let result = client
            .publish_place(params.universe_id, params.place_id, &path)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "version_number": result.version_number,
                "universe_id": params.universe_id,
                "place_id": params.place_id
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Upload an asset (image, model, or audio) to Roblox via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable.")]
    async fn cloud_upload_asset(
        &self,
        Parameters(params): Parameters<CloudUploadAssetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_upload_asset");
        let result = self.cloud_upload_asset_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_upload_asset_impl(
        &self,
        params: CloudUploadAssetParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if cloud client is available
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        // Parse asset type
        let asset_type = AssetType::from_str(&params.asset_type)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let path = PathBuf::from(&params.file_path);
        let result = client
            .upload_asset(
                asset_type,
                &path,
                &params.name,
                &params.description,
                params.creator_id,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "operation_path": result.path,
                "done": result.done
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Get a value from a Roblox DataStore via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable.")]
    async fn cloud_datastore_get(
        &self,
        Parameters(params): Parameters<CloudDatastoreGetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_datastore_get");
        let result = self.cloud_datastore_get_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_datastore_get_impl(
        &self,
        params: CloudDatastoreGetParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if cloud client is available
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let result = client
            .datastore_get(
                params.universe_id,
                &params.datastore_name,
                &params.key,
                params.scope.as_deref(),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "value": result.value,
                "version": result.version,
                "created_time": result.created_time,
                "updated_time": result.updated_time
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Set a value in a Roblox DataStore via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable.")]
    async fn cloud_datastore_set(
        &self,
        Parameters(params): Parameters<CloudDatastoreSetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_datastore_set");
        let result = self.cloud_datastore_set_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_datastore_set_impl(
        &self,
        params: CloudDatastoreSetParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if cloud client is available
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let result = client
            .datastore_set(
                params.universe_id,
                &params.datastore_name,
                &params.key,
                params.value.clone(),
                params.scope.as_deref(),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "value": result.value,
                "version": result.version,
                "created_time": result.created_time,
                "updated_time": result.updated_time
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(description = "Publish a message to a Roblox MessagingService topic via Open Cloud API. Messages are delivered to all servers subscribed to the topic. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable.")]
    async fn cloud_messaging_publish(
        &self,
        Parameters(params): Parameters<CloudMessagingPublishParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_messaging_publish");
        let result = self.cloud_messaging_publish_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_messaging_publish_impl(
        &self,
        params: CloudMessagingPublishParams,
    ) -> Result<CallToolResult, ErrorData> {
        // Check if cloud client is available
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let result = client
            .messaging_publish(params.universe_id, &params.topic, params.message.clone())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": result.success,
                "topic": result.topic,
                "universe_id": params.universe_id,
                "message_preview": if params.message.to_string().len() > 100 {
                    format!("{}...", &params.message.to_string()[..100])
                } else {
                    params.message.to_string()
                }
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === WATCHER TOOLS (1) ===
    // These tools provide real-time file change detection.

    #[tool(description = "Poll for recent file changes detected by the file watcher. Returns queued changes (created, modified, deleted .luau files).")]
    async fn fs_watch_changes(
        &self,
        Parameters(params): Parameters<FsWatchChangesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("fs_watch_changes");
        let result = self.fs_watch_changes_impl(params).await;
        call.finish_with(result).await
    }

    async fn fs_watch_changes_impl(
        &self,
        params: FsWatchChangesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let watcher = self.file_watcher.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "File watcher not available on this platform".to_string(),
                None,
            )
        })?;

        let limit = params.limit.unwrap_or(100);
        let changes = watcher.poll_changes(limit).await;
        let pending = watcher.pending_count().await;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "changes": changes,
                "returned_count": changes.len(),
                "pending_count": pending
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === METRICS TOOLS (1) ===
    // These tools provide server monitoring and health information.

    #[tool(description = "Get server metrics including tool execution counts, durations, and error rates.")]
    async fn server_get_metrics(&self) -> Result<CallToolResult, ErrorData> {
        // Note: We don't instrument server_get_metrics itself to avoid recursion
        // and because it's a meta-tool for observing metrics, not a primary tool
        let snapshot = self.metrics.snapshot().await;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&snapshot)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }
}

#[tool_handler]
impl<B: StudioBridge + Clone + 'static, L: Linter + Clone + 'static> ServerHandler
    for RobloxMcpServer<B, L>
{
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Roblox Studio MCP Server. Provides 7 filesystem tools for .luau script management (including Selene linting), 9 Studio bridge tools for live Roblox Studio interaction (including paginated DataModel), 3 Open Cloud tools (publish, asset upload, datastore) for CI/CD automation, 1 file watcher tool for change detection, and 1 metrics tool for monitoring. Studio tools require the plugin to be connected. Cloud tools require ROBLOX_OPEN_CLOUD_API_KEY."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::Duration;

    fn create_test_server(project_root: PathBuf) -> RobloxMcpServer {
        let bridge = Arc::new(PluginBridge::new());
        RobloxMcpServer::new(bridge, project_root)
    }

    fn create_test_server_with_stale_bridge(project_root: PathBuf) -> RobloxMcpServer {
        let bridge = Arc::new(PluginBridge::new());
        // Make heartbeat stale synchronously by blocking
        let bridge_clone = bridge.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                *bridge_clone.last_heartbeat.write().await =
                    std::time::Instant::now().checked_sub(Duration::from_secs(15)).unwrap();
            });
        })
        .join()
        .unwrap();
        RobloxMcpServer::new(bridge, project_root)
    }

    // === FILESYSTEM TOOL TESTS ===

    #[tokio::test]
    async fn test_fs_get_tree_success() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Create test structure
        std::fs::create_dir(project_root.join("src")).unwrap();
        std::fs::write(project_root.join("src/main.luau"), "-- main").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(2),
        };

        let result = server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok(), "fs_get_tree should succeed");

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());
    }

    #[tokio::test]
    async fn test_fs_get_tree_invalid_path() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root);

        let params = FsGetTreeParams {
            path: "/nonexistent/path".to_string(),
            max_depth: None,
        };

        let result = server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_err(), "fs_get_tree should fail for invalid path");
    }

    #[tokio::test]
    async fn test_fs_read_script_success() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("test.luau");
        std::fs::write(&script_path, "-- test script\nlocal x = 42").unwrap();

        let server = create_test_server(project_root);

        let params = FsReadScriptParams {
            file_path: script_path.display().to_string(),
        };

        let result = server.fs_read_script(Parameters(params)).await;
        assert!(result.is_ok(), "fs_read_script should succeed");

        let call_result = result.unwrap();
        let content = &call_result.content[0];
        // Content is Annotated<RawContent>, deref to match on RawContent
        if let RawContent::Text(text_content) = &**content {
            assert!(text_content.text.contains("test script"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_fs_read_script_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let params = FsReadScriptParams {
            file_path: project_root.join("nonexistent.luau").display().to_string(),
        };

        let result = server.fs_read_script(Parameters(params)).await;
        assert!(result.is_err(), "fs_read_script should fail for nonexistent file");
    }

    #[tokio::test]
    async fn test_fs_write_script_success() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let params = FsWriteScriptParams {
            file_path: project_root.join("new_script.luau").display().to_string(),
            content: "-- new script content".to_string(),
            create_directories: Some(false),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(result.is_ok(), "fs_write_script should succeed");

        // Verify file was created
        assert!(project_root.join("new_script.luau").exists());
    }

    #[tokio::test]
    async fn test_fs_write_script_non_luau_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let params = FsWriteScriptParams {
            file_path: project_root.join("script.txt").display().to_string(),
            content: "not a luau file".to_string(),
            create_directories: Some(false),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(result.is_err(), "fs_write_script should reject non-.luau files");
    }

    #[tokio::test]
    async fn test_fs_write_script_creates_directories() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let nested_path = project_root.join("deep/nested/dir/script.luau");

        let params = FsWriteScriptParams {
            file_path: nested_path.display().to_string(),
            content: "-- nested script".to_string(),
            create_directories: Some(true),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(result.is_ok(), "fs_write_script should create directories");
        assert!(nested_path.exists());
    }

    #[tokio::test]
    async fn test_fs_write_script_fails_without_create_directories() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let nested_path = project_root.join("missing/parent/script.luau");

        let params = FsWriteScriptParams {
            file_path: nested_path.display().to_string(),
            content: "-- should fail".to_string(),
            create_directories: Some(false),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(result.is_err(), "fs_write_script should fail when parent doesn't exist");
    }

    #[tokio::test]
    async fn test_fs_delete_script_success() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("to_delete.luau");
        std::fs::write(&script_path, "-- will be deleted").unwrap();

        let server = create_test_server(project_root);

        let params = FsDeleteScriptParams {
            file_path: script_path.display().to_string(),
        };

        let result = server.fs_delete_script(Parameters(params)).await;
        assert!(result.is_ok(), "fs_delete_script should succeed");
        assert!(!script_path.exists(), "File should be deleted");
    }

    #[tokio::test]
    async fn test_fs_delete_script_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let params = FsDeleteScriptParams {
            file_path: project_root.join("nonexistent.luau").display().to_string(),
        };

        let result = server.fs_delete_script(Parameters(params)).await;
        assert!(result.is_err(), "fs_delete_script should fail for nonexistent file");
    }

    #[tokio::test]
    async fn test_fs_delete_script_non_luau_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let txt_file = project_root.join("file.txt");
        std::fs::write(&txt_file, "not a luau file").unwrap();

        let server = create_test_server(project_root);

        let params = FsDeleteScriptParams {
            file_path: txt_file.display().to_string(),
        };

        let result = server.fs_delete_script(Parameters(params)).await;
        assert!(result.is_err(), "fs_delete_script should reject non-.luau files");
    }

    #[tokio::test]
    async fn test_fs_search_content_finds_matches() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        std::fs::write(project_root.join("test.luau"), "function foo()\nend").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsSearchContentParams {
            path: project_root.display().to_string(),
            pattern: "function".to_string(),
            extension: "luau".to_string(),
        };

        let result = server.fs_search_content(Parameters(params)).await;
        assert!(result.is_ok(), "fs_search_content should succeed");

        let call_result = result.unwrap();
        let content = &call_result.content[0];
        if let RawContent::Text(text_content) = &**content {
            assert!(text_content.text.contains("matches"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_fs_search_content_invalid_regex() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        let params = FsSearchContentParams {
            path: project_root.display().to_string(),
            pattern: "[invalid regex".to_string(), // Missing closing bracket
            extension: "luau".to_string(),
        };

        let result = server.fs_search_content(Parameters(params)).await;
        assert!(result.is_err(), "fs_search_content should fail on invalid regex");
    }

    #[tokio::test]
    async fn test_fs_get_changes_success() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        std::fs::write(project_root.join("script1.luau"), "-- script 1").unwrap();
        std::fs::write(project_root.join("script2.luau"), "-- script 2").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsGetChangesParams {
            path: project_root.display().to_string(),
        };

        let result = server.fs_get_changes(Parameters(params)).await;
        assert!(result.is_ok(), "fs_get_changes should succeed");

        let call_result = result.unwrap();
        let content = &call_result.content[0];
        if let RawContent::Text(text_content) = &**content {
            assert!(text_content.text.contains("file_count"));
        } else {
            panic!("Expected text content");
        }
    }

    // === STUDIO TOOL TESTS (with stale bridge) ===

    #[tokio::test]
    async fn test_studio_get_selection_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let result = server.studio_get_selection().await;
        assert!(result.is_err(), "studio_get_selection should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioGetDataModelParams { max_depth: Some(3) };

        let result = server.studio_get_datamodel(Parameters(params)).await;
        assert!(result.is_err(), "studio_get_datamodel should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_get_script_source_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioGetScriptSourceParams {
            path: "game.ServerScriptService.Main".to_string(),
        };

        let result = server.studio_get_script_source(Parameters(params)).await;
        assert!(result.is_err(), "studio_get_script_source should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_modify_script_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioModifyScriptParams {
            path: "game.ServerScriptService.Main".to_string(),
            new_source: "-- new source".to_string(),
            record_undo: Some(true),
        };

        let result = server.studio_modify_script(Parameters(params)).await;
        assert!(result.is_err(), "studio_modify_script should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_create_instance_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioCreateInstanceParams {
            class_name: "Part".to_string(),
            parent: "game.Workspace".to_string(),
            name: "TestPart".to_string(),
            properties: None,
            record_undo: Some(true),
        };

        let result = server.studio_create_instance(Parameters(params)).await;
        assert!(result.is_err(), "studio_create_instance should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_set_property_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioSetPropertyParams {
            path: "game.Workspace.Part".to_string(),
            property: "Name".to_string(),
            value: json!("NewName"),
            record_undo: Some(true),
        };

        let result = server.studio_set_property(Parameters(params)).await;
        assert!(result.is_err(), "studio_set_property should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_delete_instance_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioDeleteInstanceParams {
            path: "game.Workspace.Part".to_string(),
            record_undo: Some(true),
        };

        let result = server.studio_delete_instance(Parameters(params)).await;
        assert!(result.is_err(), "studio_delete_instance should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_studio_find_instances_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioFindInstancesParams {
            class_name: "Part".to_string(),
            root: Some("game.Workspace".to_string()),
        };

        let result = server.studio_find_instances(Parameters(params)).await;
        assert!(result.is_err(), "studio_find_instances should fail when bridge is stale");
    }

    #[tokio::test]
    async fn test_server_get_info() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server(temp_dir.path().to_path_buf());

        let info = server.get_info();
        assert!(info.instructions.is_some());
        assert!(info.instructions.unwrap().contains("Roblox Studio MCP Server"));
    }

    #[tokio::test]
    async fn test_server_new() {
        let temp_dir = TempDir::new().unwrap();
        let bridge = Arc::new(PluginBridge::new());
        let server = RobloxMcpServer::new(bridge, temp_dir.path().to_path_buf());

        // Just verify it constructs without panic
        let _info = server.get_info();
    }

    // === MOCK BRIDGE TESTS (Studio tool success paths) ===

    use crate::bridge::mock::MockBridge;

    fn create_mock_server(project_root: PathBuf) -> RobloxMcpServer<MockBridge, SeleneLinter> {
        let mock = MockBridge::new();
        RobloxMcpServer::with_mock_bridge(Arc::new(mock), project_root)
    }

    fn create_mock_server_with_responses(
        project_root: PathBuf,
        responses: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
    ) -> (RobloxMcpServer<MockBridge, SeleneLinter>, Arc<MockBridge>) {
        let mock = Arc::new(MockBridge::new());
        for (action, response) in responses {
            mock.set_response(action, response);
        }
        let server = RobloxMcpServer::with_mock_bridge(mock.clone(), project_root);
        (server, mock)
    }

    #[tokio::test]
    async fn test_studio_get_selection_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("getSelection", json!({
                "selected": [
                    {"Name": "Part1", "ClassName": "Part", "Path": "game.Workspace.Part1"},
                    {"Name": "Part2", "ClassName": "Part", "Path": "game.Workspace.Part2"}
                ]
            }))],
        );

        let result = server.studio_get_selection().await;
        assert!(result.is_ok(), "studio_get_selection should succeed");

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(text_content.text.contains("Part1"));
            assert!(text_content.text.contains("Part2"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("getSelection"));
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("getDataModel", json!({
                "Name": "DataModel",
                "ClassName": "DataModel",
                "Children": [
                    {"Name": "Workspace", "ClassName": "Workspace", "Children": []}
                ]
            }))],
        );

        let params = StudioGetDataModelParams { max_depth: Some(2) };
        let result = server.studio_get_datamodel(Parameters(params)).await;
        assert!(result.is_ok(), "studio_get_datamodel should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("Workspace"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("getDataModel"));
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_paginated_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("getDataModelPaginated", json!({
                "instances": [
                    {"Name": "Part1", "ClassName": "Part", "Path": "game.Workspace.Part1"},
                    {"Name": "Part2", "ClassName": "Part", "Path": "game.Workspace.Part2"}
                ],
                "cursor": "next_page_token",
                "hasMore": true
            }))],
        );

        let params = StudioGetDataModelPaginatedParams {
            start_path: Some("game.Workspace".to_string()),
            max_depth: Some(2),
            limit: Some(100),
            cursor: None,
        };
        let result = server.studio_get_datamodel_paginated(Parameters(params)).await;
        assert!(result.is_ok(), "studio_get_datamodel_paginated should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("Part1"));
            assert!(text_content.text.contains("cursor"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("getDataModelPaginated"));
    }

    #[tokio::test]
    async fn test_studio_get_script_source_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("getScriptSource", json!({
                "source": "-- Main script\nprint('Hello World')",
                "path": "game.ServerScriptService.Main"
            }))],
        );

        let params = StudioGetScriptSourceParams {
            path: "game.ServerScriptService.Main".to_string(),
        };
        let result = server.studio_get_script_source(Parameters(params)).await;
        assert!(result.is_ok(), "studio_get_script_source should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("Hello World"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("getScriptSource"));
    }

    #[tokio::test]
    async fn test_studio_modify_script_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("modifyScript", json!({
                "success": true,
                "path": "game.ServerScriptService.Main",
                "undoCreated": true
            }))],
        );

        let params = StudioModifyScriptParams {
            path: "game.ServerScriptService.Main".to_string(),
            new_source: "-- Updated script\nprint('Updated')".to_string(),
            record_undo: Some(true),
        };
        let result = server.studio_modify_script(Parameters(params)).await;
        assert!(result.is_ok(), "studio_modify_script should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("success"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("modifyScript"));
    }

    #[tokio::test]
    async fn test_studio_create_instance_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("createInstance", json!({
                "success": true,
                "instance": {
                    "Name": "NewPart",
                    "ClassName": "Part",
                    "Path": "game.Workspace.NewPart"
                }
            }))],
        );

        let params = StudioCreateInstanceParams {
            class_name: "Part".to_string(),
            parent: "game.Workspace".to_string(),
            name: "NewPart".to_string(),
            properties: Some(json!({"Anchored": true})),
            record_undo: Some(true),
        };
        let result = server.studio_create_instance(Parameters(params)).await;
        assert!(result.is_ok(), "studio_create_instance should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("NewPart"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("createInstance"));
    }

    #[tokio::test]
    async fn test_studio_set_property_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("setProperty", json!({
                "success": true,
                "path": "game.Workspace.Part",
                "property": "Name",
                "oldValue": "Part",
                "newValue": "RenamedPart"
            }))],
        );

        let params = StudioSetPropertyParams {
            path: "game.Workspace.Part".to_string(),
            property: "Name".to_string(),
            value: json!("RenamedPart"),
            record_undo: Some(true),
        };
        let result = server.studio_set_property(Parameters(params)).await;
        assert!(result.is_ok(), "studio_set_property should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("RenamedPart"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("setProperty"));
    }

    #[tokio::test]
    async fn test_studio_delete_instance_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("deleteInstance", json!({
                "success": true,
                "deletedPath": "game.Workspace.Part",
                "undoCreated": true
            }))],
        );

        let params = StudioDeleteInstanceParams {
            path: "game.Workspace.Part".to_string(),
            record_undo: Some(true),
        };
        let result = server.studio_delete_instance(Parameters(params)).await;
        assert!(result.is_ok(), "studio_delete_instance should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("success"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("deleteInstance"));
    }

    #[tokio::test]
    async fn test_studio_find_instances_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("findInstances", json!({
                "instances": [
                    {"Name": "Part1", "ClassName": "Part", "Path": "game.Workspace.Part1"},
                    {"Name": "Part2", "ClassName": "Part", "Path": "game.Workspace.Part2"},
                    {"Name": "Part3", "ClassName": "Part", "Path": "game.Workspace.Folder.Part3"}
                ],
                "count": 3
            }))],
        );

        let params = StudioFindInstancesParams {
            class_name: "Part".to_string(),
            root: Some("game.Workspace".to_string()),
        };
        let result = server.studio_find_instances(Parameters(params)).await;
        assert!(result.is_ok(), "studio_find_instances should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("Part1"));
            assert!(text_content.text.contains("Part2"));
            assert!(text_content.text.contains("Part3"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("findInstances"));
    }

    #[tokio::test]
    async fn test_studio_tool_with_disconnected_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let mock = Arc::new(MockBridge::new());
        mock.set_disconnected();

        let server = RobloxMcpServer::with_mock_bridge(mock.clone(), temp_dir.path().to_path_buf());

        let result = server.studio_get_selection().await;
        assert!(result.is_err(), "Should fail when bridge is disconnected");
    }

    #[tokio::test]
    async fn test_studio_tool_verifies_params() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("setProperty", json!({"success": true}))],
        );

        let params = StudioSetPropertyParams {
            path: "game.Workspace.SpecificPart".to_string(),
            property: "Transparency".to_string(),
            value: json!(0.5),
            record_undo: Some(false),
        };
        server.studio_set_property(Parameters(params)).await.unwrap();

        // Verify the call was recorded with correct params
        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.action, "setProperty");
        assert_eq!(last_call.params["path"], "game.Workspace.SpecificPart");
        assert_eq!(last_call.params["property"], "Transparency");
        assert_eq!(last_call.params["value"], 0.5);
        assert_eq!(last_call.params["recordUndo"], false);
    }

    #[tokio::test]
    async fn test_server_metrics_with_mock_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let (server, _mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [
                ("getSelection", json!({"selected": []})),
                ("getDataModel", json!({"Children": []})),
            ],
        );

        // Make some calls
        server.studio_get_selection().await.unwrap();
        server.studio_get_datamodel(Parameters(StudioGetDataModelParams { max_depth: None })).await.unwrap();

        // Verify metrics are tracked
        let metrics_result = server.server_get_metrics().await;
        assert!(metrics_result.is_ok());
    }

    // === HEALTH CHECK TESTS ===

    #[tokio::test]
    async fn test_studio_health_check_connected() {
        let temp_dir = TempDir::new().unwrap();
        let mock = Arc::new(MockBridge::new());
        // Mock bridge starts connected by default

        let server = RobloxMcpServer::with_mock_bridge(mock, temp_dir.path().to_path_buf());

        let result = server.studio_health_check().await;
        assert!(result.is_ok(), "studio_health_check should succeed");

        let call_result = result.unwrap();
        // is_error should be Some(false) when connected
        assert_eq!(call_result.is_error, Some(false));

        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(text_content.text.contains("\"connected\": true"));
            assert!(text_content.text.contains("connected and responsive"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_studio_health_check_disconnected() {
        let temp_dir = TempDir::new().unwrap();
        let mock = Arc::new(MockBridge::new());
        mock.set_disconnected();

        let server = RobloxMcpServer::with_mock_bridge(mock, temp_dir.path().to_path_buf());

        let result = server.studio_health_check().await;
        assert!(result.is_ok(), "studio_health_check should succeed even when disconnected");

        let call_result = result.unwrap();
        // is_error should be Some(true) when disconnected
        assert_eq!(call_result.is_error, Some(true));

        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(text_content.text.contains("\"connected\": false"));
            assert!(text_content.text.contains("not connected"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_studio_health_check_records_metrics() {
        let temp_dir = TempDir::new().unwrap();
        let mock = Arc::new(MockBridge::new());

        let server = RobloxMcpServer::with_mock_bridge(mock.clone(), temp_dir.path().to_path_buf());

        // Check while connected
        server.studio_health_check().await.unwrap();

        // Disconnect and check again
        mock.set_disconnected();
        server.studio_health_check().await.unwrap();

        // Reconnect and check
        mock.set_connected();
        server.studio_health_check().await.unwrap();

        // Verify connection metrics were recorded
        let metrics_result = server.server_get_metrics().await;
        assert!(metrics_result.is_ok());

        if let RawContent::Text(text_content) = &*metrics_result.unwrap().content[0] {
            // Should have connection metrics with 3 checks (2 connected, 1 disconnected)
            assert!(text_content.text.contains("connection"));
        } else {
            panic!("Expected text content");
        }
    }

    // === TEST UTILITY TESTS ===

    #[tokio::test]
    async fn test_create_mock_server_basic() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        // Verify server is functional
        let info = server.get_info();
        assert!(info.instructions.is_some());
        assert!(info.instructions.as_ref().unwrap().contains("Roblox Studio MCP Server"));
    }

    #[tokio::test]
    async fn test_with_mock_bridge_and_linter() {
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Create test file for linting (must be .luau extension)
        let script_path = project_root.join("test.luau");
        std::fs::write(&script_path, "local x = 1").unwrap();

        let mock_bridge = Arc::new(MockBridge::new());
        let mock_linter = MockLinter::with_warnings(vec![("unused_variable", "x is never used", 1)]);

        let server = RobloxMcpServer::with_mock_bridge_and_linter(
            mock_bridge.clone(),
            project_root,
            mock_linter.clone(),
        );

        // Verify server was created with custom linter
        let info = server.get_info();
        assert!(info.instructions.is_some());

        // Verify linter injection works by linting a file (use full path)
        let params = FsLintScriptParams {
            file_path: script_path.display().to_string(),
            config_path: None,
        };
        let result = server.fs_lint_script_impl(params).await;
        assert!(result.is_ok(), "fs_lint_script_impl failed: {:?}", result.err());

        // Verify custom linter was used (it should have recorded the call)
        assert!(mock_linter.call_count() > 0);
    }
}
