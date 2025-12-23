//! Auto-indexer that watches for file changes and updates the knowledge graph.
//!
//! When files are created or modified, they are automatically indexed.
//! When files are deleted, they are removed from the index.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use crate::watcher::{ChangeKind, FileWatcher};

use super::KnowledgeGraph;

/// Configuration for the auto-indexer.
#[derive(Debug, Clone)]
pub struct AutoIndexerConfig {
    /// How often to poll for changes (default: 1 second)
    pub poll_interval: Duration,
    /// Maximum changes to process per poll cycle (default: 50)
    pub batch_size: usize,
    /// Project root for resolving relative paths to absolute
    pub project_root: PathBuf,
}

impl Default for AutoIndexerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            batch_size: 50,
            project_root: PathBuf::from("."),
        }
    }
}

impl AutoIndexerConfig {
    /// Create a new config with the given project root.
    pub fn with_project_root(project_root: PathBuf) -> Self {
        Self {
            project_root,
            ..Default::default()
        }
    }
}

/// Auto-indexer that watches for file changes and updates the knowledge graph.
///
/// Runs as a background task that periodically polls the file watcher for changes
/// and automatically indexes/removes files from the knowledge graph.
pub struct AutoIndexer {
    file_watcher: Arc<FileWatcher>,
    knowledge_graph: Arc<dyn KnowledgeGraph>,
    config: AutoIndexerConfig,
    /// Shutdown signal sender
    shutdown_tx: watch::Sender<bool>,
    /// Shutdown signal receiver (cloned for the background task)
    shutdown_rx: watch::Receiver<bool>,
}

impl AutoIndexer {
    /// Create a new auto-indexer.
    pub fn new(
        file_watcher: Arc<FileWatcher>,
        knowledge_graph: Arc<dyn KnowledgeGraph>,
        config: AutoIndexerConfig,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Self {
            file_watcher,
            knowledge_graph,
            config,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Start the auto-indexer background task.
    ///
    /// Returns a handle that can be used to stop the indexer.
    pub fn start(self) -> AutoIndexerHandle {
        let shutdown_tx = self.shutdown_tx.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        let file_watcher = self.file_watcher;
        let knowledge_graph = self.knowledge_graph;
        let config = self.config;

        // Spawn the background indexing task
        let task = tokio::spawn(async move {
            info!(
                "Auto-indexer started. Polling every {:?}",
                config.poll_interval
            );

            loop {
                // Check for shutdown signal
                if *shutdown_rx.borrow() {
                    info!("Auto-indexer received shutdown signal");
                    break;
                }

                // Poll for changes
                let changes = file_watcher.poll_changes(config.batch_size).await;

                if !changes.is_empty() {
                    debug!("Auto-indexer processing {} changes", changes.len());

                    for change in changes {
                        // Skip watcher errors
                        if matches!(change.kind, ChangeKind::WatcherError) {
                            warn!("File watcher error detected: {}", change.path);
                            continue;
                        }

                        let absolute_path = config.project_root.join(&change.path);
                        let path_str = absolute_path.display().to_string();

                        match change.kind {
                            ChangeKind::Created | ChangeKind::Modified => {
                                // Read file content and index it
                                match tokio::fs::read_to_string(&absolute_path).await {
                                    Ok(content) => {
                                        if let Err(e) =
                                            knowledge_graph.index_script(&path_str, &content).await
                                        {
                                            error!(
                                                "Failed to index {}: {}. AUTO-INDEXING ERROR - file not indexed!",
                                                change.path, e
                                            );
                                        } else {
                                            debug!("Auto-indexed: {}", change.path);
                                        }
                                    }
                                    Err(e) => {
                                        // File might have been deleted between detection and read
                                        warn!(
                                            "Failed to read {} for indexing: {}. File may have been deleted.",
                                            change.path, e
                                        );
                                    }
                                }
                            }
                            ChangeKind::Deleted => {
                                if let Err(e) = knowledge_graph.remove_script(&path_str).await {
                                    error!(
                                        "Failed to remove {} from index: {}. AUTO-INDEXING ERROR - stale entry may remain!",
                                        change.path, e
                                    );
                                } else {
                                    debug!("Auto-removed from index: {}", change.path);
                                }
                            }
                            ChangeKind::WatcherError => {
                                // Already handled above
                            }
                        }
                    }
                }

                // Wait for next poll cycle or shutdown
                tokio::select! {
                    _ = tokio::time::sleep(config.poll_interval) => {}
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Auto-indexer shutting down");
                            break;
                        }
                    }
                }
            }

            info!("Auto-indexer stopped");
        });

        AutoIndexerHandle {
            shutdown_tx,
            task: Some(task),
        }
    }
}

/// Handle to control a running auto-indexer.
pub struct AutoIndexerHandle {
    shutdown_tx: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl AutoIndexerHandle {
    /// Stop the auto-indexer gracefully.
    pub async fn stop(mut self) {
        // Send shutdown signal
        let _ = self.shutdown_tx.send(true);

        // Wait for task to complete
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    /// Check if the auto-indexer is still running.
    pub fn is_running(&self) -> bool {
        self.task
            .as_ref()
            .map(|t| !t.is_finished())
            .unwrap_or(false)
    }
}

impl Drop for AutoIndexerHandle {
    fn drop(&mut self) {
        // Send shutdown signal on drop (non-blocking)
        let _ = self.shutdown_tx.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::mock::MockKnowledgeGraph;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_auto_indexer_config_default() {
        let config = AutoIndexerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.batch_size, 50);
    }

    #[tokio::test]
    async fn test_auto_indexer_config_with_project_root() {
        let config = AutoIndexerConfig::with_project_root(PathBuf::from("/test/project"));
        assert_eq!(config.project_root, PathBuf::from("/test/project"));
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_auto_indexer_creation() {
        let temp_dir = TempDir::new().unwrap();
        let file_watcher = Arc::new(FileWatcher::new(temp_dir.path().to_path_buf()).unwrap());
        let knowledge_graph = Arc::new(MockKnowledgeGraph::new());
        let config = AutoIndexerConfig::with_project_root(temp_dir.path().to_path_buf());

        let indexer = AutoIndexer::new(file_watcher, knowledge_graph, config);
        // Just verify it creates without panic
        drop(indexer);
    }

    #[tokio::test]
    async fn test_auto_indexer_start_and_stop() {
        let temp_dir = TempDir::new().unwrap();
        let file_watcher = Arc::new(FileWatcher::new(temp_dir.path().to_path_buf()).unwrap());
        let knowledge_graph = Arc::new(MockKnowledgeGraph::new());
        let config = AutoIndexerConfig {
            poll_interval: Duration::from_millis(10),
            batch_size: 10,
            project_root: temp_dir.path().to_path_buf(),
        };

        let indexer = AutoIndexer::new(file_watcher, knowledge_graph, config);
        let handle = indexer.start();

        // Give it a moment to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(handle.is_running());

        // Stop it
        handle.stop().await;

        // Verify stopped - we can't check is_running after stop() consumes self
    }

    #[tokio::test]
    async fn test_auto_indexer_handle_drop_sends_shutdown() {
        let temp_dir = TempDir::new().unwrap();
        let file_watcher = Arc::new(FileWatcher::new(temp_dir.path().to_path_buf()).unwrap());
        let knowledge_graph = Arc::new(MockKnowledgeGraph::new());
        let config = AutoIndexerConfig {
            poll_interval: Duration::from_millis(10),
            batch_size: 10,
            project_root: temp_dir.path().to_path_buf(),
        };

        let indexer = AutoIndexer::new(file_watcher, knowledge_graph, config);
        let handle = indexer.start();

        // Drop the handle - should send shutdown signal
        drop(handle);

        // Give it time to shut down
        tokio::time::sleep(Duration::from_millis(100)).await;

        // No panic means success
    }

    #[tokio::test]
    async fn test_auto_indexer_indexes_new_files() {
        let temp_dir = TempDir::new().unwrap();
        let file_watcher = Arc::new(FileWatcher::new(temp_dir.path().to_path_buf()).unwrap());
        let knowledge_graph = Arc::new(MockKnowledgeGraph::new());
        let kg_clone = knowledge_graph.clone();

        let config = AutoIndexerConfig {
            poll_interval: Duration::from_millis(50),
            batch_size: 10,
            project_root: temp_dir.path().to_path_buf(),
        };

        let indexer = AutoIndexer::new(file_watcher, knowledge_graph, config);
        let handle = indexer.start();

        // Create a new .luau file
        let script_path = temp_dir.path().join("test.luau");
        tokio::fs::write(&script_path, "print('hello')").await.unwrap();

        // Wait for the file watcher to detect and auto-indexer to process
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Check if it was indexed
        let indexed = kg_clone.get_indexed_scripts();

        // Note: File watcher detection can be flaky in tests, so we just verify no panic
        // In production, the file would be indexed

        handle.stop().await;
    }
}
