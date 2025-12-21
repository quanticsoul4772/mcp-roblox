//! File watching for real-time change detection
//!
//! Uses the `notify` crate to track filesystem changes in the project.
//! Changes are queued and can be polled via the `fs_watch_changes` MCP tool.

use crate::error::RobloxMcpError;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tokio::sync::RwLock;
use tracing::{debug, error};

/// Maximum changes to queue before dropping old ones
const MAX_QUEUE_SIZE: usize = 1000;

/// A recorded file change event
#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub timestamp: u64,
}

/// Type of file change
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
    /// Watcher error - filesystem watching may have stopped working
    WatcherError,
}

/// File watcher that tracks changes to .luau files
pub struct FileWatcher {
    /// The underlying filesystem watcher - must be kept alive for watching to continue
    #[allow(dead_code)]
    watcher: RecommendedWatcher,
    /// Queue of file change events waiting to be polled
    change_queue: Arc<RwLock<VecDeque<FileChange>>>,
}

impl FileWatcher {
    /// Create a new FileWatcher
    ///
    /// CRITICAL: Must be called from within a tokio runtime context.
    /// The notify callback runs on a background thread, so we capture
    /// the runtime Handle to spawn async tasks correctly.
    pub fn new(project_root: PathBuf) -> Result<Self, RobloxMcpError> {
        let change_queue = Arc::new(RwLock::new(VecDeque::new()));

        // Use Arc for project_root since it's read-only and shared across many spawned tasks
        // This makes cloning per-event O(1) pointer increment instead of O(n) path copy
        let root_arc = Arc::new(project_root.clone());

        // Clone Arc handles for the closure (cheap pointer clones)
        let queue_for_events = change_queue.clone();
        let queue_for_errors = change_queue.clone();

        // CRITICAL FIX: Capture runtime handle BEFORE creating watcher
        // notify callbacks run on a background thread (not tokio runtime),
        // so tokio::spawn() would panic without explicit handle
        let runtime_handle = Handle::current();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Arc::clone is O(1) - just atomic increment
                    let queue = queue_for_events.clone();
                    let root = root_arc.clone();

                    // Use captured handle to spawn on tokio runtime
                    runtime_handle.spawn(async move {
                        Self::handle_event(event, queue, root).await;
                    });
                }
                Err(e) => {
                    // NO SILENT FAILURE: Log and queue watcher errors
                    // This ensures users are notified if file watching stops working
                    error!("File watcher error: {}. File watching may be degraded.", e);

                    let queue = queue_for_errors.clone();
                    let error_msg = e.to_string();

                    runtime_handle.spawn(async move {
                        Self::queue_error(queue, error_msg).await;
                    });
                }
            }
        })
        .map_err(RobloxMcpError::WatcherError)?;

        // Start watching the project root
        watcher
            .watch(&project_root, RecursiveMode::Recursive)
            .map_err(RobloxMcpError::WatcherError)?;

        Ok(Self {
            watcher,
            change_queue,
        })
    }

    /// Poll for recent file changes
    pub async fn poll_changes(&self, limit: usize) -> Vec<FileChange> {
        let mut queue = self.change_queue.write().await;
        let take_count = limit.min(queue.len());
        let mut changes = Vec::with_capacity(take_count);

        for _ in 0..take_count {
            if let Some(change) = queue.pop_front() {
                changes.push(change);
            }
        }

        changes
    }

    /// Get the number of pending changes
    pub async fn pending_count(&self) -> usize {
        self.change_queue.read().await.len()
    }

    /// Queue an error event so users are notified of watcher failures
    async fn queue_error(queue: Arc<RwLock<VecDeque<FileChange>>>, error_msg: String) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut queue = queue.write().await;

        // Enforce queue size limit
        if queue.len() >= MAX_QUEUE_SIZE {
            queue.pop_front();
        }

        queue.push_back(FileChange {
            path: format!("[WATCHER_ERROR] {}", error_msg),
            kind: ChangeKind::WatcherError,
            timestamp,
        });
    }

    async fn handle_event(
        event: Event,
        queue: Arc<RwLock<VecDeque<FileChange>>>,
        project_root: Arc<PathBuf>,
    ) {
        for path in event.paths {
            // Only track .luau files
            if path.extension() != Some(OsStr::new("luau")) {
                continue;
            }

            let relative_path = path
                .strip_prefix(project_root.as_ref())
                .unwrap_or(&path)
                .display()
                .to_string();

            let kind = match event.kind {
                EventKind::Create(_) => ChangeKind::Created,
                EventKind::Modify(_) => ChangeKind::Modified,
                EventKind::Remove(_) => ChangeKind::Deleted,
                _ => continue,
            };

            debug!(?kind, path = %relative_path, "File change detected");

            // Queue change notification
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let mut queue = queue.write().await;

            // Enforce queue size limit
            if queue.len() >= MAX_QUEUE_SIZE {
                queue.pop_front();
            }

            queue.push_back(FileChange {
                path: relative_path,
                kind,
                timestamp,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_watcher_creation() {
        let temp_dir = TempDir::new().unwrap();
        let watcher = FileWatcher::new(temp_dir.path().to_path_buf());
        assert!(watcher.is_ok());
    }

    #[tokio::test]
    async fn test_poll_changes_empty() {
        let temp_dir = TempDir::new().unwrap();
        let watcher = FileWatcher::new(temp_dir.path().to_path_buf()).unwrap();

        let changes = watcher.poll_changes(10).await;
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn test_pending_count_initially_zero() {
        let temp_dir = TempDir::new().unwrap();
        let watcher = FileWatcher::new(temp_dir.path().to_path_buf()).unwrap();

        assert_eq!(watcher.pending_count().await, 0);
    }

    #[test]
    fn test_change_kind_serialization() {
        let created = serde_json::to_string(&ChangeKind::Created).unwrap();
        assert_eq!(created, "\"created\"");

        let modified = serde_json::to_string(&ChangeKind::Modified).unwrap();
        assert_eq!(modified, "\"modified\"");

        let deleted = serde_json::to_string(&ChangeKind::Deleted).unwrap();
        assert_eq!(deleted, "\"deleted\"");

        let watcher_error = serde_json::to_string(&ChangeKind::WatcherError).unwrap();
        assert_eq!(watcher_error, "\"watcher_error\"");
    }

    #[test]
    fn test_file_change_serialization() {
        let change = FileChange {
            path: "src/main.luau".to_string(),
            kind: ChangeKind::Modified,
            timestamp: 1234567890,
        };

        let json = serde_json::to_string(&change).unwrap();
        assert!(json.contains("src/main.luau"));
        assert!(json.contains("modified"));
        assert!(json.contains("1234567890"));
    }

    #[test]
    fn test_file_change_clone() {
        let change = FileChange {
            path: "test.luau".to_string(),
            kind: ChangeKind::Created,
            timestamp: 9999,
        };

        let cloned = change.clone();
        assert_eq!(cloned.path, "test.luau");
        assert_eq!(cloned.timestamp, 9999);
    }

    #[test]
    fn test_file_change_debug() {
        let change = FileChange {
            path: "script.luau".to_string(),
            kind: ChangeKind::Deleted,
            timestamp: 5555,
        };

        let debug = format!("{:?}", change);
        assert!(debug.contains("FileChange"));
        assert!(debug.contains("script.luau"));
        assert!(debug.contains("Deleted"));
    }

    #[test]
    fn test_change_kind_clone() {
        let kind = ChangeKind::Modified;
        let cloned = kind.clone();
        assert!(matches!(cloned, ChangeKind::Modified));
    }

    #[test]
    fn test_change_kind_debug() {
        let kind = ChangeKind::WatcherError;
        let debug = format!("{:?}", kind);
        assert!(debug.contains("WatcherError"));
    }

    #[tokio::test]
    async fn test_poll_changes_with_limit() {
        let temp_dir = TempDir::new().unwrap();
        let watcher = FileWatcher::new(temp_dir.path().to_path_buf()).unwrap();

        // Even with empty queue, limit should work
        let changes = watcher.poll_changes(0).await;
        assert!(changes.is_empty());

        let changes = watcher.poll_changes(100).await;
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn test_watcher_on_nonexistent_path_fails() {
        let result = FileWatcher::new(PathBuf::from("/nonexistent/path/that/does/not/exist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_file_change_with_empty_path() {
        let change = FileChange {
            path: "".to_string(),
            kind: ChangeKind::Modified,
            timestamp: 0,
        };

        let json = serde_json::to_string(&change).unwrap();
        assert!(json.contains("\"path\":\"\""));
    }

    #[test]
    fn test_file_change_with_special_characters() {
        let change = FileChange {
            path: "folder/subfolder/my script.luau".to_string(),
            kind: ChangeKind::Created,
            timestamp: 1000,
        };

        let json = serde_json::to_string(&change).unwrap();
        assert!(json.contains("my script.luau"));
    }

    #[test]
    fn test_all_change_kinds_serialization() {
        // Test each variant serializes correctly to snake_case
        let kinds = vec![
            (ChangeKind::Created, "\"created\""),
            (ChangeKind::Modified, "\"modified\""),
            (ChangeKind::Deleted, "\"deleted\""),
            (ChangeKind::WatcherError, "\"watcher_error\""),
        ];

        for (kind, expected) in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn test_max_queue_size_constant() {
        // Verify the constant exists and has a reasonable value
        let size = MAX_QUEUE_SIZE;
        assert!(size > 0);
        assert!(size <= 10000); // Reasonable upper bound
    }

    // ========================================
    // Tests for queue_error and handle_event
    // ========================================

    #[tokio::test]
    async fn test_queue_error_adds_watcher_error_entry() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));

        FileWatcher::queue_error(queue.clone(), "Test error message".to_string()).await;

        let changes = queue.read().await;
        assert_eq!(changes.len(), 1);

        let change = &changes[0];
        assert!(change.path.contains("[WATCHER_ERROR]"));
        assert!(change.path.contains("Test error message"));
        assert!(matches!(change.kind, ChangeKind::WatcherError));
        assert!(change.timestamp > 0);
    }

    #[tokio::test]
    async fn test_queue_error_respects_max_queue_size() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));

        // Fill the queue to max capacity
        for i in 0..MAX_QUEUE_SIZE {
            queue.write().await.push_back(FileChange {
                path: format!("file{}.luau", i),
                kind: ChangeKind::Modified,
                timestamp: i as u64,
            });
        }

        assert_eq!(queue.read().await.len(), MAX_QUEUE_SIZE);

        // Add one more via queue_error - should drop the oldest
        FileWatcher::queue_error(queue.clone(), "Overflow error".to_string()).await;

        let changes = queue.read().await;
        assert_eq!(changes.len(), MAX_QUEUE_SIZE);

        // First item should now be file1.luau (file0.luau was dropped)
        assert_eq!(changes[0].path, "file1.luau");

        // Last item should be the error
        let last = changes.back().unwrap();
        assert!(matches!(last.kind, ChangeKind::WatcherError));
    }

    #[tokio::test]
    async fn test_handle_event_filters_non_luau_files() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));
        let project_root = Arc::new(PathBuf::from("/project"));

        // Create an event with non-.luau files
        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![
                PathBuf::from("/project/readme.md"),
                PathBuf::from("/project/main.rs"),
                PathBuf::from("/project/config.json"),
            ],
            attrs: Default::default(),
        };

        FileWatcher::handle_event(event, queue.clone(), project_root).await;

        // Queue should be empty - no .luau files
        assert_eq!(queue.read().await.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_event_accepts_luau_files() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));
        let project_root = Arc::new(PathBuf::from("/project"));

        // Create an event with a .luau file
        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/project/script.luau")],
            attrs: Default::default(),
        };

        FileWatcher::handle_event(event, queue.clone(), project_root).await;

        let changes = queue.read().await;
        assert_eq!(changes.len(), 1);
        assert!(changes[0].path.contains("script.luau"));
        assert!(matches!(changes[0].kind, ChangeKind::Created));
    }

    #[tokio::test]
    async fn test_handle_event_maps_event_kinds() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));
        let project_root = Arc::new(PathBuf::from("/project"));

        // Test Create event
        let create_event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/project/new.luau")],
            attrs: Default::default(),
        };
        FileWatcher::handle_event(create_event, queue.clone(), project_root.clone()).await;

        // Test Modify event
        let modify_event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![PathBuf::from("/project/modified.luau")],
            attrs: Default::default(),
        };
        FileWatcher::handle_event(modify_event, queue.clone(), project_root.clone()).await;

        // Test Remove event
        let remove_event = Event {
            kind: EventKind::Remove(notify::event::RemoveKind::File),
            paths: vec![PathBuf::from("/project/deleted.luau")],
            attrs: Default::default(),
        };
        FileWatcher::handle_event(remove_event, queue.clone(), project_root).await;

        let changes = queue.read().await;
        assert_eq!(changes.len(), 3);
        assert!(matches!(changes[0].kind, ChangeKind::Created));
        assert!(matches!(changes[1].kind, ChangeKind::Modified));
        assert!(matches!(changes[2].kind, ChangeKind::Deleted));
    }

    #[tokio::test]
    async fn test_handle_event_ignores_other_event_kinds() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));
        let project_root = Arc::new(PathBuf::from("/project"));

        // Create an Access event (should be ignored)
        let event = Event {
            kind: EventKind::Access(notify::event::AccessKind::Read),
            paths: vec![PathBuf::from("/project/script.luau")],
            attrs: Default::default(),
        };

        FileWatcher::handle_event(event, queue.clone(), project_root).await;

        // Queue should be empty - Access events are ignored
        assert_eq!(queue.read().await.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_event_respects_max_queue_size() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));
        let project_root = Arc::new(PathBuf::from("/project"));

        // Fill the queue to max capacity
        for i in 0..MAX_QUEUE_SIZE {
            queue.write().await.push_back(FileChange {
                path: format!("file{}.luau", i),
                kind: ChangeKind::Modified,
                timestamp: i as u64,
            });
        }

        // Add one more via handle_event
        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/project/overflow.luau")],
            attrs: Default::default(),
        };
        FileWatcher::handle_event(event, queue.clone(), project_root).await;

        let changes = queue.read().await;
        assert_eq!(changes.len(), MAX_QUEUE_SIZE);

        // Last item should be the new file
        let last = changes.back().unwrap();
        assert!(last.path.contains("overflow.luau"));
    }

    #[tokio::test]
    async fn test_handle_event_strips_project_root_prefix() {
        let queue = Arc::new(RwLock::new(VecDeque::new()));
        let project_root = Arc::new(PathBuf::from("/project/root"));

        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/project/root/src/main.luau")],
            attrs: Default::default(),
        };

        FileWatcher::handle_event(event, queue.clone(), project_root).await;

        let changes = queue.read().await;
        assert_eq!(changes.len(), 1);
        // Path should be relative (prefix stripped)
        assert!(!changes[0].path.starts_with("/project/root"));
        assert!(changes[0].path.contains("main.luau"));
    }
}
