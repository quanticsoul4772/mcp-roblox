use crate::error::RobloxMcpError;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize)]
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
pub fn validate_path(requested: &Path, project_root: &Path) -> Result<PathBuf, RobloxMcpError> {
    // Canonicalize both paths to ensure consistent comparison
    // This handles Windows \\?\ prefix and other platform differences
    let canonical_root = project_root.canonicalize().map_err(|e| {
        RobloxMcpError::InvalidPath(format!("Cannot canonicalize project root: {e}"))
    })?;

    let canonical = requested
        .canonicalize()
        .map_err(|e| RobloxMcpError::InvalidPath(e.to_string()))?;

    if !canonical.starts_with(&canonical_root) {
        return Err(RobloxMcpError::PathTraversal(
            canonical.display().to_string(),
        ));
    }

    Ok(canonical)
}

/// Build a file tree recursively (boxed for async recursion)
/// Returns both the tree and a list of all skipped entries with reasons
pub async fn build_tree(
    path: &Path,
    current_depth: usize,
    max_depth: usize,
) -> Result<TreeBuildResult> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Path has no file name: {}", path.display()))?
        .to_string_lossy()
        .to_string();

    let metadata = fs::metadata(path).await?;

    if metadata.is_file() {
        return Ok(TreeBuildResult {
            tree: FileTree {
                path: path.display().to_string(),
                name,
                is_file: true,
                children: None,
            },
            skipped: vec![],
        });
    }

    // For directories, recurse if we haven't hit max depth
    let mut all_skipped: Vec<SkippedEntry> = vec![];

    let children = if current_depth < max_depth {
        let mut entries = vec![];
        let mut dir = fs::read_dir(path).await?;

        while let Some(entry) = dir.next_entry().await? {
            let child_path = entry.path();

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

            // Box the recursive call to avoid infinite sized future
            let child_result =
                Box::pin(build_tree(&child_path, current_depth + 1, max_depth)).await?;
            entries.push(child_result.tree);
            // Collect skipped entries from children
            all_skipped.extend(child_result.skipped);
        }

        // Sort entries: directories first, then files, alphabetically within each group
        entries.sort_by(|a, b| {
            match (a.is_file, b.is_file) {
                (false, true) => std::cmp::Ordering::Less, // directories before files
                (true, false) => std::cmp::Ordering::Greater, // files after directories
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()), // alphabetical within group
            }
        });

        Some(entries)
    } else {
        None
    };

    Ok(TreeBuildResult {
        tree: FileTree {
            path: path.display().to_string(),
            name,
            is_file: false,
            children,
        },
        skipped: all_skipped,
    })
}

/// Read a Luau script file
pub async fn read_script(file_path: &Path) -> Result<ScriptContent, RobloxMcpError> {
    // Validate .luau extension
    if file_path.extension() != Some(std::ffi::OsStr::new("luau")) {
        return Err(RobloxMcpError::InvalidPath(
            "Only .luau files supported".to_string(),
        ));
    }

    let content =
        fs::read_to_string(file_path)
            .await
            .map_err(|e| RobloxMcpError::FileSystemError {
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
    // Create parent directories if requested
    if create_directories {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| RobloxMcpError::FileSystemError {
                    path: parent.display().to_string(),
                    source: e,
                })?;
        }
    }

    fs::write(file_path, content)
        .await
        .map_err(|e| RobloxMcpError::FileSystemError {
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
    fn test_validate_path_nonexistent_fails() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().to_path_buf();
        let nonexistent = project_root.join("does_not_exist.luau");

        let result = validate_path(&nonexistent, &project_root);
        assert!(result.is_err());

        // Check it's an InvalidPath error
        if let Err(e) = result {
            match e {
                RobloxMcpError::InvalidPath(_) => (),
                _ => panic!("Expected InvalidPath error, got {e:?}"),
            }
        }
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
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("skipped"));
        assert!(json.contains("hidden directory"));
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
}
