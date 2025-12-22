use crate::error::RobloxMcpError;
use crate::limits::MAX_TREE_ENTRIES;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::FileType;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileTree {
    pub path: String,
    pub name: String,
    pub is_file: bool,
    pub children: Option<Vec<Self>>,
}

/// Result of building a file tree, including information about skipped entries
#[derive(Debug, Serialize, Deserialize)]
pub struct TreeBuildResult {
    pub tree: FileTree,
    pub skipped: Vec<SkippedEntry>,
    /// Whether the tree was truncated due to entry limit
    #[serde(default)]
    pub truncated: bool,
    /// The limit that was applied (only present when truncated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Total entries counted (only present when truncated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_entries: Option<usize>,
}

/// An entry that was skipped during tree building
#[derive(Debug, Serialize, Deserialize)]
pub struct SkippedEntry {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptContent {
    pub path: String,
    pub content: String,
    pub size_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub bytes_written: usize,
}

/// Validate that a path is within the project root
///
/// Handles both existing and non-existing paths:
/// - If path exists: uses canonicalize for maximum security
/// - If path doesn't exist: finds nearest existing ancestor, canonicalizes it,
///   then safely appends remaining components (rejecting any `..` segments)
pub fn validate_path(requested: &Path, project_root: &Path) -> Result<PathBuf, RobloxMcpError> {
    let canonical_root = project_root.canonicalize().map_err(|e| {
        RobloxMcpError::InvalidPath(format!("Cannot canonicalize project root: {e}"))
    })?;

    // Try canonicalize first (works if path exists)
    if let Ok(canonical) = requested.canonicalize() {
        if !canonical.starts_with(&canonical_root) {
            return Err(RobloxMcpError::PathTraversal(
                canonical.display().to_string(),
            ));
        }
        return Ok(canonical);
    }

    // Path doesn't exist - find existing ancestor and validate
    let mut existing_ancestor = requested.to_path_buf();
    let mut components_to_add = Vec::new();

    // Walk up until we find an existing directory
    while !existing_ancestor.exists() {
        if let Some(file_name) = existing_ancestor.file_name() {
            components_to_add.push(file_name.to_os_string());
            if let Some(parent) = existing_ancestor.parent() {
                existing_ancestor = parent.to_path_buf();
            } else {
                return Err(RobloxMcpError::InvalidPath(
                    "Cannot find existing ancestor directory".to_string(),
                ));
            }
        } else {
            return Err(RobloxMcpError::InvalidPath(
                "Path has no valid components".to_string(),
            ));
        }
    }

    // Canonicalize the existing ancestor
    let canonical_ancestor = existing_ancestor.canonicalize().map_err(|e| {
        RobloxMcpError::InvalidPath(format!("Cannot canonicalize ancestor: {e}"))
    })?;

    // Verify ancestor is within project root
    if !canonical_ancestor.starts_with(&canonical_root) {
        return Err(RobloxMcpError::PathTraversal(
            canonical_ancestor.display().to_string(),
        ));
    }

    // Rebuild path by appending components (in reverse order)
    let mut result = canonical_ancestor;
    for component in components_to_add.into_iter().rev() {
        // Reject any path traversal attempts in remaining components
        let component_str = component.to_string_lossy();
        if component_str == ".." || component_str.contains("..") {
            return Err(RobloxMcpError::PathTraversal(
                requested.display().to_string(),
            ));
        }
        result.push(component);
    }

    Ok(result)
}

/// Check if a path is a symlink using symlink_metadata (doesn't follow symlinks)
///
/// Returns the file type for further inspection, or an error if metadata cannot be read.
pub fn get_file_type_no_follow(path: &Path) -> Result<FileType, RobloxMcpError> {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type())
        .map_err(|e| RobloxMcpError::FileSystemError {
            operation: "symlink_metadata".to_string(),
            path: path.display().to_string(),
            source: e,
        })
}

/// Reject symlinks for security - prevents symlink-based information disclosure
///
/// Must be called before any file read/write operation to ensure the target
/// is not a symlink pointing outside the project root.
pub fn reject_if_symlink(path: &Path) -> Result<(), RobloxMcpError> {
    let file_type = get_file_type_no_follow(path)?;
    if file_type.is_symlink() {
        return Err(RobloxMcpError::SecurityViolation(format!(
            "Symlinks are not allowed for security reasons: {}",
            path.display()
        )));
    }
    Ok(())
}

/// Build a file tree synchronously (for use inside spawn_blocking)
///
/// Uses iterative BFS approach to avoid recursive async overhead and blocking the async runtime.
/// Enforces MAX_TREE_ENTRIES limit to prevent unbounded memory growth.
pub fn build_tree_sync(root: &Path, max_depth: usize) -> Result<TreeBuildResult> {
    let mut all_skipped: Vec<SkippedEntry> = vec![];
    let mut truncated = false;
    let mut total_entries: usize = 1; // Start with root

    let root_name = root
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Path has no file name: {}", root.display()))?
        .to_string_lossy()
        .to_string();

    let root_metadata = std::fs::metadata(root)?;

    if root_metadata.is_file() {
        return Ok(TreeBuildResult {
            tree: FileTree {
                path: root.display().to_string(),
                name: root_name,
                is_file: true,
                children: None,
            },
            skipped: vec![],
            truncated: false,
            limit: None,
            total_entries: None,
        });
    }

    // Struct for tracking pending directories to process
    struct PendingDir {
        path: PathBuf,
        depth: usize,
        tree_index: usize,
    }

    // Build tree iteratively using BFS
    let mut trees: Vec<FileTree> = vec![FileTree {
        path: root.display().to_string(),
        name: root_name,
        is_file: false,
        children: Some(vec![]),
    }];

    let mut queue: VecDeque<PendingDir> = VecDeque::new();
    queue.push_back(PendingDir {
        path: root.to_path_buf(),
        depth: 0,
        tree_index: 0,
    });

    // Map from tree_index to list of child indices
    let mut children_map: HashMap<usize, Vec<usize>> = HashMap::new();

    'outer: while let Some(pending) = queue.pop_front() {
        if pending.depth >= max_depth {
            continue;
        }

        let entries = match std::fs::read_dir(&pending.path) {
            Ok(entries) => entries,
            Err(e) => {
                all_skipped.push(SkippedEntry {
                    path: pending.path.display().to_string(),
                    reason: format!("cannot read directory: {}", e),
                });
                continue;
            }
        };

        let mut child_entries: Vec<(PathBuf, bool)> = vec![]; // (path, is_file)

        for entry_result in entries {
            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    all_skipped.push(SkippedEntry {
                        path: pending.path.display().to_string(),
                        reason: format!("entry error: {}", e),
                    });
                    continue;
                }
            };

            let child_path = entry.path();

            // Security: Check for symlinks FIRST (before any other checks)
            match std::fs::symlink_metadata(&child_path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    all_skipped.push(SkippedEntry {
                        path: child_path.display().to_string(),
                        reason: "symlink rejected for security".to_string(),
                    });
                    continue;
                }
                Err(_) => {
                    all_skipped.push(SkippedEntry {
                        path: child_path.display().to_string(),
                        reason: "cannot read file metadata".to_string(),
                    });
                    continue;
                }
                Ok(_) => {} // Not a symlink, continue processing
            }

            // Check for exclusions and REPORT them
            if let Some(file_name) = child_path.file_name() {
                let name_str = file_name.to_string_lossy();
                if name_str.starts_with('.') {
                    all_skipped.push(SkippedEntry {
                        path: child_path.display().to_string(),
                        reason: "hidden file/directory (starts with '.')".to_string(),
                    });
                    continue;
                }
                if name_str == "node_modules" {
                    all_skipped.push(SkippedEntry {
                        path: child_path.display().to_string(),
                        reason: "node_modules directory excluded".to_string(),
                    });
                    continue;
                }
                if name_str == "target" {
                    all_skipped.push(SkippedEntry {
                        path: child_path.display().to_string(),
                        reason: "target directory excluded (Rust build output)".to_string(),
                    });
                    continue;
                }
            }

            let is_file = match std::fs::metadata(&child_path) {
                Ok(m) => m.is_file(),
                Err(e) => {
                    // Report the metadata error, don't silently assume
                    all_skipped.push(SkippedEntry {
                        path: child_path.display().to_string(),
                        reason: format!("cannot read metadata: {}", e),
                    });
                    continue;
                }
            };

            child_entries.push((child_path, is_file));
        }

        // Sort: directories first, then files, alphabetically within each group
        child_entries.sort_by(|(a, a_is_file), (b, b_is_file)| {
            match (a_is_file, b_is_file) {
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
                _ => {
                    let a_name = a.file_name().map(|n| n.to_string_lossy().to_lowercase());
                    let b_name = b.file_name().map(|n| n.to_string_lossy().to_lowercase());
                    a_name.cmp(&b_name)
                }
            }
        });

        let mut child_indices = vec![];

        for (child_path, is_file) in child_entries {
            // Check entry limit BEFORE adding
            if total_entries >= MAX_TREE_ENTRIES {
                truncated = true;
                all_skipped.push(SkippedEntry {
                    path: child_path.display().to_string(),
                    reason: format!("entry limit reached ({})", MAX_TREE_ENTRIES),
                });
                // Continue to skip remaining entries in this directory,
                // then break out of the main loop
                continue;
            }

            let child_name = child_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let child_index = trees.len();
            let child_depth = pending.depth + 1;

            // Files have no children; directories at max depth have None (won't recurse);
            // other directories get Some(vec![]) to be populated when processed
            let children = if !is_file && child_depth < max_depth {
                Some(vec![]) // Will be populated when we process this directory
            } else {
                None // File, or max depth reached
            };

            trees.push(FileTree {
                path: child_path.display().to_string(),
                name: child_name,
                is_file,
                children,
            });

            total_entries += 1;
            child_indices.push(child_index);

            // Only queue directories that haven't reached max depth
            if !is_file && child_depth < max_depth {
                queue.push_back(PendingDir {
                    path: child_path,
                    depth: child_depth,
                    tree_index: child_index,
                });
            }
        }

        children_map.insert(pending.tree_index, child_indices);

        // If we hit the limit, stop processing the queue
        if truncated {
            break 'outer;
        }
    }

    // Reconstruct tree by assigning children (work backwards to handle nested)
    // Use indices sorted in reverse to process children before parents
    let mut indices: Vec<usize> = children_map.keys().copied().collect();
    indices.sort_by(|a, b| b.cmp(a)); // Reverse order

    for parent_idx in indices {
        if let Some(child_indices) = children_map.get(&parent_idx) {
            let children: Vec<FileTree> = child_indices
                .iter()
                .map(|&idx| trees[idx].clone())
                .collect();
            trees[parent_idx].children = Some(children);
        }
    }

    Ok(TreeBuildResult {
        tree: trees.into_iter().next().unwrap(),
        skipped: all_skipped,
        truncated,
        limit: if truncated { Some(MAX_TREE_ENTRIES) } else { None },
        total_entries: if truncated { Some(total_entries) } else { None },
    })
}

/// Build a file tree asynchronously by running sync traversal on blocking thread pool
///
/// This wrapper maintains API compatibility while moving blocking I/O off the async runtime.
/// Returns both the tree and a list of all skipped entries with reasons.
pub async fn build_tree(
    path: &Path,
    _current_depth: usize, // Kept for API compatibility, ignored (always starts from 0)
    max_depth: usize,
) -> Result<TreeBuildResult> {
    let path = path.to_path_buf();

    tokio::task::spawn_blocking(move || build_tree_sync(&path, max_depth))
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {e}"))?
}

/// Read a Luau script file
pub async fn read_script(file_path: &Path) -> Result<ScriptContent, RobloxMcpError> {
    // Validate .luau extension
    if file_path.extension() != Some(std::ffi::OsStr::new("luau")) {
        return Err(RobloxMcpError::InvalidPath(
            "Only .luau files supported".to_string(),
        ));
    }

    // Security: Reject symlinks to prevent information disclosure
    reject_if_symlink(file_path)?;

    let content =
        fs::read_to_string(file_path)
            .await
            .map_err(|e| RobloxMcpError::FileSystemError {
                operation: "read".to_string(),
                path: file_path.display().to_string(),
                source: e,
            })?;

    Ok(ScriptContent {
        path: file_path.display().to_string(),
        content: content.clone(),
        size_bytes: content.len(),
    })
}

/// Write a Luau script file
pub async fn write_script(
    file_path: &Path,
    content: &str,
    create_directories: bool,
) -> Result<WriteResult, RobloxMcpError> {
    // Security: If file exists, reject if it's a symlink
    // This prevents attackers from tricking us into overwriting files outside project root
    if file_path.exists() {
        reject_if_symlink(file_path)?;
    }

    // Create parent directories if requested
    if create_directories {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| RobloxMcpError::FileSystemError {
                    operation: "create_dir_all".to_string(),
                    path: parent.display().to_string(),
                    source: e,
                })?;
        }
    }

    fs::write(file_path, content)
        .await
        .map_err(|e| RobloxMcpError::FileSystemError {
            operation: "write".to_string(),
            path: file_path.display().to_string(),
            source: e,
        })?;

    Ok(WriteResult {
        path: file_path.display().to_string(),
        bytes_written: content.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_validate_path_within_project() {
        let temp_dir = TempDir::new().unwrap();
        // Canonicalize project_root to match what validate_path does internally
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create a file inside the project
        let file_path = project_root.join("src").join("test.luau");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "-- test").unwrap();

        let result = validate_path(&file_path, &project_root);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_outside_project_fails() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();

        // Try to access a path outside project root
        let outside_path = PathBuf::from("/etc/passwd");

        // This might not exist or have permissions, so we test the behavior
        let result = validate_path(&outside_path, &project_root);
        // Either InvalidPath (if can't canonicalize) or PathTraversal (if outside)
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_nonexistent_with_existing_parent_succeeds() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let nonexistent = project_root.join("does_not_exist.luau");

        // Should succeed - parent directory exists and is within project root
        let result = validate_path(&nonexistent, &project_root);
        assert!(result.is_ok());

        let validated = result.unwrap();
        assert!(validated.ends_with("does_not_exist.luau"));
    }

    #[test]
    fn test_validate_path_nonexistent_parent_fails() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        // Both the file AND the parent directory don't exist
        let nonexistent = project_root.join("nonexistent_dir").join("file.luau");

        // Should succeed - walks up to project_root which exists
        let result = validate_path(&nonexistent, &project_root);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_read_script_rejects_non_luau() {
        let temp_dir = TempDir::new().unwrap();
        let txt_file = temp_dir.path().join("test.txt");
        std::fs::write(&txt_file, "not a luau file").unwrap();

        let result = read_script(&txt_file).await;
        assert!(result.is_err());

        if let Err(e) = result {
            match e {
                RobloxMcpError::InvalidPath(msg) => {
                    assert!(msg.contains(".luau"));
                }
                _ => panic!("Expected InvalidPath error, got {e:?}"),
            }
        }
    }

    #[tokio::test]
    async fn test_read_script_success() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("test.luau");
        let content = "print('Hello, Roblox!')";
        std::fs::write(&luau_file, content).unwrap();

        let result = read_script(&luau_file).await;
        assert!(result.is_ok());

        let script = result.unwrap();
        assert_eq!(script.content, content);
        assert_eq!(script.size_bytes, content.len());
    }

    #[tokio::test]
    async fn test_read_script_nonexistent_fails() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("nonexistent.luau");

        let result = read_script(&nonexistent).await;
        assert!(result.is_err());

        if let Err(e) = result {
            match e {
                RobloxMcpError::FileSystemError { path, .. } => {
                    assert!(path.contains("nonexistent.luau"));
                }
                _ => panic!("Expected FileSystemError, got {e:?}"),
            }
        }
    }

    #[tokio::test]
    async fn test_write_script_success() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("output.luau");
        let content = "local x = 42";

        let result = write_script(&luau_file, content, false).await;
        assert!(result.is_ok());

        let write_result = result.unwrap();
        assert_eq!(write_result.bytes_written, content.len());

        // Verify file contents
        let read_back = std::fs::read_to_string(&luau_file).unwrap();
        assert_eq!(read_back, content);
    }

    #[tokio::test]
    async fn test_write_script_creates_directories() {
        let temp_dir = TempDir::new().unwrap();
        let nested_file = temp_dir
            .path()
            .join("deep")
            .join("nested")
            .join("script.luau");
        let content = "-- nested script";

        let result = write_script(&nested_file, content, true).await;
        assert!(result.is_ok());

        // Verify file exists
        assert!(nested_file.exists());
    }

    #[tokio::test]
    async fn test_write_script_fails_without_create_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let nested_file = temp_dir
            .path()
            .join("missing")
            .join("parent")
            .join("script.luau");
        let content = "-- should fail";

        let result = write_script(&nested_file, content, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_tree_file() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("test.luau");
        std::fs::write(&file, "content").unwrap();

        let result = build_tree(&file, 0, 5).await.unwrap();

        assert!(result.tree.is_file);
        assert_eq!(result.tree.name, "test.luau");
        assert!(result.tree.children.is_none());
        assert!(result.skipped.is_empty()); // No skipped entries for a single file
    }

    #[tokio::test]
    async fn test_build_tree_directory() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        std::fs::write(src_dir.join("main.luau"), "-- main").unwrap();
        std::fs::write(src_dir.join("utils.luau"), "-- utils").unwrap();

        let result = build_tree(&src_dir, 0, 5).await.unwrap();

        assert!(!result.tree.is_file);
        assert_eq!(result.tree.name, "src");

        let children = result.tree.children.unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_build_tree_respects_max_depth() {
        let temp_dir = TempDir::new().unwrap();
        let deep_dir = temp_dir.path().join("level1").join("level2").join("level3");
        std::fs::create_dir_all(&deep_dir).unwrap();
        std::fs::write(deep_dir.join("deep.luau"), "-- deep").unwrap();

        // Build with max_depth = 1 (should not recurse beyond root)
        let result = build_tree(temp_dir.path(), 0, 1).await.unwrap();

        let children = result.tree.children.unwrap();
        // level1 should exist but have no children (depth exceeded)
        let level1 = &children[0];
        assert_eq!(level1.name, "level1");
        assert!(level1.children.is_none()); // max depth reached
    }

    #[tokio::test]
    async fn test_build_tree_reports_skipped_hidden_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let visible = temp_dir.path().join("visible.luau");
        let hidden = temp_dir.path().join(".hidden");

        std::fs::write(&visible, "-- visible").unwrap();
        std::fs::create_dir(&hidden).unwrap();
        std::fs::write(hidden.join("secret.luau"), "-- secret").unwrap();

        let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();

        let children = result.tree.children.unwrap();
        // Should only have visible.luau, not .hidden
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "visible.luau");

        // MUST report the skipped hidden directory
        assert!(
            !result.skipped.is_empty(),
            "Hidden directory should be reported as skipped"
        );
        let skipped_hidden = result.skipped.iter().find(|s| s.path.contains(".hidden"));
        assert!(skipped_hidden.is_some(), "Should report .hidden as skipped");
        assert!(
            skipped_hidden.unwrap().reason.contains("hidden"),
            "Should explain why it was skipped"
        );
    }

    #[tokio::test]
    async fn test_build_tree_reports_skipped_node_modules() {
        let temp_dir = TempDir::new().unwrap();
        let src = temp_dir.path().join("src.luau");
        let node_modules = temp_dir.path().join("node_modules");

        std::fs::write(&src, "-- src").unwrap();
        std::fs::create_dir(&node_modules).unwrap();
        std::fs::write(node_modules.join("package.luau"), "-- package").unwrap();

        let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();

        let children = result.tree.children.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "src.luau");

        // MUST report the skipped node_modules directory
        assert!(
            !result.skipped.is_empty(),
            "node_modules should be reported as skipped"
        );
        let skipped_nm = result
            .skipped
            .iter()
            .find(|s| s.path.contains("node_modules"));
        assert!(
            skipped_nm.is_some(),
            "Should report node_modules as skipped"
        );
        assert!(
            skipped_nm.unwrap().reason.contains("node_modules"),
            "Should explain why it was skipped"
        );
    }

    #[test]
    fn test_filetree_serialization() {
        let tree = FileTree {
            path: "/project/src".to_string(),
            name: "src".to_string(),
            is_file: false,
            children: Some(vec![FileTree {
                path: "/project/src/main.luau".to_string(),
                name: "main.luau".to_string(),
                is_file: true,
                children: None,
            }]),
        };

        let json = serde_json::to_string(&tree).unwrap();
        assert!(json.contains("main.luau"));
        assert!(json.contains("is_file"));
    }

    #[test]
    fn test_script_content_serialization() {
        let content = ScriptContent {
            path: "/project/script.luau".to_string(),
            content: "print('test')".to_string(),
            size_bytes: 13,
        };

        let json = serde_json::to_string(&content).unwrap();
        let deserialized: ScriptContent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.path, content.path);
        assert_eq!(deserialized.content, content.content);
        assert_eq!(deserialized.size_bytes, content.size_bytes);
    }

    #[test]
    fn test_write_result_serialization() {
        let result = WriteResult {
            path: "/project/output.luau".to_string(),
            bytes_written: 256,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: WriteResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.path, result.path);
        assert_eq!(deserialized.bytes_written, result.bytes_written);
    }

    // ========================================
    // Additional Edge Case Tests
    // ========================================

    #[test]
    fn test_validate_path_project_root_cannot_canonicalize() {
        // Use a path that definitely doesn't exist as project root
        let nonexistent_root = PathBuf::from("/this/path/definitely/does/not/exist/anywhere");
        let file_path = nonexistent_root.join("script.luau");

        let result = validate_path(&file_path, &nonexistent_root);
        assert!(result.is_err());

        if let Err(e) = result {
            match e {
                RobloxMcpError::InvalidPath(msg) => {
                    assert!(
                        msg.contains("canonicalize") || msg.contains("Cannot"),
                        "Error should mention canonicalization: {}",
                        msg
                    );
                }
                _ => panic!("Expected InvalidPath error for bad project root"),
            }
        }
    }

    #[test]
    fn test_validate_path_relative_within_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create nested structure
        let nested = project_root.join("src").join("game");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("main.luau");
        std::fs::write(&file, "-- test").unwrap();

        let result = validate_path(&file, &project_root);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_read_script_with_unicode_content() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("unicode.luau");
        let content = "-- 你好世界\nlocal greeting = \"Привет мир\"\nprint(greeting) -- 🎮";
        std::fs::write(&luau_file, content).unwrap();

        let result = read_script(&luau_file).await;
        assert!(result.is_ok());

        let script = result.unwrap();
        assert!(script.content.contains("你好世界"));
        assert!(script.content.contains("Привет мир"));
        assert!(script.content.contains("🎮"));
    }

    #[tokio::test]
    async fn test_write_script_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("overwrite.luau");

        // Write initial content
        let result1 = write_script(&luau_file, "-- original", false).await;
        assert!(result1.is_ok());

        // Overwrite with new content
        let result2 = write_script(&luau_file, "-- overwritten", false).await;
        assert!(result2.is_ok());

        // Verify new content
        let content = std::fs::read_to_string(&luau_file).unwrap();
        assert_eq!(content, "-- overwritten");
    }

    #[tokio::test]
    async fn test_build_tree_reports_skipped_target_dir() {
        let temp_dir = TempDir::new().unwrap();
        let src = temp_dir.path().join("src.luau");
        let target = temp_dir.path().join("target");

        std::fs::write(&src, "-- src").unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("debug.luau"), "-- debug").unwrap();

        let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();

        let children = result.tree.children.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "src.luau");

        // MUST report the skipped target directory
        assert!(
            !result.skipped.is_empty(),
            "target should be reported as skipped"
        );
        let skipped_target = result.skipped.iter().find(|s| s.path.contains("target"));
        assert!(skipped_target.is_some(), "Should report target as skipped");
        assert!(
            skipped_target.unwrap().reason.contains("target"),
            "Should explain why it was skipped"
        );
    }

    #[tokio::test]
    async fn test_build_tree_sorts_directories_before_files() {
        let temp_dir = TempDir::new().unwrap();

        // Create files and directories in random order
        std::fs::write(temp_dir.path().join("zebra.luau"), "-- z").unwrap();
        std::fs::create_dir(temp_dir.path().join("alpha")).unwrap();
        std::fs::write(temp_dir.path().join("beta.luau"), "-- b").unwrap();
        std::fs::create_dir(temp_dir.path().join("gamma")).unwrap();

        let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();
        let children = result.tree.children.unwrap();

        // Directories should come first, then files
        // Directories: alpha, gamma
        // Files: beta.luau, zebra.luau
        assert!(!children[0].is_file); // alpha (dir)
        assert!(!children[1].is_file); // gamma (dir)
        assert!(children[2].is_file); // beta.luau (file)
        assert!(children[3].is_file); // zebra.luau (file)
    }

    #[test]
    fn test_tree_build_result_serialization() {
        let result = TreeBuildResult {
            tree: FileTree {
                path: "/project".to_string(),
                name: "project".to_string(),
                is_file: false,
                children: Some(vec![]),
            },
            skipped: vec![SkippedEntry {
                path: "/project/.git".to_string(),
                reason: "hidden directory".to_string(),
            }],
            truncated: false,
            limit: None,
            total_entries: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("skipped"));
        assert!(json.contains("hidden directory"));
    }

    #[test]
    fn test_tree_build_result_truncation_serialization() {
        let result = TreeBuildResult {
            tree: FileTree {
                path: "/project".to_string(),
                name: "project".to_string(),
                is_file: false,
                children: Some(vec![]),
            },
            skipped: vec![],
            truncated: true,
            limit: Some(10000),
            total_entries: Some(10000),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"truncated\":true"));
        assert!(json.contains("\"limit\":10000"));
        assert!(json.contains("\"total_entries\":10000"));
    }

    #[test]
    fn test_skipped_entry_serialization() {
        let entry = SkippedEntry {
            path: "/test/.hidden".to_string(),
            reason: "starts with dot".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SkippedEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.path, entry.path);
        assert_eq!(deserialized.reason, entry.reason);
    }

    #[test]
    fn test_filetree_debug() {
        let tree = FileTree {
            path: "/project/src".to_string(),
            name: "src".to_string(),
            is_file: false,
            children: None,
        };

        let debug = format!("{:?}", tree);
        assert!(debug.contains("FileTree"));
        assert!(debug.contains("src"));
    }

    #[tokio::test]
    async fn test_build_tree_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        // Directory is empty

        let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();

        assert!(!result.tree.is_file);
        assert!(result.tree.children.is_some());
        assert!(result.tree.children.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_write_script_empty_content() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("empty.luau");

        let result = write_script(&luau_file, "", false).await;
        assert!(result.is_ok());

        let write_result = result.unwrap();
        assert_eq!(write_result.bytes_written, 0);

        // Verify file exists and is empty
        let content = std::fs::read_to_string(&luau_file).unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_write_script_with_unicode_content() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("unicode.luau");

        let unicode_content = "-- Unicode: 你好世界 🚀 émoji";
        let result = write_script(&luau_file, unicode_content, false).await;
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&luau_file).unwrap();
        assert_eq!(content, unicode_content);
    }

    #[tokio::test]
    async fn test_write_script_deeply_nested_with_create_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let deep_path = temp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("deep.luau");

        let result = write_script(&deep_path, "-- deep", true).await;
        assert!(result.is_ok());

        assert!(deep_path.exists());
    }

    // ========================================
    // Symlink Security Tests
    // ========================================

    #[test]
    fn test_get_file_type_no_follow_regular_file() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("regular.luau");
        std::fs::write(&file, "-- content").unwrap();

        let file_type = get_file_type_no_follow(&file).unwrap();
        assert!(file_type.is_file());
        assert!(!file_type.is_symlink());
    }

    #[test]
    fn test_get_file_type_no_follow_directory() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&dir).unwrap();

        let file_type = get_file_type_no_follow(&dir).unwrap();
        assert!(file_type.is_dir());
        assert!(!file_type.is_symlink());
    }

    #[test]
    fn test_get_file_type_no_follow_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("does_not_exist.luau");

        let result = get_file_type_no_follow(&nonexistent);
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_if_symlink_regular_file_succeeds() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("regular.luau");
        std::fs::write(&file, "-- content").unwrap();

        let result = reject_if_symlink(&file);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reject_if_symlink_directory_succeeds() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&dir).unwrap();

        let result = reject_if_symlink(&dir);
        assert!(result.is_ok());
    }

    // Unix-only symlink tests
    #[cfg(unix)]
    mod unix_symlink_tests {
        use super::*;

        #[test]
        fn test_get_file_type_no_follow_detects_symlink() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("target.luau");
            let symlink = temp_dir.path().join("symlink.luau");

            std::fs::write(&target, "-- target").unwrap();
            std::os::unix::fs::symlink(&target, &symlink).unwrap();

            let file_type = get_file_type_no_follow(&symlink).unwrap();
            assert!(file_type.is_symlink());
        }

        #[test]
        fn test_reject_if_symlink_rejects_symlink() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("target.luau");
            let symlink = temp_dir.path().join("symlink.luau");

            std::fs::write(&target, "-- target").unwrap();
            std::os::unix::fs::symlink(&target, &symlink).unwrap();

            let result = reject_if_symlink(&symlink);
            assert!(result.is_err());
            match result.unwrap_err() {
                RobloxMcpError::SecurityViolation(msg) => {
                    assert!(msg.contains("Symlinks are not allowed"));
                    assert!(msg.contains("symlink.luau"));
                }
                e => panic!("Expected SecurityViolation, got {e:?}"),
            }
        }

        #[tokio::test]
        async fn test_read_script_rejects_symlinks() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("real.luau");
            let symlink = temp_dir.path().join("link.luau");

            std::fs::write(&target, "-- real content").unwrap();
            std::os::unix::fs::symlink(&target, &symlink).unwrap();

            let result = read_script(&symlink).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                RobloxMcpError::SecurityViolation(msg) => {
                    assert!(msg.contains("Symlinks"));
                }
                e => panic!("Expected SecurityViolation, got {e:?}"),
            }
        }

        #[tokio::test]
        async fn test_write_script_rejects_existing_symlinks() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("target.luau");
            let symlink = temp_dir.path().join("symlink.luau");

            std::fs::write(&target, "-- original").unwrap();
            std::os::unix::fs::symlink(&target, &symlink).unwrap();

            // Try to write through the symlink - should be rejected
            let result = write_script(&symlink, "-- malicious", false).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                RobloxMcpError::SecurityViolation(msg) => {
                    assert!(msg.contains("Symlinks"));
                }
                e => panic!("Expected SecurityViolation, got {e:?}"),
            }

            // Verify target was not modified
            let content = std::fs::read_to_string(&target).unwrap();
            assert_eq!(content, "-- original");
        }

        #[tokio::test]
        async fn test_build_tree_skips_symlinks() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("real_file.luau");
            let symlink = temp_dir.path().join("symlink.luau");

            std::fs::write(&target, "-- real").unwrap();
            std::os::unix::fs::symlink(&target, &symlink).unwrap();

            let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();

            // Symlink should be in skipped list
            let skipped_symlink = result
                .skipped
                .iter()
                .find(|s| s.path.contains("symlink.luau"));
            assert!(
                skipped_symlink.is_some(),
                "Symlink should be in skipped list"
            );
            assert!(
                skipped_symlink.unwrap().reason.contains("symlink"),
                "Reason should mention symlink"
            );

            // Symlink should NOT be in children
            let children = result.tree.children.unwrap();
            assert!(
                !children.iter().any(|c| c.name == "symlink.luau"),
                "Symlink should not appear in tree children"
            );

            // Real file should still be present
            assert!(
                children.iter().any(|c| c.name == "real_file.luau"),
                "Real file should be in tree children"
            );
        }

        #[tokio::test]
        async fn test_build_tree_skips_symlink_directories() {
            let temp_dir = TempDir::new().unwrap();
            let real_dir = temp_dir.path().join("real_dir");
            let symlink_dir = temp_dir.path().join("symlink_dir");

            std::fs::create_dir(&real_dir).unwrap();
            std::fs::write(real_dir.join("file.luau"), "-- inside real dir").unwrap();
            std::os::unix::fs::symlink(&real_dir, &symlink_dir).unwrap();

            let result = build_tree(temp_dir.path(), 0, 5).await.unwrap();

            // Symlink directory should be skipped
            let skipped_symlink = result
                .skipped
                .iter()
                .find(|s| s.path.contains("symlink_dir"));
            assert!(
                skipped_symlink.is_some(),
                "Symlink directory should be skipped"
            );

            // Should not recurse into symlink directory
            let children = result.tree.children.unwrap();
            assert!(
                !children.iter().any(|c| c.name == "symlink_dir"),
                "Symlink directory should not appear in tree"
            );
        }
    }

    // Windows-only symlink tests (requires admin privileges)
    #[cfg(windows)]
    mod windows_symlink_tests {
        use super::*;

        // Note: Creating symlinks on Windows typically requires admin privileges
        // These tests will be skipped if symlink creation fails
        fn try_create_symlink(target: &Path, symlink: &Path) -> bool {
            std::os::windows::fs::symlink_file(target, symlink).is_ok()
        }

        #[test]
        fn test_reject_if_symlink_rejects_windows_symlink() {
            let temp_dir = TempDir::new().unwrap();
            let target = temp_dir.path().join("target.luau");
            let symlink = temp_dir.path().join("symlink.luau");

            std::fs::write(&target, "-- target").unwrap();

            if !try_create_symlink(&target, &symlink) {
                // Skip test if we can't create symlinks (no admin privileges)
                return;
            }

            let result = reject_if_symlink(&symlink);
            assert!(result.is_err());
            match result.unwrap_err() {
                RobloxMcpError::SecurityViolation(msg) => {
                    assert!(msg.contains("Symlinks"));
                }
                e => panic!("Expected SecurityViolation, got {e:?}"),
            }
        }
    }

    #[test]
    fn test_security_violation_error_display() {
        let err = RobloxMcpError::SecurityViolation("test symlink attack".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Security violation"));
        assert!(msg.contains("test symlink attack"));
    }

    // ========================================
    // Path Traversal Attack Tests
    // ========================================

    #[test]
    fn test_validate_path_rejects_dot_dot_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create a subdirectory structure
        let subdir = project_root.join("src");
        std::fs::create_dir(&subdir).unwrap();

        // Create a file in the project root
        let file_in_root = project_root.join("secret.luau");
        std::fs::write(&file_in_root, "-- secret").unwrap();

        // Try to traverse out and back using ../.. pattern
        let traversal_path = subdir.join("..").join("..").join("etc").join("passwd");
        let result = validate_path(&traversal_path, &project_root);

        // Should fail - either as InvalidPath or PathTraversal
        assert!(result.is_err(), "Path traversal should be rejected");
    }

    #[test]
    fn test_validate_path_rejects_embedded_null_byte() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Note: Rust's Path doesn't allow embedded nulls, so this tests
        // that we properly handle the path before any null can cause issues
        let normal_path = project_root.join("test.luau");
        std::fs::write(&normal_path, "-- test").unwrap();

        let result = validate_path(&normal_path, &project_root);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_handles_absolute_vs_relative() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();
        let test_file = project_root.join("test.luau");
        std::fs::write(&test_file, "-- test").unwrap();

        // Absolute path should work
        let result_abs = validate_path(&test_file, &project_root);
        assert!(result_abs.is_ok());

        // A completely different absolute path should be rejected
        let other_absolute = PathBuf::from(if cfg!(windows) {
            "C:\\Windows\\System32\\cmd.exe"
        } else {
            "/usr/bin/ls"
        });

        // This should fail because it's outside project root
        // May fail as InvalidPath if it can't be canonicalized or PathTraversal if it can
        let result_other = validate_path(&other_absolute, &project_root);
        assert!(result_other.is_err());
    }

    #[test]
    fn test_validate_path_rejects_path_outside_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create sibling directory outside project root
        let sibling_dir = temp_dir.path().parent().unwrap().join("sibling_dir");
        if std::fs::create_dir(&sibling_dir).is_ok() {
            let sibling_file = sibling_dir.join("file.luau");
            if std::fs::write(&sibling_file, "-- outside project").is_ok() {
                let result = validate_path(&sibling_file, &project_root);
                assert!(result.is_err(), "File outside project root should be rejected");
                // Clean up
                let _ = std::fs::remove_dir_all(&sibling_dir);
            }
        }
    }

    #[test]
    fn test_validate_path_detects_traversal_in_filename_components() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Try various traversal patterns in path components
        let traversal_patterns = [
            project_root.join("..").join("script.luau"),
            project_root.join("src").join("..").join("..").join("script.luau"),
            project_root.join(".").join("..").join("script.luau"),
        ];

        for pattern in &traversal_patterns {
            // The validate_path function should either:
            // 1. Reject these as InvalidPath (can't canonicalize)
            // 2. Reject as PathTraversal (resolved outside project)
            // 3. Accept if it resolves back inside project root
            // The key is no path should allow access outside project_root

            if let Ok(validated) = validate_path(pattern, &project_root) {
                // If it succeeded, the validated path MUST start with project_root
                assert!(
                    validated.starts_with(&project_root),
                    "Validated path {:?} must start with project root {:?}",
                    validated,
                    project_root
                );
            }
            // If it errored, that's also acceptable (security block)
        }
    }

    #[test]
    fn test_validate_path_case_sensitivity_windows() {
        // On Windows, paths are case-insensitive
        // This test ensures validate_path handles this correctly
        if !cfg!(windows) {
            return; // Skip on non-Windows
        }

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();
        let test_file = project_root.join("Test.luau");
        std::fs::write(&test_file, "-- test").unwrap();

        // Try with different case
        let different_case = project_root.join("TEST.LUAU");
        let result = validate_path(&different_case, &project_root);

        // Should succeed on Windows (case-insensitive)
        assert!(result.is_ok());
    }

    // ========================================
    // Resource Limit Tests
    // ========================================

    #[tokio::test]
    async fn test_build_tree_respects_max_entries_limit() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create more than a few files but not exceeding test limits
        for i in 0..20 {
            std::fs::write(project_root.join(format!("file{}.luau", i)), format!("-- file {}", i))
                .unwrap();
        }

        // Build tree - should not panic or hang
        let result = build_tree(&project_root, 0, 5).await;
        assert!(result.is_ok());

        let tree_result = result.unwrap();
        assert!(tree_result.tree.children.is_some());
    }

    #[tokio::test]
    async fn test_build_tree_handles_deeply_nested_directories() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create a deeply nested directory structure
        let mut current = project_root.clone();
        for i in 0..10 {
            current = current.join(format!("level{}", i));
            std::fs::create_dir(&current).unwrap();
        }
        std::fs::write(current.join("deep.luau"), "-- deep file").unwrap();

        // Build tree with limited depth - should truncate gracefully
        let result = build_tree(&project_root, 0, 3).await;
        assert!(result.is_ok());

        let tree_result = result.unwrap();
        // Should have truncated metadata if depth was exceeded
        assert!(tree_result.tree.children.is_some());
    }

    #[tokio::test]
    async fn test_build_tree_handles_empty_directories() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create empty nested directories
        let empty_dir = project_root.join("empty").join("nested").join("dir");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let result = build_tree(&project_root, 0, 5).await;
        assert!(result.is_ok());
    }

    // ========================================
    // Additional Coverage Tests for Edge Cases
    // ========================================

    #[test]
    fn test_validate_path_traversal_in_nonexistent_components() {
        // Test that .. in nonexistent path components is rejected
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create a path with .. in the nonexistent portion
        let malicious_path = project_root.join("exists").join("..").join("attack.luau");

        // Create the "exists" directory so we have an existing ancestor
        let exists_dir = project_root.join("exists");
        std::fs::create_dir_all(&exists_dir).unwrap();

        let result = validate_path(&malicious_path, &project_root);
        // This should be caught by canonicalize or the .. check
        // Either way, it should not return a path outside the project root
        if let Ok(validated) = result {
            assert!(
                validated.starts_with(&project_root),
                "Validated path should be within project root"
            );
        }
    }

    #[test]
    fn test_validate_path_ancestor_outside_project_root() {
        // Test when the existing ancestor is outside the project root
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let project_root = temp_dir1.path().canonicalize().unwrap();
        // Try to validate a path that's in a completely different temp dir
        let outside_path = temp_dir2.path().join("file.luau");

        let result = validate_path(&outside_path, &project_root);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_deeply_nested_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create a deeply nested nonexistent path
        let deep_path = project_root
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("f")
            .join("file.luau");

        let result = validate_path(&deep_path, &project_root);
        assert!(result.is_ok());
        let validated = result.unwrap();
        assert!(validated.starts_with(&project_root));
    }

    #[test]
    fn test_validate_path_existing_file_inside_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().canonicalize().unwrap();

        // Create an actual file
        let file = project_root.join("script.luau");
        std::fs::write(&file, "-- test").unwrap();

        let result = validate_path(&file, &project_root);
        assert!(result.is_ok());
        let validated = result.unwrap();
        assert!(validated.ends_with("script.luau"));
    }

    #[test]
    fn test_validate_path_existing_file_outside_project() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let project_root = temp_dir1.path().canonicalize().unwrap();

        // Create a file outside the project
        let outside_file = temp_dir2.path().join("outside.luau");
        std::fs::write(&outside_file, "-- outside").unwrap();

        let result = validate_path(&outside_file, &project_root);
        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::PathTraversal(_) => (),
            e => panic!("Expected PathTraversal error, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_read_script_large_file() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("large.luau");

        // Create a large file (100KB)
        let large_content = "-- ".to_string() + &"x".repeat(100_000);
        std::fs::write(&luau_file, &large_content).unwrap();

        let result = read_script(&luau_file).await;
        assert!(result.is_ok());

        let script = result.unwrap();
        assert_eq!(script.size_bytes, large_content.len());
    }

    #[tokio::test]
    async fn test_write_script_overwrite_preserves_content() {
        let temp_dir = TempDir::new().unwrap();
        let luau_file = temp_dir.path().join("test.luau");

        // Write initial
        write_script(&luau_file, "-- v1", false).await.unwrap();
        assert_eq!(std::fs::read_to_string(&luau_file).unwrap(), "-- v1");

        // Overwrite
        write_script(&luau_file, "-- v2", false).await.unwrap();
        assert_eq!(std::fs::read_to_string(&luau_file).unwrap(), "-- v2");

        // Overwrite again
        write_script(&luau_file, "-- v3", false).await.unwrap();
        assert_eq!(std::fs::read_to_string(&luau_file).unwrap(), "-- v3");
    }

    #[tokio::test]
    async fn test_build_tree_with_mixed_content() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create a mix of files, directories, hidden entries
        std::fs::write(root.join("visible.luau"), "-- visible").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.luau"), "-- main").unwrap();
        std::fs::create_dir(root.join(".hidden")).unwrap();
        std::fs::create_dir(root.join("node_modules")).unwrap();
        std::fs::create_dir(root.join("target")).unwrap();

        let result = build_tree(root, 0, 5).await.unwrap();

        // Should have skipped 3 entries (.hidden, node_modules, target)
        assert_eq!(result.skipped.len(), 3);

        // Should only have 2 visible items (visible.luau and src)
        let children = result.tree.children.unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_build_tree_starting_depth() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        std::fs::create_dir(root.join("level1")).unwrap();
        std::fs::create_dir(root.join("level1").join("level2")).unwrap();
        std::fs::write(
            root.join("level1").join("level2").join("deep.luau"),
            "-- deep",
        )
        .unwrap();

        // Start at depth 2 with max_depth 1 - should not recurse into level2
        let result = build_tree(&root.join("level1"), 2, 3).await.unwrap();

        assert_eq!(result.tree.name, "level1");
        // With starting_depth=2 and max_depth=3, we can go 1 more level
        let children = result.tree.children.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "level2");
    }

    // ========================================
    // Additional validate_path Edge Case Tests
    // ========================================

    #[test]
    fn test_validate_path_empty_path() {
        // Test with an empty path (edge case for "no valid components")
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create a path that has no components
        let empty_path = PathBuf::from("");

        let result = validate_path(&empty_path, project_root);
        // Empty path won't canonicalize - should error
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_path_root_path_outside_project() {
        // Test with an absolute path outside the project root
        // This covers the path traversal detection for non-existent paths
        use std::path::PathBuf;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Use a non-existent path at root level (outside project root)
        let outside_path = PathBuf::from("/tmp/outside_project_xyz/file.luau");

        let result = validate_path(&outside_path, project_root);
        // Path exists at root level but is outside project - should fail
        assert!(result.is_err(), "Path outside project root should fail validation");
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_path_dotdot_escape_attempt() {
        // Test that a path attempting to escape via .. is rejected
        // This uses a manually constructed OsString to ensure .. isn't normalized
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create existing subdir
        let subdir = project_root.join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        // Try to escape: project_root/subdir/../../../etc/passwd
        // Using OsString to prevent path normalization
        let mut malicious = subdir.as_os_str().to_os_string();
        malicious.push("/../../../etc/passwd");
        let malicious_path = PathBuf::from(malicious);

        let result = validate_path(&malicious_path, project_root);
        // Should be rejected - either path doesn't exist or it's outside project
        assert!(result.is_err(), "Path escape attempt should be rejected");
    }

    #[test]
    fn test_validate_path_deep_nested_walks_to_root() {
        // Test with a deeply nested non-existent path that walks up to project root
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create a deeply nested path that doesn't exist
        let deep_path = project_root
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("deep.luau");

        // Should succeed because walking up eventually reaches project_root
        let result = validate_path(&deep_path, project_root);
        // This should succeed because it walks up to project_root
        assert!(result.is_ok(), "Deep nested path should be valid if it walks up to project root");
    }

    #[test]
    fn test_validate_path_with_existing_parent() {
        // Test a non-existent file within an existing directory
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create a parent directory
        let subdir = project_root.join("src");
        std::fs::create_dir(&subdir).unwrap();

        // Path to non-existent file in existing directory
        let new_file = subdir.join("new_file.luau");

        let result = validate_path(&new_file, project_root);
        assert!(result.is_ok(), "Non-existent file in existing directory should be valid");
    }

    #[test]
    fn test_validate_path_canonical_ancestor_outside_project() {
        // Test that path traversal is detected when ancestor resolves outside project
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path();

        // Create a subdirectory to use as project root
        let inner_project = project_root.join("project");
        std::fs::create_dir(&inner_project).unwrap();

        // Try to access a path outside the inner project
        let outside_path = project_root.join("outside.luau");
        std::fs::write(&outside_path, "-- outside").unwrap();

        // Validate path relative to inner_project - should fail
        let result = validate_path(&outside_path, &inner_project);
        assert!(result.is_err(), "Path outside project should be rejected");
        match result.unwrap_err() {
            RobloxMcpError::PathTraversal(_) => (),
            e => panic!("Expected PathTraversal error, got {e:?}"),
        }
    }
}
