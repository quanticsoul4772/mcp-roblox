use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

// === FILESYSTEM PARAMS ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsGetTreeParams {
    #[schemars(description = "Root path to explore")]
    pub path: String,
    #[schemars(description = "Maximum depth (default: 5)")]
    pub max_depth: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsReadScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsWriteScriptParams {
    #[schemars(description = "Path to .luau file")]
    pub file_path: String,
    #[schemars(description = "Script content")]
    pub content: String,
    #[schemars(description = "Create parent directories if missing")]
    pub create_directories: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsDeleteScriptParams {
    #[schemars(description = "Path to .luau file to delete")]
    pub file_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsSearchContentParams {
    #[schemars(description = "Directory to search")]
    pub path: String,
    #[schemars(description = "Regex pattern to match")]
    pub pattern: String,
    #[schemars(description = "File extension filter (e.g., 'luau')")]
    pub extension: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FsGetChangesParams {
    #[schemars(description = "Root path to scan for file modification times")]
    pub path: String,
}
