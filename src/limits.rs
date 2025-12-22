//! Resource limits for filesystem operations
//!
//! These limits prevent unbounded memory growth and oversized responses.
//! All limits are chosen to balance usability with resource safety.

/// Maximum number of search results returned by fs_search_content
///
/// Prevents unbounded growth when searching patterns that match many lines.
/// Callers can refine their search pattern if results are truncated.
pub const MAX_SEARCH_RESULTS: usize = 1000;

/// Maximum number of files tracked by fs_get_changes
///
/// Prevents excessive memory usage when scanning large codebases.
/// Most Roblox projects have far fewer than 10,000 .luau files.
pub const MAX_FILE_ENTRIES: usize = 10000;

/// Maximum number of entries (files + directories) in build_tree
///
/// Prevents tree building from consuming excessive memory.
/// Use max_depth parameter for more targeted tree exploration.
pub const MAX_TREE_ENTRIES: usize = 10000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_are_reasonable() {
        // Ensure limits are positive and reasonable
        assert!(MAX_SEARCH_RESULTS > 0);
        assert!(MAX_SEARCH_RESULTS <= 10000);

        assert!(MAX_FILE_ENTRIES > 0);
        assert!(MAX_FILE_ENTRIES <= 100000);

        assert!(MAX_TREE_ENTRIES > 0);
        assert!(MAX_TREE_ENTRIES <= 100000);
    }

    #[test]
    fn test_search_results_limit() {
        // Verify the constant value
        assert_eq!(MAX_SEARCH_RESULTS, 1000);
    }

    #[test]
    fn test_file_entries_limit() {
        // Verify the constant value
        assert_eq!(MAX_FILE_ENTRIES, 10000);
    }

    #[test]
    fn test_tree_entries_limit() {
        // Verify the constant value
        assert_eq!(MAX_TREE_ENTRIES, 10000);
    }
}
