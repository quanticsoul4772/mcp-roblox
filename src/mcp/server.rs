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
use crate::cloud::CloudClient;
use crate::cloud::OpenCloudClient;
use crate::mcp::instrumentation::InstrumentedCall;
use crate::mcp::params::{
    // Cloud params
    CloudDatastoreGetParams,
    CloudDatastoreSetParams,
    CloudGetUniverseParams,
    CloudMessagingPublishParams,
    CloudOrderedDatastoreDeleteParams,
    CloudOrderedDatastoreIncrementParams,
    CloudOrderedDatastoreListParams,
    CloudOrderedDatastoreSetParams,
    CloudPublishPlaceParams,
    CloudRestartServersParams,
    CloudUploadAssetParams,
    // Filesystem params
    FsDeleteScriptParams,
    FsGetChangesParams,
    FsGetTreeParams,
    FsLintScriptParams,
    FsReadScriptParams,
    FsSearchContentParams,
    FsWatchChangesParams,
    FsWriteScriptParams,
    // Toolchain params
    MoonwaveBuildParams,
    RojoBuildParams,
    RojoSourcemapParams,
    StyluaFormatParams,
    WallyInstallParams,
    WallyUpdateParams,
    // Studio params
    StudioCreateInstanceParams,
    StudioDeleteInstanceParams,
    StudioFindInstancesParams,
    StudioGetBoundsParams,
    StudioGetDataModelPaginatedParams,
    StudioGetDataModelParams,
    StudioGetOutputParams,
    StudioGetPropertiesParams,
    StudioGetScriptSourceParams,
    StudioModifyScriptParams,
    StudioSetPropertyParams,
};
use crate::metrics::ServerMetrics;
use crate::tools::filesystem::{build_tree, read_script, validate_output_path, validate_path, write_script};
use crate::tools::formatting::{Formatter, StyLuaFormatter};
use crate::tools::linting::{Linter, SeleneLinter};
use crate::tools::moonwave::{DefaultMoonwaveRunner, MoonwaveRunner};
use crate::tools::rojo::{DefaultRojoRunner, RojoRunner};
use crate::tools::wally::{DefaultWallyRunner, WallyRunner};
use crate::watcher::FileWatcher;

/// Roblox MCP Server with injectable dependencies
///
/// Generic over:
/// - `B`: StudioBridge implementation (default: PluginBridge)
/// - `L`: Linter implementation (default: SeleneLinter)
/// - `F`: Formatter implementation (default: StyLuaFormatter)
/// - `R`: RojoRunner implementation (default: DefaultRojoRunner)
/// - `W`: WallyRunner implementation (default: DefaultWallyRunner)
/// - `M`: MoonwaveRunner implementation (default: DefaultMoonwaveRunner)
///
/// This allows injecting mock implementations for testing while keeping
/// production code unchanged.
#[derive(Clone)]
pub struct RobloxMcpServer<
    B: StudioBridge + Clone = PluginBridge,
    L: Linter + Clone = SeleneLinter,
    F: Formatter + Clone = StyLuaFormatter,
    R: RojoRunner + Clone = DefaultRojoRunner,
    W: WallyRunner + Clone = DefaultWallyRunner,
    M: MoonwaveRunner + Clone = DefaultMoonwaveRunner,
> {
    tool_router: ToolRouter<Self>,
    bridge: Arc<B>,
    project_root: PathBuf,
    /// Open Cloud client for CI/CD operations (optional - only if API key configured)
    /// Uses trait object for testability with MockCloudClient
    cloud_client: Option<Arc<dyn CloudClient>>,
    /// File watcher for real-time change detection (optional - may fail on some platforms)
    file_watcher: Option<Arc<FileWatcher>>,
    /// Server metrics for monitoring
    metrics: Arc<ServerMetrics>,
    /// Linter for Luau script analysis
    linter: L,
    /// Formatter for Luau code formatting
    formatter: F,
    /// Rojo runner for project builds
    rojo: R,
    /// Wally runner for package management
    wally: W,
    /// Moonwave runner for documentation generation
    moonwave: M,
}

/// Production constructor for RobloxMcpServer with all default implementations
impl
    RobloxMcpServer<
        PluginBridge,
        SeleneLinter,
        StyLuaFormatter,
        DefaultRojoRunner,
        DefaultWallyRunner,
        DefaultMoonwaveRunner,
    >
{
    pub fn new(bridge: Arc<PluginBridge>, project_root: PathBuf) -> Self {
        // Initialize cloud client with explicit logging on failure
        // Cloud tools will check availability and return clear error to users
        // Cast to trait object for consistency with test injection
        let cloud_client: Option<Arc<dyn CloudClient>> = match OpenCloudClient::new() {
            Ok(client) => Some(Arc::new(client) as Arc<dyn CloudClient>),
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

        // Initialize toolchain components
        let linter = SeleneLinter::new();
        let formatter = StyLuaFormatter::new();
        let rojo = DefaultRojoRunner::new();
        let wally = DefaultWallyRunner::new();
        let moonwave = DefaultMoonwaveRunner::new();

        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client,
            file_watcher,
            metrics,
            linter,
            formatter,
            rojo,
            wally,
            moonwave,
        }
    }
}

/// Generic implementation for any StudioBridge with default toolchain implementations
impl<B: StudioBridge + Clone + 'static>
    RobloxMcpServer<
        B,
        SeleneLinter,
        StyLuaFormatter,
        DefaultRojoRunner,
        DefaultWallyRunner,
        DefaultMoonwaveRunner,
    >
{
    /// Create a test server with a mock bridge (uses default toolchain implementations)
    ///
    /// This constructor is used for testing to inject mock bridge dependencies
    /// while using production toolchain implementations.
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
            formatter: StyLuaFormatter::new(),
            rojo: DefaultRojoRunner::new(),
            wally: DefaultWallyRunner::new(),
            moonwave: DefaultMoonwaveRunner::new(),
        }
    }
}

/// Generic implementation for any StudioBridge and Linter with default toolchain implementations
impl<B: StudioBridge + Clone + 'static, L: Linter + Clone + 'static>
    RobloxMcpServer<
        B,
        L,
        StyLuaFormatter,
        DefaultRojoRunner,
        DefaultWallyRunner,
        DefaultMoonwaveRunner,
    >
{
    /// Create a test server with a mock bridge and custom linter
    ///
    /// This constructor is used for testing to inject both bridge and linter dependencies.
    /// Uses default toolchain implementations.
    #[cfg(test)]
    pub fn with_mock_bridge_and_linter(bridge: Arc<B>, project_root: PathBuf, linter: L) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client: None,
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
            linter,
            formatter: StyLuaFormatter::new(),
            rojo: DefaultRojoRunner::new(),
            wally: DefaultWallyRunner::new(),
            moonwave: DefaultMoonwaveRunner::new(),
        }
    }

    /// Create a test server with mock bridge, cloud client, and linter
    ///
    /// Uses default toolchain implementations.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn with_mocks(
        bridge: Arc<B>,
        project_root: PathBuf,
        cloud_client: Option<Arc<dyn CloudClient>>,
        linter: L,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            bridge,
            project_root,
            cloud_client,
            file_watcher: None,
            metrics: Arc::new(ServerMetrics::new()),
            linter,
            formatter: StyLuaFormatter::new(),
            rojo: DefaultRojoRunner::new(),
            wally: DefaultWallyRunner::new(),
            moonwave: DefaultMoonwaveRunner::new(),
        }
    }
}

/// Generic implementation for all variants - shared utility methods
impl<
        B: StudioBridge + Clone + 'static,
        L: Linter + Clone + 'static,
        F: Formatter + Clone + 'static,
        R: RojoRunner + Clone + 'static,
        W: WallyRunner + Clone + 'static,
        M: MoonwaveRunner + Clone + 'static,
    > RobloxMcpServer<B, L, F, R, W, M>
{
    /// Set shared metrics for cross-component tracking (e.g., late plugin results)
    ///
    /// When metrics are shared between RobloxMcpServer and PluginBridge,
    /// late result tracking becomes available for operational visibility.
    ///
    /// # Example
    /// ```ignore
    /// let metrics = Arc::new(ServerMetrics::new());
    /// let bridge = Arc::new(PluginBridge::with_metrics(metrics.clone()));
    /// let server = RobloxMcpServer::new(bridge, root).with_shared_metrics(metrics);
    /// ```
    pub fn with_shared_metrics(mut self, metrics: Arc<ServerMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    /// Set a custom cloud client (for testing cloud tool success paths)
    ///
    /// # Example
    /// ```ignore
    /// let mock = Arc::new(MockCloudClient::new());
    /// mock.queue_datastore_get(Ok(entry));
    /// let server = create_test_server(root).with_cloud_client(mock);
    /// ```
    #[cfg(test)]
    pub fn with_cloud_client(mut self, client: Arc<dyn CloudClient>) -> Self {
        self.cloud_client = Some(client);
        self
    }

    /// Start instrumentation for a tool call
    fn start_instrumentation(&self, tool_name: &str) -> InstrumentedCall {
        InstrumentedCall::start(self.metrics.clone(), tool_name)
    }
}

#[tool_router]
impl<
        B: StudioBridge + Clone + 'static,
        L: Linter + Clone + 'static,
        F: Formatter + Clone + 'static,
        R: RojoRunner + Clone + 'static,
        W: WallyRunner + Clone + 'static,
        M: MoonwaveRunner + Clone + 'static,
    > RobloxMcpServer<B, L, F, R, W, M>
{
    // === FILESYSTEM TOOLS (7) ===

    #[tool(
        description = "List project file structure with depth limits. Returns a tree of files and directories, plus any skipped entries."
    )]
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

    #[tool(
        description = "Write or create a Luau script file. Optionally create parent directories."
    )]
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

        fs::remove_file(&validated_path)
            .await
            .map_err(|e| ErrorData::internal_error(format!("Failed to delete file: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            json!({
                "deleted": validated_path.display().to_string(),
                "success": true
            })
            .to_string(),
        )]))
    }

    #[tool(
        description = "Search for patterns in script files using regex. Returns matching lines with context."
    )]
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
        let regex = Regex::new(&params.pattern)
            .map_err(|e| ErrorData::internal_error(format!("Invalid regex pattern: {e}"), None))?;

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

    #[tool(
        description = "Get file modification times for change detection. Returns a map of file paths to modification timestamps."
    )]
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

    #[tool(
        description = "Run Selene linter on a Luau script file. Returns diagnostics with errors and warnings. Requires 'selene' to be installed (cargo install selene)."
    )]
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

    #[tool(
        description = "Check if Roblox Studio plugin is connected and responsive. Use this before batch operations to avoid timeout errors."
    )]
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

    #[tool(
        description = "Get currently selected instances in Roblox Studio. Returns array of selected instances with Name, ClassName, and Path."
    )]
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

    #[tool(
        description = "Explore the live Studio DataModel hierarchy. Returns nested structure of instances with Name, ClassName, Path, and Children."
    )]
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

    #[tool(
        description = "Get DataModel with pagination to avoid context overflow for large hierarchies. Returns instances with a cursor for continuation."
    )]
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

    #[tool(
        description = "Read script source from a script instance in Studio. Works with Script, LocalScript, and ModuleScript."
    )]
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

    #[tool(
        description = "Read properties from any instance in Studio. Returns the specified properties or common properties for the class if none specified."
    )]
    async fn studio_get_properties(
        &self,
        Parameters(params): Parameters<StudioGetPropertiesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_properties");
        let result = self.studio_get_properties_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_get_properties_impl(
        &self,
        params: StudioGetPropertiesParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command(
                "getProperties",
                json!({
                    "path": params.path,
                    "properties": params.properties
                }),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Get the bounding box of a BasePart or Model in Studio. Returns center, size, min, max coordinates, and orientation."
    )]
    async fn studio_get_bounds(
        &self,
        Parameters(params): Parameters<StudioGetBoundsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_bounds");
        let result = self.studio_get_bounds_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_get_bounds_impl(
        &self,
        params: StudioGetBoundsParams,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .bridge
            .execute_command("getBounds", json!({ "path": params.path }))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Modify script source in Studio with undo support. Creates a waypoint for undo/redo functionality."
    )]
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

    #[tool(
        description = "Create a new instance in Studio. Supports setting initial properties and creates an undo waypoint."
    )]
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

    #[tool(
        description = "Set a property on an instance in Studio. Supports common property types and creates an undo waypoint."
    )]
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

    #[tool(
        description = "Find all instances of a specific class in Studio. Searches descendants from the specified root."
    )]
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

    #[tool(
        description = "Get recent Output window logs from Roblox Studio. Returns log entries with message, type, and timestamp."
    )]
    async fn studio_get_output(
        &self,
        Parameters(params): Parameters<StudioGetOutputParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("studio_get_output");
        let result = self.studio_get_output_impl(params).await;
        call.finish_with(result).await
    }

    async fn studio_get_output_impl(
        &self,
        params: StudioGetOutputParams,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = params.limit.unwrap_or(100);

        let result = self
            .bridge
            .execute_command("getOutput", json!({ "limit": limit }))
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

    #[tool(
        description = "Publish a place file (.rbxl) to Roblox via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
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

    #[tool(
        description = "Upload an asset (image, model, or audio) to Roblox via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
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

    #[tool(
        description = "Get a value from a Roblox DataStore via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
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

    #[tool(
        description = "Set a value in a Roblox DataStore via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
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

    #[tool(
        description = "Publish a message to a Roblox MessagingService topic via Open Cloud API. Messages are delivered to all servers subscribed to the topic. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
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

        client
            .messaging_publish(params.universe_id, &params.topic, params.message.clone())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "topic": params.topic,
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

    // === PHASE 1: ORDERED DATASTORE TOOLS (4) ===
    // OrderedDataStores are used for leaderboards and ranking systems.

    #[tool(
        description = "List entries from an OrderedDataStore via Open Cloud API. Returns entries sorted by value, commonly used for leaderboards. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
    async fn cloud_ordered_datastore_list(
        &self,
        Parameters(params): Parameters<CloudOrderedDatastoreListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_ordered_datastore_list");
        let result = self.cloud_ordered_datastore_list_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_ordered_datastore_list_impl(
        &self,
        params: CloudOrderedDatastoreListParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let result = client
            .ordered_datastore_list(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                params.max_page_size,
                params.page_token.as_deref(),
                params.order_by.as_deref(),
                params.filter.as_deref(),
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "entries": result.entries,
                "next_page_token": result.next_page_token
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Set a value in an OrderedDataStore via Open Cloud API. Creates or updates an entry with the specified key and numerical value. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
    async fn cloud_ordered_datastore_set(
        &self,
        Parameters(params): Parameters<CloudOrderedDatastoreSetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_ordered_datastore_set");
        let result = self.cloud_ordered_datastore_set_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_ordered_datastore_set_impl(
        &self,
        params: CloudOrderedDatastoreSetParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let result = client
            .ordered_datastore_set(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                &params.entry_id,
                params.value,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "path": result.path,
                "id": result.id,
                "value": result.value
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Atomically increment a value in an OrderedDataStore via Open Cloud API. Creates the entry if it doesn't exist. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
    async fn cloud_ordered_datastore_increment(
        &self,
        Parameters(params): Parameters<CloudOrderedDatastoreIncrementParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_ordered_datastore_increment");
        let result = self.cloud_ordered_datastore_increment_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_ordered_datastore_increment_impl(
        &self,
        params: CloudOrderedDatastoreIncrementParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let result = client
            .ordered_datastore_increment(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                &params.entry_id,
                params.increment,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "path": result.path,
                "id": result.id,
                "value": result.value
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Delete an entry from an OrderedDataStore via Open Cloud API. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
    async fn cloud_ordered_datastore_delete(
        &self,
        Parameters(params): Parameters<CloudOrderedDatastoreDeleteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_ordered_datastore_delete");
        let result = self.cloud_ordered_datastore_delete_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_ordered_datastore_delete_impl(
        &self,
        params: CloudOrderedDatastoreDeleteParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        client
            .ordered_datastore_delete(
                params.universe_id,
                &params.datastore_name,
                params.scope.as_deref(),
                &params.entry_id,
            )
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "deleted_entry_id": params.entry_id
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === PHASE 1: UNIVERSE TOOLS (2) ===
    // Universe API tools for game metadata and server management.

    #[tool(
        description = "Get information about a Roblox universe (game) via Open Cloud API. Returns metadata including name, description, ownership, and platform support. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
    async fn cloud_get_universe(
        &self,
        Parameters(params): Parameters<CloudGetUniverseParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_get_universe");
        let result = self.cloud_get_universe_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_get_universe_impl(
        &self,
        params: CloudGetUniverseParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        let info = client
            .get_universe(params.universe_id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "path": info.path,
                "display_name": info.display_name,
                "description": info.description,
                "create_time": info.create_time,
                "update_time": info.update_time,
                "visibility": info.visibility,
                "user": info.user,
                "group": info.group,
                "voice_chat_enabled": info.voice_chat_enabled,
                "age_rating": info.age_rating,
                "platforms": {
                    "desktop": info.desktop_enabled,
                    "mobile": info.mobile_enabled,
                    "tablet": info.tablet_enabled,
                    "vr": info.vr_enabled,
                    "console": info.console_enabled
                }
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Restart all game servers for a Roblox universe via Open Cloud API. Triggers a graceful restart - players will be disconnected and can rejoin. Requires ROBLOX_OPEN_CLOUD_API_KEY environment variable."
    )]
    async fn cloud_restart_servers(
        &self,
        Parameters(params): Parameters<CloudRestartServersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("cloud_restart_servers");
        let result = self.cloud_restart_servers_impl(params).await;
        call.finish_with(result).await
    }

    async fn cloud_restart_servers_impl(
        &self,
        params: CloudRestartServersParams,
    ) -> Result<CallToolResult, ErrorData> {
        let client = self.cloud_client.as_ref().ok_or_else(|| {
            ErrorData::internal_error(
                "Open Cloud not configured: ROBLOX_OPEN_CLOUD_API_KEY environment variable not set"
                    .to_string(),
                None,
            )
        })?;

        client
            .restart_universe_servers(params.universe_id)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "universe_id": params.universe_id,
                "message": "Server restart initiated. All servers will gracefully restart."
            }))
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === WATCHER TOOLS (1) ===
    // These tools provide real-time file change detection.

    #[tool(
        description = "Poll for recent file changes detected by the file watcher. Returns queued changes (created, modified, deleted .luau files)."
    )]
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

    // === TOOLCHAIN TOOLS (6) ===
    // External toolchain integration for StyLua, Rojo, Wally, Moonwave and other Luau development tools.
    // These tools require the corresponding binaries to be installed (cargo install stylua, aftman install rojo-rbx/rojo, etc).

    #[tool(
        description = "Format a Luau script using StyLua. Requires 'stylua' to be installed (cargo install stylua)."
    )]
    async fn stylua_format(
        &self,
        Parameters(params): Parameters<StyluaFormatParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("stylua_format");
        let result = self.stylua_format_impl(params).await;
        call.finish_with(result).await
    }

    async fn stylua_format_impl(
        &self,
        params: StyluaFormatParams,
    ) -> Result<CallToolResult, ErrorData> {
        use std::path::Path;

        let file_path = Path::new(&params.file_path);

        // Validate path is within project root
        let validated_path = validate_path(file_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate .luau extension
        if validated_path.extension().is_none_or(|ext| ext != "luau") {
            return Err(ErrorData::invalid_params(
                "Only .luau files can be formatted",
                None,
            ));
        }

        // Parse config path
        let config_path = params.config_path.as_ref().map(Path::new);

        // Default check_only to false
        let check_only = params.check_only.unwrap_or(false);

        // Run the formatter
        let result = self
            .formatter
            .format(&validated_path, config_path, check_only)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Build a Roblox project using Rojo. Requires 'rojo' to be installed. Generates .rbxl/.rbxlx or .rbxm/.rbxmx output files."
    )]
    async fn rojo_build(
        &self,
        Parameters(params): Parameters<RojoBuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("rojo_build");
        let result = self.rojo_build_impl(params).await;
        call.finish_with(result).await
    }

    async fn rojo_build_impl(&self, params: RojoBuildParams) -> Result<CallToolResult, ErrorData> {
        use std::path::Path;

        let project_path = Path::new(&params.project_path);
        let output_path = Path::new(&params.output_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Validate output path is within project root (file may not exist yet)
        let validated_output = validate_output_path(output_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Run rojo build
        let result = self
            .rojo
            .build(&validated_project, &validated_output)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Generate a Rojo sourcemap for a project. Requires 'rojo' to be installed. Returns JSON mapping between Roblox instances and filesystem locations."
    )]
    async fn rojo_sourcemap(
        &self,
        Parameters(params): Parameters<RojoSourcemapParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("rojo_sourcemap");
        let result = self.rojo_sourcemap_impl(params).await;
        call.finish_with(result).await
    }

    async fn rojo_sourcemap_impl(
        &self,
        params: RojoSourcemapParams,
    ) -> Result<CallToolResult, ErrorData> {
        use std::path::Path;

        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Parse optional output path
        let output_path = params.output_path.as_ref().map(|p| Path::new(p).to_path_buf());

        // Run rojo sourcemap
        let result = self
            .rojo
            .sourcemap(&validated_project, output_path.as_deref())
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Install packages from wally.toml. Requires 'wally' to be installed (aftman install UpliftGames/wally)."
    )]
    async fn wally_install(
        &self,
        Parameters(params): Parameters<WallyInstallParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("wally_install");
        let result = self.wally_install_impl(params).await;
        call.finish_with(result).await
    }

    async fn wally_install_impl(
        &self,
        params: WallyInstallParams,
    ) -> Result<CallToolResult, ErrorData> {
        use std::path::Path;

        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Run wally install
        let result = self
            .wally
            .install(&validated_project)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Update packages to latest compatible versions. Requires 'wally' to be installed (aftman install UpliftGames/wally)."
    )]
    async fn wally_update(
        &self,
        Parameters(params): Parameters<WallyUpdateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("wally_update");
        let result = self.wally_update_impl(params).await;
        call.finish_with(result).await
    }

    async fn wally_update_impl(
        &self,
        params: WallyUpdateParams,
    ) -> Result<CallToolResult, ErrorData> {
        use std::path::Path;

        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Run wally update
        let result = self
            .wally
            .update(&validated_project)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Build Moonwave documentation from source files. Requires 'moonwave' to be installed (npm install -g moonwave)."
    )]
    async fn moonwave_build(
        &self,
        Parameters(params): Parameters<MoonwaveBuildParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let call = self.start_instrumentation("moonwave_build");
        let result = self.moonwave_build_impl(params).await;
        call.finish_with(result).await
    }

    async fn moonwave_build_impl(
        &self,
        params: MoonwaveBuildParams,
    ) -> Result<CallToolResult, ErrorData> {
        use std::path::Path;

        let project_path = Path::new(&params.project_path);

        // Validate project path is within project root
        let validated_project = validate_path(project_path, &self.project_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Parse optional output directory
        let output_dir = params.output_dir.as_ref().map(Path::new);

        // Run moonwave build
        let result = self
            .moonwave
            .build(&validated_project, output_dir)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        )]))
    }

    // === METRICS TOOLS (1) ===
    // These tools provide server monitoring and health information.

    #[tool(
        description = "Get server metrics including tool execution counts, durations, and error rates."
    )]
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
impl<
        B: StudioBridge + Clone + 'static,
        L: Linter + Clone + 'static,
        F: Formatter + Clone + 'static,
        R: RojoRunner + Clone + 'static,
        W: WallyRunner + Clone + 'static,
        M: MoonwaveRunner + Clone + 'static,
    > ServerHandler for RobloxMcpServer<B, L, F, R, W, M>
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
                *bridge_clone.last_heartbeat.write().await = std::time::Instant::now()
                    .checked_sub(Duration::from_secs(15))
                    .unwrap();
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
        assert!(
            result.is_err(),
            "fs_read_script should fail for nonexistent file"
        );
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
        assert!(
            result.is_err(),
            "fs_write_script should reject non-.luau files"
        );
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
        assert!(
            result.is_err(),
            "fs_write_script should fail when parent doesn't exist"
        );
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
        assert!(
            result.is_err(),
            "fs_delete_script should fail for nonexistent file"
        );
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
        assert!(
            result.is_err(),
            "fs_delete_script should reject non-.luau files"
        );
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
        assert!(
            result.is_err(),
            "fs_search_content should fail on invalid regex"
        );
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
        assert!(
            result.is_err(),
            "studio_get_selection should fail when bridge is stale"
        );
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioGetDataModelParams { max_depth: Some(3) };

        let result = server.studio_get_datamodel(Parameters(params)).await;
        assert!(
            result.is_err(),
            "studio_get_datamodel should fail when bridge is stale"
        );
    }

    #[tokio::test]
    async fn test_studio_get_script_source_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioGetScriptSourceParams {
            path: "game.ServerScriptService.Main".to_string(),
        };

        let result = server.studio_get_script_source(Parameters(params)).await;
        assert!(
            result.is_err(),
            "studio_get_script_source should fail when bridge is stale"
        );
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
        assert!(
            result.is_err(),
            "studio_modify_script should fail when bridge is stale"
        );
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
        assert!(
            result.is_err(),
            "studio_create_instance should fail when bridge is stale"
        );
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
        assert!(
            result.is_err(),
            "studio_set_property should fail when bridge is stale"
        );
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
        assert!(
            result.is_err(),
            "studio_delete_instance should fail when bridge is stale"
        );
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
        assert!(
            result.is_err(),
            "studio_find_instances should fail when bridge is stale"
        );
    }

    #[tokio::test]
    async fn test_server_get_info() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server(temp_dir.path().to_path_buf());

        let info = server.get_info();
        assert!(info.instructions.is_some());
        assert!(info
            .instructions
            .unwrap()
            .contains("Roblox Studio MCP Server"));
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
            [(
                "getSelection",
                json!({
                    "selected": [
                        {"Name": "Part1", "ClassName": "Part", "Path": "game.Workspace.Part1"},
                        {"Name": "Part2", "ClassName": "Part", "Path": "game.Workspace.Part2"}
                    ]
                }),
            )],
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
            [(
                "getDataModel",
                json!({
                    "Name": "DataModel",
                    "ClassName": "DataModel",
                    "Children": [
                        {"Name": "Workspace", "ClassName": "Workspace", "Children": []}
                    ]
                }),
            )],
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
            [(
                "getDataModelPaginated",
                json!({
                    "instances": [
                        {"Name": "Part1", "ClassName": "Part", "Path": "game.Workspace.Part1"},
                        {"Name": "Part2", "ClassName": "Part", "Path": "game.Workspace.Part2"}
                    ],
                    "cursor": "next_page_token",
                    "hasMore": true
                }),
            )],
        );

        let params = StudioGetDataModelPaginatedParams {
            start_path: Some("game.Workspace".to_string()),
            max_depth: Some(2),
            limit: Some(100),
            cursor: None,
        };
        let result = server
            .studio_get_datamodel_paginated(Parameters(params))
            .await;
        assert!(
            result.is_ok(),
            "studio_get_datamodel_paginated should succeed"
        );

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
            [(
                "getScriptSource",
                json!({
                    "source": "-- Main script\nprint('Hello World')",
                    "path": "game.ServerScriptService.Main"
                }),
            )],
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
            [(
                "modifyScript",
                json!({
                    "success": true,
                    "path": "game.ServerScriptService.Main",
                    "undoCreated": true
                }),
            )],
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
            [(
                "createInstance",
                json!({
                    "success": true,
                    "instance": {
                        "Name": "NewPart",
                        "ClassName": "Part",
                        "Path": "game.Workspace.NewPart"
                    }
                }),
            )],
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
            [(
                "setProperty",
                json!({
                    "success": true,
                    "path": "game.Workspace.Part",
                    "property": "Name",
                    "oldValue": "Part",
                    "newValue": "RenamedPart"
                }),
            )],
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
            [(
                "deleteInstance",
                json!({
                    "success": true,
                    "deletedPath": "game.Workspace.Part",
                    "undoCreated": true
                }),
            )],
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
            [(
                "findInstances",
                json!({
                    "instances": [
                        {"Name": "Part1", "ClassName": "Part", "Path": "game.Workspace.Part1"},
                        {"Name": "Part2", "ClassName": "Part", "Path": "game.Workspace.Part2"},
                        {"Name": "Part3", "ClassName": "Part", "Path": "game.Workspace.Folder.Part3"}
                    ],
                    "count": 3
                }),
            )],
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
        server
            .studio_set_property(Parameters(params))
            .await
            .unwrap();

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
        server
            .studio_get_datamodel(Parameters(StudioGetDataModelParams { max_depth: None }))
            .await
            .unwrap();

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
        assert!(
            result.is_ok(),
            "studio_health_check should succeed even when disconnected"
        );

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
        assert!(info
            .instructions
            .as_ref()
            .unwrap()
            .contains("Roblox Studio MCP Server"));
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
        let mock_linter =
            MockLinter::with_warnings(vec![("unused_variable", "x is never used", 1)]);

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
        assert!(
            result.is_ok(),
            "fs_lint_script_impl failed: {:?}",
            result.err()
        );

        // Verify custom linter was used (it should have recorded the call)
        assert!(mock_linter.call_count() > 0);
    }

    #[tokio::test]
    async fn test_studio_get_output_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getOutput",
                json!({
                    "logs": [
                        {"message": "Hello from Studio", "messageType": "MessageOutput", "timestamp": 1234567890},
                        {"message": "Warning message", "messageType": "MessageWarning", "timestamp": 1234567891}
                    ]
                }),
            )],
        );

        let params = StudioGetOutputParams { limit: Some(50) };
        let result = server.studio_get_output(Parameters(params)).await;
        assert!(result.is_ok(), "studio_get_output should succeed");

        if let RawContent::Text(text_content) = &*result.unwrap().content[0] {
            assert!(text_content.text.contains("Hello from Studio"));
            assert!(text_content.text.contains("Warning message"));
        } else {
            panic!("Expected text content");
        }

        assert!(mock.was_called("getOutput"));
    }

    #[tokio::test]
    async fn test_studio_get_output_default_limit() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("getOutput", json!({"logs": []}))],
        );

        let params = StudioGetOutputParams { limit: None };
        let result = server.studio_get_output(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "studio_get_output with default limit should succeed"
        );

        // Verify the call was made with default limit of 100
        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.action, "getOutput");
        assert_eq!(last_call.params["limit"], 100);
    }

    #[tokio::test]
    async fn test_studio_get_output_fails_on_stale_bridge() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server_with_stale_bridge(temp_dir.path().to_path_buf());

        let params = StudioGetOutputParams { limit: Some(10) };
        let result = server.studio_get_output(Parameters(params)).await;
        assert!(
            result.is_err(),
            "studio_get_output should fail when bridge is stale"
        );
    }

    // === CLOUD TOOL TESTS ===
    // Tests for cloud tool success and error paths using MockCloudClient

    use crate::cloud::mock::MockCloudClient;
    use crate::cloud::{AssetUploadResult, DataStoreEntry, PublishResult};

    fn create_server_with_mock_cloud(
        project_root: PathBuf,
        mock_cloud: Arc<MockCloudClient>,
    ) -> RobloxMcpServer<MockBridge, SeleneLinter> {
        let mock_bridge = Arc::new(MockBridge::new());
        RobloxMcpServer::with_mock_bridge(mock_bridge, project_root)
            .with_cloud_client(mock_cloud as Arc<dyn crate::cloud::CloudClient>)
    }

    #[tokio::test]
    async fn test_cloud_publish_place_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let rbxl_path = project_root.join("game.rbxl");
        std::fs::write(&rbxl_path, b"fake rbxl content").unwrap();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_publish_place(Ok(PublishResult { version_number: 42 }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudPublishPlaceParams {
            universe_id: 123456,
            place_id: 789012,
            rbxl_path: rbxl_path.display().to_string(),
        };

        let result = server.cloud_publish_place(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_publish_place should succeed: {:?}", result);

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(text_content.text.contains("42"), "Should contain version: {}", text_content.text);
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_datastore_get_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_datastore_get(Ok(DataStoreEntry {
            value: serde_json::json!({"coins": 100, "level": 5}),
            version: "abc123".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T00:00:00Z".to_string(),
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudDatastoreGetParams {
            universe_id: 123456,
            datastore_name: "PlayerData".to_string(),
            key: "user_123".to_string(),
            scope: None,
        };

        let result = server.cloud_datastore_get(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_datastore_get should succeed: {:?}", result);

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(text_content.text.contains("coins"), "Should contain data: {}", text_content.text);
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_datastore_set_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_datastore_set(Ok(DataStoreEntry {
            value: serde_json::json!({"coins": 200}),
            version: "v2".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T00:00:00Z".to_string(),
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudDatastoreSetParams {
            universe_id: 123456,
            datastore_name: "PlayerData".to_string(),
            key: "user_123".to_string(),
            value: serde_json::json!({"coins": 200}),
            scope: None,
        };

        let result = server.cloud_datastore_set(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_datastore_set should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_cloud_messaging_publish_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_messaging_publish(Ok(()));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudMessagingPublishParams {
            universe_id: 123456,
            topic: "game-events".to_string(),
            message: serde_json::json!({"event": "player_joined"}),
        };

        let result = server.cloud_messaging_publish(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_messaging_publish should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_cloud_upload_asset_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let asset_path = project_root.join("test_image.png");
        std::fs::write(&asset_path, b"fake png content").unwrap();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_upload_asset(Ok(AssetUploadResult {
            path: "operations/op123".to_string(),
            done: true,
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudUploadAssetParams {
            asset_type: "Decal".to_string(),
            file_path: asset_path.display().to_string(),
            name: "TestAsset".to_string(),
            description: "A test asset".to_string(),
            creator_id: 123456,
        };

        let result = server.cloud_upload_asset(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_upload_asset should succeed: {:?}", result);
    }

    // Tests for ordered datastore tools

    use crate::cloud::{OrderedDataStoreEntry, OrderedDataStoreList, UniverseInfo};

    #[tokio::test]
    async fn test_cloud_ordered_datastore_list_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_ordered_datastore_list(Ok(OrderedDataStoreList {
            entries: vec![
                OrderedDataStoreEntry {
                    path: "entry1".to_string(),
                    id: "player1".to_string(),
                    value: 1500,
                },
                OrderedDataStoreEntry {
                    path: "entry2".to_string(),
                    id: "player2".to_string(),
                    value: 1200,
                },
            ],
            next_page_token: None,
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudOrderedDatastoreListParams {
            universe_id: 123456,
            datastore_name: "Leaderboard".to_string(),
            scope: None,
            max_page_size: Some(10),
            page_token: None,
            order_by: None,
            filter: None,
        };

        let result = server.cloud_ordered_datastore_list(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_ordered_datastore_list should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_cloud_ordered_datastore_set_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_ordered_datastore_set(Ok(OrderedDataStoreEntry {
            path: "entry1".to_string(),
            id: "player1".to_string(),
            value: 2000,
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudOrderedDatastoreSetParams {
            universe_id: 123456,
            datastore_name: "Leaderboard".to_string(),
            scope: None,
            entry_id: "player1".to_string(),
            value: 2000,
        };

        let result = server.cloud_ordered_datastore_set(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_ordered_datastore_set should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_cloud_ordered_datastore_increment_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_ordered_datastore_increment(Ok(OrderedDataStoreEntry {
            path: "entry1".to_string(),
            id: "player1".to_string(),
            value: 1550,
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudOrderedDatastoreIncrementParams {
            universe_id: 123456,
            datastore_name: "Leaderboard".to_string(),
            scope: None,
            entry_id: "player1".to_string(),
            increment: 50,
        };

        let result = server.cloud_ordered_datastore_increment(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_ordered_datastore_increment should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_cloud_ordered_datastore_delete_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_ordered_datastore_delete(Ok(()));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudOrderedDatastoreDeleteParams {
            universe_id: 123456,
            datastore_name: "Leaderboard".to_string(),
            scope: None,
            entry_id: "player1".to_string(),
        };

        let result = server.cloud_ordered_datastore_delete(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_ordered_datastore_delete should succeed: {:?}", result);
    }

    // Tests for universe management tools

    #[tokio::test]
    async fn test_cloud_get_universe_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_get_universe(Ok(UniverseInfo {
            path: "universes/123456".to_string(),
            create_time: "2024-01-01T00:00:00Z".to_string(),
            update_time: "2024-06-15T12:00:00Z".to_string(),
            display_name: "Test Game".to_string(),
            description: "A test game".to_string(),
            user: Some("users/12345".to_string()),
            group: None,
            visibility: Some("PUBLIC".to_string()),
            voice_chat_enabled: false,
            age_rating: None,
            desktop_enabled: true,
            mobile_enabled: true,
            tablet_enabled: true,
            vr_enabled: false,
            console_enabled: false,
        }));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudGetUniverseParams {
            universe_id: 123456,
        };

        let result = server.cloud_get_universe(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_get_universe should succeed: {:?}", result);
    }

    #[tokio::test]
    async fn test_cloud_restart_servers_success_with_mock() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_restart_universe_servers(Ok(()));

        let server = create_server_with_mock_cloud(project_root, mock_cloud);

        let params = CloudRestartServersParams {
            universe_id: 123456,
        };

        let result = server.cloud_restart_servers(Parameters(params)).await;
        assert!(result.is_ok(), "cloud_restart_servers should succeed: {:?}", result);
    }

    // Tests for when cloud client is not configured

    #[tokio::test]
    async fn test_cloud_ordered_datastore_list_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudOrderedDatastoreListParams {
            universe_id: 123456,
            datastore_name: "Leaderboard".to_string(),
            scope: None,
            max_page_size: None,
            page_token: None,
            order_by: None,
            filter: None,
        };

        let result = server.cloud_ordered_datastore_list(Parameters(params)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cloud_get_universe_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudGetUniverseParams {
            universe_id: 123456,
        };

        let result = server.cloud_get_universe(Parameters(params)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cloud_restart_servers_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudRestartServersParams {
            universe_id: 123456,
        };

        let result = server.cloud_restart_servers(Parameters(params)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cloud_publish_place_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudPublishPlaceParams {
            universe_id: 123456,
            place_id: 789,
            rbxl_path: "/path/to/game.rbxl".to_string(),
        };

        let result = server.cloud_publish_place(Parameters(params)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Open Cloud not configured"));
    }

    #[tokio::test]
    async fn test_cloud_upload_asset_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudUploadAssetParams {
            asset_type: "image".to_string(),
            file_path: "/path/to/image.png".to_string(),
            name: "Test Image".to_string(),
            description: "A test image".to_string(),
            creator_id: 12345,
        };

        let result = server.cloud_upload_asset(Parameters(params)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Open Cloud not configured"));
    }

    #[tokio::test]
    async fn test_cloud_datastore_get_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudDatastoreGetParams {
            universe_id: 123456,
            datastore_name: "PlayerData".to_string(),
            key: "user_123".to_string(),
            scope: None,
        };

        let result = server.cloud_datastore_get(Parameters(params)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Open Cloud not configured"));
    }

    #[tokio::test]
    async fn test_cloud_datastore_set_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudDatastoreSetParams {
            universe_id: 123456,
            datastore_name: "PlayerData".to_string(),
            key: "user_123".to_string(),
            value: json!({"coins": 100, "level": 5}),
            scope: None,
        };

        let result = server.cloud_datastore_set(Parameters(params)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Open Cloud not configured"));
    }

    #[tokio::test]
    async fn test_cloud_messaging_publish_no_client() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = CloudMessagingPublishParams {
            universe_id: 123456,
            topic: "game-events".to_string(),
            message: json!({"event": "player_joined", "player_id": 789}),
        };

        let result = server.cloud_messaging_publish(Parameters(params)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Open Cloud not configured"));
    }

    // === FS_WATCH_CHANGES TESTS ===

    #[tokio::test]
    async fn test_fs_watch_changes_watcher_unavailable() {
        let temp_dir = TempDir::new().unwrap();
        // Mock server has file_watcher: None by default
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = FsWatchChangesParams { limit: Some(10) };
        let result = server.fs_watch_changes(Parameters(params)).await;

        assert!(
            result.is_err(),
            "fs_watch_changes should fail when watcher is unavailable"
        );
        let err = result.unwrap_err();
        assert!(err.message.contains("File watcher not available"));
    }

    #[tokio::test]
    async fn test_fs_watch_changes_watcher_unavailable_default_limit() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_mock_server(temp_dir.path().to_path_buf());

        let params = FsWatchChangesParams { limit: None };
        let result = server.fs_watch_changes(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("File watcher not available"));
    }

    #[tokio::test]
    async fn test_fs_watch_changes_success() {
        let temp_dir = TempDir::new().unwrap();
        // create_test_server creates a server with a real file watcher
        let server = create_test_server(temp_dir.path().to_path_buf());

        let params = FsWatchChangesParams { limit: Some(50) };
        let result = server.fs_watch_changes(Parameters(params)).await;

        // May succeed or fail depending on platform - check it doesn't panic
        if let Ok(call_result) = result {
            if let RawContent::Text(text) = &*call_result.content[0] {
                // Should contain expected fields
                assert!(text.text.contains("changes"));
                assert!(text.text.contains("returned_count"));
                assert!(text.text.contains("pending_count"));
            }
        }
        // If file watcher not available, it's fine - just return early
    }

    #[tokio::test]
    async fn test_fs_watch_changes_with_default_limit() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server(temp_dir.path().to_path_buf());

        let params = FsWatchChangesParams { limit: None };
        let result = server.fs_watch_changes(Parameters(params)).await;

        // May succeed or fail depending on platform
        if let Ok(call_result) = result {
            if let RawContent::Text(text) = &*call_result.content[0] {
                assert!(text.text.contains("changes"));
            }
        }
    }

    // === FS_LINT_SCRIPT EDGE CASE TESTS ===

    #[tokio::test]
    async fn test_fs_lint_script_non_luau_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let txt_file = project_root.join("script.txt");
        std::fs::write(&txt_file, "-- not a luau file").unwrap();

        let server = create_test_server(project_root);

        let params = FsLintScriptParams {
            file_path: txt_file.display().to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(
            result.is_err(),
            "fs_lint_script should reject non-.luau files"
        );
        let err = result.unwrap_err();
        assert!(err.message.contains("Only .luau files can be linted"));
    }

    #[tokio::test]
    async fn test_fs_lint_script_lua_extension_rejected() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let lua_file = project_root.join("script.lua");
        std::fs::write(&lua_file, "-- lua file not luau").unwrap();

        let server = create_test_server(project_root);

        let params = FsLintScriptParams {
            file_path: lua_file.display().to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(result.is_err(), "fs_lint_script should reject .lua files");
    }

    // === FS_WRITE_SCRIPT PATH TRAVERSAL TESTS ===

    #[tokio::test]
    async fn test_fs_write_script_path_traversal_with_create_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();
        let server = create_test_server(project_root.clone());

        // Create another temp dir that is definitely outside project root
        let other_dir = TempDir::new().unwrap();
        let outside_path = other_dir.path().join("nested/script.luau");

        let params = FsWriteScriptParams {
            file_path: outside_path.display().to_string(),
            content: "-- should not be created".to_string(),
            create_directories: Some(true),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(
            result.is_err(),
            "fs_write_script should detect path traversal with create_dirs"
        );
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Path traversal")
                || err.message.contains("outside")
                || err.message.contains("not exist"),
            "Error should mention path issue: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn test_fs_write_script_absolute_path_outside_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root);

        // Create another temp dir to simulate an absolute path outside project
        let other_dir = TempDir::new().unwrap();
        let outside_path = other_dir.path().join("script.luau");

        let params = FsWriteScriptParams {
            file_path: outside_path.display().to_string(),
            content: "-- should not be created".to_string(),
            create_directories: Some(true),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(
            result.is_err(),
            "fs_write_script should reject absolute paths outside project"
        );
    }

    #[tokio::test]
    async fn test_fs_write_script_relative_path_traversal_check() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root.clone());

        // Use a relative path that would escape project root if allowed
        // This tests line 346 (project_root.join(&path)) and line 349 (!abs_path.starts_with)
        let params = FsWriteScriptParams {
            file_path: "../escape.luau".to_string(),
            content: "-- should be rejected".to_string(),
            create_directories: Some(true),
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(
            result.is_err(),
            "fs_write_script with path traversal should fail"
        );

        let err = result.unwrap_err();
        assert!(
            err.message.contains("traversal") || err.message.contains("Path"),
            "Error should mention path issue: {}",
            err.message
        );
    }

    // === STUDIO_GET_DATAMODEL_PAGINATED EDGE CASE TESTS ===

    #[tokio::test]
    async fn test_studio_get_datamodel_paginated_all_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getDataModelPaginated",
                json!({
                    "instances": [],
                    "cursor": null,
                    "hasMore": false
                }),
            )],
        );

        // All params are None - should use defaults
        let params = StudioGetDataModelPaginatedParams {
            start_path: None,
            max_depth: None,
            limit: None,
            cursor: None,
        };
        let result = server
            .studio_get_datamodel_paginated(Parameters(params))
            .await;
        assert!(result.is_ok(), "Should succeed with all default params");

        // Verify defaults were applied
        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.action, "getDataModelPaginated");
        assert_eq!(last_call.params["startPath"], "game"); // default
        assert_eq!(last_call.params["maxDepth"], 3); // default
        assert_eq!(last_call.params["limit"], 500); // default
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_paginated_limit_capped_at_1000() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getDataModelPaginated",
                json!({
                    "instances": [],
                    "cursor": null,
                    "hasMore": false
                }),
            )],
        );

        // Request a limit higher than 1000
        let params = StudioGetDataModelPaginatedParams {
            start_path: Some("game.Workspace".to_string()),
            max_depth: Some(5),
            limit: Some(5000), // Should be capped to 1000
            cursor: None,
        };
        let result = server
            .studio_get_datamodel_paginated(Parameters(params))
            .await;
        assert!(result.is_ok(), "Should succeed with capped limit");

        // Verify limit was capped
        let last_call = mock.last_call().unwrap();
        assert_eq!(
            last_call.params["limit"], 1000,
            "Limit should be capped at 1000"
        );
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_paginated_with_cursor() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getDataModelPaginated",
                json!({
                    "instances": [{"Name": "Part", "ClassName": "Part"}],
                    "cursor": "next_cursor",
                    "hasMore": true
                }),
            )],
        );

        let params = StudioGetDataModelPaginatedParams {
            start_path: None,
            max_depth: None,
            limit: None,
            cursor: Some("previous_cursor".to_string()),
        };
        let result = server
            .studio_get_datamodel_paginated(Parameters(params))
            .await;
        assert!(result.is_ok());

        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.params["cursor"], "previous_cursor");
    }

    // === FS_SEARCH_CONTENT HIDDEN FILE TESTS ===

    #[tokio::test]
    async fn test_fs_search_content_reports_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Create a regular file and a hidden file
        std::fs::write(project_root.join("visible.luau"), "function test() end").unwrap();
        std::fs::write(project_root.join(".hidden.luau"), "function hidden() end").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsSearchContentParams {
            path: project_root.display().to_string(),
            pattern: "function".to_string(),
            extension: "luau".to_string(),
        };

        let result = server.fs_search_content(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            // Should find match in visible file
            assert!(
                text_content.text.contains("visible.luau"),
                "Should find visible file: {}",
                text_content.text
            );
            // Should report hidden file was skipped
            assert!(
                text_content.text.contains("skipped_hidden"),
                "Should report hidden files were skipped: {}",
                text_content.text
            );
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_fs_search_content_no_matches() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        std::fs::write(project_root.join("script.luau"), "local x = 1").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsSearchContentParams {
            path: project_root.display().to_string(),
            pattern: "nonexistent_pattern_xyz".to_string(),
            extension: "luau".to_string(),
        };

        let result = server.fs_search_content(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(
                text_content.text.contains("\"matches\":0"),
                "Should show zero matches: {}",
                text_content.text
            );
        } else {
            panic!("Expected text content");
        }
    }

    // === FS_GET_CHANGES HIDDEN FILE TESTS ===

    #[tokio::test]
    async fn test_fs_get_changes_reports_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Create a regular file and a hidden file
        std::fs::write(project_root.join("visible.luau"), "-- visible").unwrap();
        std::fs::write(project_root.join(".hidden.luau"), "-- hidden").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsGetChangesParams {
            path: project_root.display().to_string(),
        };

        let result = server.fs_get_changes(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            // Should include the visible file in the files map
            assert!(
                text_content.text.contains("visible.luau"),
                "Should track visible files: {}",
                text_content.text
            );
            // Should report hidden file was skipped
            assert!(
                text_content.text.contains("skipped_hidden")
                    || text_content.text.contains(".hidden"),
                "Should report hidden files: {}",
                text_content.text
            );
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_fs_get_changes_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // No .luau files in directory
        std::fs::write(project_root.join("readme.txt"), "not a luau file").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsGetChangesParams {
            path: project_root.display().to_string(),
        };

        let result = server.fs_get_changes(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(
                text_content.text.contains("\"file_count\":0"),
                "Should show zero file count: {}",
                text_content.text
            );
        } else {
            panic!("Expected text content");
        }
    }

    // === ADDITIONAL EDGE CASE TESTS ===

    #[tokio::test]
    async fn test_fs_get_tree_with_nested_directories() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Create nested structure
        std::fs::create_dir_all(project_root.join("src/game/modules")).unwrap();
        std::fs::write(project_root.join("src/main.luau"), "-- main").unwrap();
        std::fs::write(project_root.join("src/game/init.luau"), "-- game").unwrap();
        std::fs::write(project_root.join("src/game/modules/utils.luau"), "-- utils").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(10),
        };

        let result = server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let RawContent::Text(text_content) = &*call_result.content[0] {
            assert!(text_content.text.contains("src"));
            assert!(text_content.text.contains("tree"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_fs_get_tree_default_max_depth() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        std::fs::write(project_root.join("test.luau"), "-- test").unwrap();

        let server = create_test_server(project_root.clone());

        // max_depth is None, should use default of 5
        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: None,
        };

        let result = server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_studio_modify_script_default_record_undo() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("modifyScript", json!({"success": true}))],
        );

        let params = StudioModifyScriptParams {
            path: "game.ServerScriptService.Main".to_string(),
            new_source: "-- updated".to_string(),
            record_undo: None, // Should default to true
        };
        server
            .studio_modify_script(Parameters(params))
            .await
            .unwrap();

        let last_call = mock.last_call().unwrap();
        assert_eq!(
            last_call.params["recordUndo"], true,
            "record_undo should default to true"
        );
    }

    #[tokio::test]
    async fn test_studio_create_instance_default_record_undo() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "createInstance",
                json!({"success": true, "instance": {"Name": "Part"}}),
            )],
        );

        let params = StudioCreateInstanceParams {
            class_name: "Part".to_string(),
            parent: "game.Workspace".to_string(),
            name: "TestPart".to_string(),
            properties: None,
            record_undo: None, // Should default to true
        };
        server
            .studio_create_instance(Parameters(params))
            .await
            .unwrap();

        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.params["recordUndo"], true);
    }

    #[tokio::test]
    async fn test_studio_set_property_default_record_undo() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("setProperty", json!({"success": true}))],
        );

        let params = StudioSetPropertyParams {
            path: "game.Workspace.Part".to_string(),
            property: "Name".to_string(),
            value: json!("NewName"),
            record_undo: None, // Should default to true
        };
        server
            .studio_set_property(Parameters(params))
            .await
            .unwrap();

        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.params["recordUndo"], true);
    }

    #[tokio::test]
    async fn test_studio_delete_instance_default_record_undo() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("deleteInstance", json!({"success": true}))],
        );

        let params = StudioDeleteInstanceParams {
            path: "game.Workspace.Part".to_string(),
            record_undo: None, // Should default to true
        };
        server
            .studio_delete_instance(Parameters(params))
            .await
            .unwrap();

        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.params["recordUndo"], true);
    }

    #[tokio::test]
    async fn test_studio_find_instances_default_root() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("findInstances", json!({"instances": [], "count": 0}))],
        );

        let params = StudioFindInstancesParams {
            class_name: "Part".to_string(),
            root: None, // No root specified
        };
        server
            .studio_find_instances(Parameters(params))
            .await
            .unwrap();

        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.params["className"], "Part");
        // root should be null/None when not specified
        assert!(last_call.params["root"].is_null());
    }

    #[tokio::test]
    async fn test_studio_get_datamodel_default_max_depth() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [("getDataModel", json!({"Name": "DataModel", "Children": []}))],
        );

        let params = StudioGetDataModelParams {
            max_depth: None, // Should default to 3
        };
        server
            .studio_get_datamodel(Parameters(params))
            .await
            .unwrap();

        let last_call = mock.last_call().unwrap();
        assert_eq!(last_call.params["maxDepth"], 3);
    }

    // === CLOUD TOOL SUCCESS PATH TESTS ===
    // These tests verify cloud tool success paths using MockCloudClient

    #[tokio::test]
    async fn test_cloud_datastore_get_success() {
        use crate::cloud::mock::MockCloudClient;
        use crate::cloud::DataStoreEntry;

        let temp_dir = TempDir::new().unwrap();
        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_datastore_get(Ok(DataStoreEntry {
            value: serde_json::json!({"coins": 100, "level": 5}),
            version: "v1".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T00:00:00Z".to_string(),
        }));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        let params = CloudDatastoreGetParams {
            universe_id: 123,
            datastore_name: "PlayerData".to_string(),
            key: "user_123".to_string(),
            scope: None,
        };

        let result = server.cloud_datastore_get(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "cloud_datastore_get should succeed: {:?}",
            result
        );

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("coins"));
            assert!(text.text.contains("100"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_datastore_set_success() {
        use crate::cloud::mock::MockCloudClient;
        use crate::cloud::DataStoreEntry;

        let temp_dir = TempDir::new().unwrap();
        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_datastore_set(Ok(DataStoreEntry {
            value: serde_json::json!({"coins": 500}),
            version: "v2".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T00:00:00Z".to_string(),
        }));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        let params = CloudDatastoreSetParams {
            universe_id: 123,
            datastore_name: "PlayerData".to_string(),
            key: "user_456".to_string(),
            value: serde_json::json!({"coins": 500}),
            scope: None,
        };

        let result = server.cloud_datastore_set(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "cloud_datastore_set should succeed: {:?}",
            result
        );

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("success"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_messaging_publish_success() {
        use crate::cloud::mock::MockCloudClient;

        let temp_dir = TempDir::new().unwrap();
        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_messaging_publish(Ok(()));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        let params = CloudMessagingPublishParams {
            universe_id: 123,
            topic: "game-events".to_string(),
            message: serde_json::json!({"event": "player_joined", "player_id": 789}),
        };

        let result = server.cloud_messaging_publish(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "cloud_messaging_publish should succeed: {:?}",
            result
        );

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("success"));
            assert!(text.text.contains("game-events"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_messaging_publish_long_message() {
        use crate::cloud::mock::MockCloudClient;

        let temp_dir = TempDir::new().unwrap();
        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_messaging_publish(Ok(()));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        // Create a message longer than 100 characters to trigger truncation
        let long_message = "a".repeat(150);
        let params = CloudMessagingPublishParams {
            universe_id: 123,
            topic: "test-topic".to_string(),
            message: serde_json::json!(long_message),
        };

        let result = server.cloud_messaging_publish(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            // Should contain truncated message preview ending with "..."
            assert!(text.text.contains("message_preview"));
            assert!(text.text.contains("..."));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_publish_place_success() {
        use crate::cloud::mock::MockCloudClient;
        use crate::cloud::PublishResult;

        let temp_dir = TempDir::new().unwrap();
        let rbxl_path = temp_dir.path().join("game.rbxl");
        std::fs::write(&rbxl_path, b"fake rbxl content").unwrap();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_publish_place(Ok(PublishResult { version_number: 42 }));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        let params = CloudPublishPlaceParams {
            universe_id: 123,
            place_id: 456,
            rbxl_path: rbxl_path.display().to_string(),
        };

        let result = server.cloud_publish_place(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "cloud_publish_place should succeed: {:?}",
            result
        );

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("42")); // version number
            assert!(text.text.contains("success"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_upload_asset_success() {
        use crate::cloud::mock::MockCloudClient;
        use crate::cloud::AssetUploadResult;

        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("icon.png");
        std::fs::write(&image_path, b"fake png content").unwrap();

        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_upload_asset(Ok(AssetUploadResult {
            path: "assets/v1/operations/12345".to_string(),
            done: true,
        }));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        let params = CloudUploadAssetParams {
            asset_type: "image".to_string(),
            file_path: image_path.display().to_string(),
            name: "Test Icon".to_string(),
            description: "A test icon".to_string(),
            creator_id: 999,
        };

        let result = server.cloud_upload_asset(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "cloud_upload_asset should succeed: {:?}",
            result
        );

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("12345") || text.text.contains("operations"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_cloud_datastore_get_with_scope() {
        use crate::cloud::mock::MockCloudClient;
        use crate::cloud::DataStoreEntry;

        let temp_dir = TempDir::new().unwrap();
        let mock_cloud = Arc::new(MockCloudClient::new());
        mock_cloud.queue_datastore_get(Ok(DataStoreEntry {
            value: serde_json::json!({"settings": {"volume": 75}}),
            version: "v3".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-03T00:00:00Z".to_string(),
        }));

        let server =
            create_test_server(temp_dir.path().to_path_buf()).with_cloud_client(mock_cloud);

        let params = CloudDatastoreGetParams {
            universe_id: 123,
            datastore_name: "UserSettings".to_string(),
            key: "settings_789".to_string(),
            scope: Some("custom_scope".to_string()),
        };

        let result = server.cloud_datastore_get(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cloud_no_client_configured() {
        // Test that cloud tools return appropriate error when no client is configured
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server(temp_dir.path().to_path_buf());
        // Note: create_test_server uses OpenCloudClient::new() which will fail
        // if ROBLOX_OPEN_CLOUD_API_KEY is not set, resulting in cloud_client = None

        let params = CloudDatastoreGetParams {
            universe_id: 123,
            datastore_name: "Test".to_string(),
            key: "key".to_string(),
            scope: None,
        };

        let result = server.cloud_datastore_get(Parameters(params)).await;
        // Should fail because cloud client is not configured
        assert!(
            result.is_err(),
            "Should fail when cloud client not configured"
        );
    }

    // === ADDITIONAL FS_LINT_SCRIPT TESTS ===

    #[tokio::test]
    async fn test_fs_lint_script_success_clean_file() {
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("clean.luau");
        std::fs::write(&script_path, "-- Clean script\nreturn {}").unwrap();

        let mock_linter = MockLinter::clean();
        let server = RobloxMcpServer::with_mock_bridge_and_linter(
            Arc::new(MockBridge::new()),
            project_root,
            mock_linter.clone(),
        );

        let params = FsLintScriptParams {
            file_path: script_path.display().to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(result.is_ok(), "Clean file should lint successfully");

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());

        // Verify linter was called
        assert_eq!(mock_linter.call_count(), 1);
    }

    #[tokio::test]
    async fn test_fs_lint_script_with_errors() {
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("error.luau");
        std::fs::write(&script_path, "local x = y").unwrap();

        let mock_linter =
            MockLinter::with_errors(vec![("undefined_variable", "y is not defined", 1)]);
        let server = RobloxMcpServer::with_mock_bridge_and_linter(
            Arc::new(MockBridge::new()),
            project_root,
            mock_linter,
        );

        let params = FsLintScriptParams {
            file_path: script_path.display().to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(
            result.is_ok(),
            "Lint should succeed even with errors in file"
        );

        // Verify the result contains error diagnostics
        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("error"));
            assert!(text.text.contains("undefined_variable"));
        }
    }

    #[tokio::test]
    async fn test_fs_lint_script_with_config_path() {
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("script.luau");
        std::fs::write(&script_path, "local x = 1").unwrap();

        let mock_linter = MockLinter::clean();
        let server = RobloxMcpServer::with_mock_bridge_and_linter(
            Arc::new(MockBridge::new()),
            project_root.clone(),
            mock_linter.clone(),
        );

        let config_path = project_root.join("selene.toml");

        let params = FsLintScriptParams {
            file_path: script_path.display().to_string(),
            config_path: Some(config_path.display().to_string()),
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(result.is_ok());

        // Verify config path was passed to linter
        let calls = mock_linter.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].config_path.is_some());
    }

    #[tokio::test]
    async fn test_fs_lint_script_linter_error() {
        use crate::error::RobloxMcpError;
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("script.luau");
        std::fs::write(&script_path, "local x = 1").unwrap();

        let mock_linter = MockLinter::new();
        mock_linter.queue_error(RobloxMcpError::ConfigError(
            "Selene not installed".to_string(),
        ));

        let server = RobloxMcpServer::with_mock_bridge_and_linter(
            Arc::new(MockBridge::new()),
            project_root,
            mock_linter,
        );

        let params = FsLintScriptParams {
            file_path: script_path.display().to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(result.is_err(), "Linter error should propagate");

        let err = result.unwrap_err();
        assert!(err.message.contains("Selene"));
    }

    #[tokio::test]
    async fn test_fs_lint_script_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let server = create_test_server(project_root);

        let params = FsLintScriptParams {
            file_path: "../../../etc/passwd.luau".to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        assert!(result.is_err(), "Path traversal should be rejected");
    }

    #[tokio::test]
    async fn test_fs_lint_script_nonexistent_file() {
        use crate::error::RobloxMcpError;
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Don't create the file - it should fail during path validation
        let mock_linter = MockLinter::new();
        // Queue an error in case we get to the linter (we shouldn't)
        mock_linter.queue_error(RobloxMcpError::ConfigError("File not found".to_string()));

        let server = RobloxMcpServer::with_mock_bridge_and_linter(
            Arc::new(MockBridge::new()),
            project_root.clone(),
            mock_linter,
        );

        let params = FsLintScriptParams {
            file_path: project_root.join("nonexistent.luau").display().to_string(),
            config_path: None,
        };

        let result = server.fs_lint_script(Parameters(params)).await;
        // Should fail because file doesn't exist (path validation succeeds for .luau but file is missing)
        // Note: The actual behavior depends on whether validate_path checks file existence
        // or if the linter checks it. Either way, it should error.
        assert!(result.is_err(), "Nonexistent file should fail");
    }

    // === ADDITIONAL EDGE CASE TESTS ===

    #[tokio::test]
    async fn test_fs_get_tree_with_max_depth_zero() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        std::fs::create_dir(project_root.join("subdir")).unwrap();
        std::fs::write(project_root.join("subdir/file.luau"), "-- file").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(0),
        };

        let result = server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok(), "max_depth 0 should work");
    }

    #[tokio::test]
    async fn test_fs_write_script_overwrite_existing() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let script_path = project_root.join("existing.luau");

        // Create initial file
        std::fs::write(&script_path, "-- original content").unwrap();

        let server = create_test_server(project_root);

        // Overwrite with new content
        let params = FsWriteScriptParams {
            file_path: script_path.display().to_string(),
            content: "-- new content".to_string(),
            create_directories: None,
        };

        let result = server.fs_write_script(Parameters(params)).await;
        assert!(result.is_ok(), "Should overwrite existing file");

        let content = std::fs::read_to_string(&script_path).unwrap();
        assert_eq!(content, "-- new content");
    }

    #[tokio::test]
    async fn test_fs_search_content_zero_matches_returned() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        std::fs::write(project_root.join("script.luau"), "-- nothing here").unwrap();

        let server = create_test_server(project_root.clone());

        let params = FsSearchContentParams {
            path: project_root.display().to_string(),
            pattern: "NONEXISTENT_PATTERN_12345".to_string(),
            extension: "luau".to_string(),
        };

        let result = server.fs_search_content(Parameters(params)).await;
        assert!(result.is_ok(), "Search with no matches should succeed");

        let call_result = result.unwrap();
        if let RawContent::Text(text) = &*call_result.content[0] {
            // Should have 0 matches
            assert!(text.text.contains("\"matches\":0") || text.text.contains("\"matches\": 0"));
        }
    }

    #[tokio::test]
    async fn test_server_get_metrics_returns_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let server = create_test_server(temp_dir.path().to_path_buf());

        let result = server.server_get_metrics().await;
        assert!(result.is_ok(), "server_get_metrics should succeed");

        let call_result = result.unwrap();
        assert!(!call_result.content.is_empty());

        // Verify response is valid JSON with expected fields
        if let RawContent::Text(text) = &*call_result.content[0] {
            let metrics: serde_json::Value =
                serde_json::from_str(&text.text).expect("Metrics should be valid JSON");
            assert!(metrics.get("tools").is_some(), "Should have tools field");
            assert!(
                metrics.get("connection").is_some(),
                "Should have connection field"
            );
        }
    }

    // === UTILITY METHOD TESTS ===

    #[tokio::test]
    async fn test_with_shared_metrics() {
        use crate::metrics::ServerMetrics;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Create shared metrics with pre-recorded data
        let shared_metrics = Arc::new(ServerMetrics::new());
        shared_metrics.record_connection_status(true);
        shared_metrics.record_connection_status(true);

        // Create server with shared metrics
        let _server = create_test_server(project_root).with_shared_metrics(shared_metrics.clone());

        // The server should use the shared metrics
        // Record some more data through the server's metrics
        shared_metrics.record_late_result(false);

        // Verify the metrics are shared by checking the snapshot
        let snapshot = shared_metrics.snapshot().await;
        assert_eq!(snapshot.connection.total_checks, 2);
        assert_eq!(snapshot.late_results.total, 1);
    }

    #[tokio::test]
    async fn test_with_mocks_constructor() {
        use crate::bridge::mock::MockBridge;
        use crate::cloud::mock::MockCloudClient;
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_bridge = Arc::new(MockBridge::new());
        let mock_cloud = Arc::new(MockCloudClient::new());
        let mock_linter = MockLinter::new();

        // Use with_mocks constructor
        let server: RobloxMcpServer<MockBridge, MockLinter> = RobloxMcpServer::with_mocks(
            mock_bridge,
            project_root.clone(),
            Some(mock_cloud as Arc<dyn CloudClient>),
            mock_linter,
        );

        // Verify the server was created by calling a simple method
        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(1),
        };

        let result: Result<CallToolResult, ErrorData> =
            server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_with_mocks_no_cloud_client() {
        use crate::bridge::mock::MockBridge;
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_bridge = Arc::new(MockBridge::new());
        let mock_linter = MockLinter::new();

        // Use with_mocks constructor without cloud client
        let server: RobloxMcpServer<MockBridge, MockLinter> = RobloxMcpServer::with_mocks(
            mock_bridge,
            project_root.clone(),
            None, // No cloud client
            mock_linter,
        );

        // Verify the server was created
        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(1),
        };

        let result: Result<CallToolResult, ErrorData> =
            server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_with_cloud_client() {
        use crate::bridge::mock::MockBridge;
        use crate::cloud::mock::MockCloudClient;
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_bridge = Arc::new(MockBridge::new());
        let mock_linter = MockLinter::new();
        let mock_cloud = Arc::new(MockCloudClient::new());

        // Create server without cloud client first
        let server: RobloxMcpServer<MockBridge, MockLinter> =
            RobloxMcpServer::with_mocks(mock_bridge, project_root.clone(), None, mock_linter);

        // Then add cloud client via with_cloud_client
        let server = server.with_cloud_client(mock_cloud);

        // The server should now have a cloud client
        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(1),
        };

        let result: Result<CallToolResult, ErrorData> =
            server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_instrumentation() {
        use crate::bridge::mock::MockBridge;
        use crate::tools::linting::mock::MockLinter;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        let mock_bridge = Arc::new(MockBridge::new());
        let mock_linter = MockLinter::new();

        let server: RobloxMcpServer<MockBridge, MockLinter> =
            RobloxMcpServer::with_mocks(mock_bridge, project_root.clone(), None, mock_linter);

        // start_instrumentation is exercised through any tool call
        // We verify that metrics are properly recorded by calling a tool
        // and checking that no panic occurs
        let params = FsGetTreeParams {
            path: project_root.display().to_string(),
            max_depth: Some(1),
        };

        let result: Result<CallToolResult, ErrorData> =
            server.fs_get_tree(Parameters(params)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_studio_get_properties_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getProperties",
                json!({
                    "Name": "MyPart",
                    "Position": [0, 10, 0],
                    "Size": [4, 1, 2],
                    "ClassName": "Part"
                }),
            )],
        );

        let params = StudioGetPropertiesParams {
            path: "game.Workspace.MyPart".to_string(),
            properties: Some(vec![
                "Name".to_string(),
                "Position".to_string(),
                "Size".to_string(),
            ]),
        };

        let result = server.studio_get_properties(Parameters(params)).await;
        assert!(result.is_ok());

        assert!(mock.was_called("getProperties"));
        let call = mock.last_call().unwrap();
        assert_eq!(call.params["path"], "game.Workspace.MyPart");
        assert!(call.params["properties"].is_array());
    }

    #[tokio::test]
    async fn test_studio_get_properties_no_properties_specified() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getProperties",
                json!({
                    "Name": "Part",
                    "ClassName": "Part"
                }),
            )],
        );

        let params = StudioGetPropertiesParams {
            path: "game.Workspace.Part".to_string(),
            properties: None,
        };

        let result = server.studio_get_properties(Parameters(params)).await;
        assert!(result.is_ok());

        let call = mock.last_call().unwrap();
        assert!(call.params["properties"].is_null());
    }

    #[tokio::test]
    async fn test_studio_get_bounds_success() {
        let temp_dir = TempDir::new().unwrap();
        let (server, mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getBounds",
                json!({
                    "center": [0, 5, 0],
                    "size": [10, 10, 10],
                    "min": [-5, 0, -5],
                    "max": [5, 10, 5],
                    "orientation": [0, 0, 0]
                }),
            )],
        );

        let params = StudioGetBoundsParams {
            path: "game.Workspace.Model".to_string(),
        };

        let result = server.studio_get_bounds(Parameters(params)).await;
        assert!(result.is_ok());

        assert!(mock.was_called("getBounds"));
        let call = mock.last_call().unwrap();
        assert_eq!(call.params["path"], "game.Workspace.Model");
    }

    #[tokio::test]
    async fn test_studio_get_bounds_with_model() {
        let temp_dir = TempDir::new().unwrap();
        let (server, _mock) = create_mock_server_with_responses(
            temp_dir.path().to_path_buf(),
            [(
                "getBounds",
                json!({
                    "center": [100, 50, 200],
                    "size": [50, 100, 50],
                    "min": [75, 0, 175],
                    "max": [125, 100, 225],
                    "orientation": [0, 45, 0]
                }),
            )],
        );

        let params = StudioGetBoundsParams {
            path: "game.Workspace.Building.Roof".to_string(),
        };

        let result = server.studio_get_bounds(Parameters(params)).await;
        assert!(result.is_ok());

        let call_result = result.unwrap();
        if let rmcp::model::RawContent::Text(text) = &*call_result.content[0] {
            assert!(text.text.contains("center"));
            assert!(text.text.contains("size"));
            assert!(text.text.contains("min"));
            assert!(text.text.contains("max"));
        } else {
            panic!("Expected text content");
        }
    }
}
