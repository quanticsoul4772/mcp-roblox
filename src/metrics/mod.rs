//! Server metrics and health monitoring
//!
//! Tracks tool execution counts, durations, and error rates.
//! Uses bounded VecDeque (MAX_DURATION_SAMPLES) to prevent unbounded memory growth.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Maximum duration samples to keep per tool (prevents unbounded growth)
const MAX_DURATION_SAMPLES: usize = 1000;

/// Metrics for a single tool
#[derive(Debug, Default)]
pub struct ToolMetrics {
    pub calls: AtomicU64,
    pub errors: AtomicU64,
    /// Bounded ring buffer for duration samples (in milliseconds)
    durations_ms: RwLock<VecDeque<u64>>,
}

impl ToolMetrics {
    pub fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            durations_ms: RwLock::new(VecDeque::with_capacity(MAX_DURATION_SAMPLES)),
        }
    }

    /// Record a successful call with duration
    pub async fn record_call(&self, duration: Duration) {
        self.calls.fetch_add(1, Ordering::Relaxed);

        let mut durations = self.durations_ms.write().await;
        if durations.len() >= MAX_DURATION_SAMPLES {
            durations.pop_front();
        }
        durations.push_back(duration.as_millis() as u64);
    }

    /// Record an error
    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Get statistics snapshot
    pub async fn snapshot(&self) -> ToolMetricsSnapshot {
        let calls = self.calls.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let durations = self.durations_ms.read().await;

        let (avg_ms, p95_ms, p99_ms) = if durations.is_empty() {
            (0.0, 0, 0)
        } else {
            let mut sorted: Vec<u64> = durations.iter().copied().collect();
            sorted.sort_unstable();

            let sum: u64 = sorted.iter().sum();
            let avg = sum as f64 / sorted.len() as f64;

            let p95_idx = (sorted.len() as f64 * 0.95) as usize;
            let p99_idx = (sorted.len() as f64 * 0.99) as usize;

            let p95 = sorted
                .get(p95_idx.min(sorted.len() - 1))
                .copied()
                .unwrap_or(0);
            let p99 = sorted
                .get(p99_idx.min(sorted.len() - 1))
                .copied()
                .unwrap_or(0);

            (avg, p95, p99)
        };

        ToolMetricsSnapshot {
            calls,
            errors,
            error_rate: if calls > 0 {
                errors as f64 / calls as f64
            } else {
                0.0
            },
            avg_duration_ms: avg_ms,
            p95_duration_ms: p95_ms,
            p99_duration_ms: p99_ms,
        }
    }
}

/// Serializable metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct ToolMetricsSnapshot {
    pub calls: u64,
    pub errors: u64,
    pub error_rate: f64,
    pub avg_duration_ms: f64,
    pub p95_duration_ms: u64,
    pub p99_duration_ms: u64,
}

/// Metrics for late plugin results (results arriving after caller timeout)
#[derive(Debug, Default)]
pub struct LateResultMetrics {
    /// Total late results received
    pub total: AtomicU64,
    /// Late results that were successful (plugin did work that went unused)
    pub successful: AtomicU64,
    /// Late results that were errors
    pub errors: AtomicU64,
}

impl LateResultMetrics {
    pub fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            successful: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    /// Record a late result (result arrived after caller timed out)
    pub fn record(&self, had_error: bool) {
        self.total.fetch_add(1, Ordering::Relaxed);
        if had_error {
            self.errors.fetch_add(1, Ordering::Relaxed);
        } else {
            self.successful.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get late result metrics snapshot
    pub fn snapshot(&self) -> LateResultMetricsSnapshot {
        LateResultMetricsSnapshot {
            total: self.total.load(Ordering::Relaxed),
            successful: self.successful.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

/// Serializable late result metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct LateResultMetricsSnapshot {
    /// Total late results received
    pub total: u64,
    /// Late results that were successful (wasted work)
    pub successful: u64,
    /// Late results that were errors
    pub errors: u64,
}

/// Metrics for unknown command results (plugin sends result for unknown command ID)
///
/// This tracks an anomaly where the plugin returns a result for a command ID
/// that isn't registered. This could indicate:
/// - A bug in command tracking
/// - Plugin sending duplicate results
/// - Command ID collision (unlikely with UUID)
#[derive(Debug, Default)]
pub struct UnknownCommandMetrics {
    /// Total unknown command results received
    pub total: AtomicU64,
}

impl UnknownCommandMetrics {
    pub fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
        }
    }

    /// Record an unknown command result
    pub fn record(&self) {
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    /// Get unknown command metrics snapshot
    pub fn snapshot(&self) -> UnknownCommandMetricsSnapshot {
        UnknownCommandMetricsSnapshot {
            total: self.total.load(Ordering::Relaxed),
        }
    }
}

/// Serializable unknown command metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct UnknownCommandMetricsSnapshot {
    /// Total unknown command results received
    pub total: u64,
}

/// Connection status tracking
#[derive(Debug, Default)]
pub struct ConnectionMetrics {
    /// Total connection checks performed
    pub checks: AtomicU64,
    /// Number of times connection was found connected
    pub connected_checks: AtomicU64,
    /// Number of times connection was found disconnected
    pub disconnected_checks: AtomicU64,
    /// Last known connection status
    pub last_connected: std::sync::atomic::AtomicBool,
}

impl ConnectionMetrics {
    pub fn new() -> Self {
        Self {
            checks: AtomicU64::new(0),
            connected_checks: AtomicU64::new(0),
            disconnected_checks: AtomicU64::new(0),
            last_connected: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Record a connection status check
    pub fn record_status(&self, connected: bool) {
        self.checks.fetch_add(1, Ordering::Relaxed);
        if connected {
            self.connected_checks.fetch_add(1, Ordering::Relaxed);
        } else {
            self.disconnected_checks.fetch_add(1, Ordering::Relaxed);
        }
        self.last_connected.store(connected, Ordering::Relaxed);
    }

    /// Get connection metrics snapshot
    pub fn snapshot(&self) -> ConnectionMetricsSnapshot {
        let checks = self.checks.load(Ordering::Relaxed);
        let connected = self.connected_checks.load(Ordering::Relaxed);
        let disconnected = self.disconnected_checks.load(Ordering::Relaxed);

        ConnectionMetricsSnapshot {
            total_checks: checks,
            connected_checks: connected,
            disconnected_checks: disconnected,
            uptime_ratio: if checks > 0 {
                connected as f64 / checks as f64
            } else {
                0.0
            },
            last_connected: self.last_connected.load(Ordering::Relaxed),
        }
    }
}

/// Serializable connection metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionMetricsSnapshot {
    pub total_checks: u64,
    pub connected_checks: u64,
    pub disconnected_checks: u64,
    pub uptime_ratio: f64,
    pub last_connected: bool,
}

/// Server-wide metrics collector
#[derive(Debug)]
pub struct ServerMetrics {
    /// Metrics by tool name
    tools: RwLock<std::collections::HashMap<String, Arc<ToolMetrics>>>,
    /// Server start time
    started_at: std::time::Instant,
    /// Studio connection metrics
    connection: ConnectionMetrics,
    /// Late plugin result metrics (results arriving after caller timeout)
    late_results: LateResultMetrics,
    /// Unknown command metrics (plugin sends result for unregistered command ID)
    unknown_commands: UnknownCommandMetrics,
}

impl ServerMetrics {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(std::collections::HashMap::new()),
            started_at: std::time::Instant::now(),
            connection: ConnectionMetrics::new(),
            late_results: LateResultMetrics::new(),
            unknown_commands: UnknownCommandMetrics::new(),
        }
    }

    /// Record Studio connection status
    pub fn record_connection_status(&self, connected: bool) {
        self.connection.record_status(connected);
    }

    /// Get connection metrics snapshot
    pub fn connection_snapshot(&self) -> ConnectionMetricsSnapshot {
        self.connection.snapshot()
    }

    /// Record a late plugin result (result arrived after caller timed out)
    pub fn record_late_result(&self, had_error: bool) {
        self.late_results.record(had_error);
    }

    /// Get late result metrics snapshot
    ///
    /// Used for targeted monitoring of late results without full server metrics.
    /// For comprehensive metrics, use `snapshot()` which includes late_results.
    #[allow(dead_code)]
    pub fn late_results_snapshot(&self) -> LateResultMetricsSnapshot {
        self.late_results.snapshot()
    }

    /// Record an unknown command result (plugin sent result for unregistered command ID)
    ///
    /// This indicates a potential bug in command tracking or duplicate plugin responses.
    pub fn record_unknown_command(&self) {
        self.unknown_commands.record();
    }

    /// Get unknown command metrics snapshot
    ///
    /// Used for targeted monitoring of unknown commands without full server metrics.
    /// For comprehensive metrics, use `snapshot()` which includes unknown_commands.
    #[allow(dead_code)]
    pub fn unknown_commands_snapshot(&self) -> UnknownCommandMetricsSnapshot {
        self.unknown_commands.snapshot()
    }

    /// Get or create metrics for a tool
    pub async fn get_tool(&self, name: &str) -> Arc<ToolMetrics> {
        // Try read lock first
        {
            let tools = self.tools.read().await;
            if let Some(metrics) = tools.get(name) {
                return metrics.clone();
            }
        }

        // Need to create - take write lock
        let mut tools = self.tools.write().await;
        tools
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(ToolMetrics::new()))
            .clone()
    }

    /// Get full metrics snapshot
    pub async fn snapshot(&self) -> ServerMetricsSnapshot {
        let tools = self.tools.read().await;
        let mut tool_snapshots = std::collections::HashMap::new();

        for (name, metrics) in tools.iter() {
            tool_snapshots.insert(name.clone(), metrics.snapshot().await);
        }

        ServerMetricsSnapshot {
            uptime_secs: self.started_at.elapsed().as_secs(),
            tools: tool_snapshots,
            connection: self.connection.snapshot(),
            late_results: self.late_results.snapshot(),
            unknown_commands: self.unknown_commands.snapshot(),
        }
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Full server metrics snapshot
#[derive(Debug, Clone, Serialize)]
pub struct ServerMetricsSnapshot {
    pub uptime_secs: u64,
    pub tools: std::collections::HashMap<String, ToolMetricsSnapshot>,
    pub connection: ConnectionMetricsSnapshot,
    /// Late plugin results (results arriving after caller timeout)
    pub late_results: LateResultMetricsSnapshot,
    /// Unknown command results (plugin sent result for unregistered command ID)
    pub unknown_commands: UnknownCommandMetricsSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tool_metrics_record_call() {
        let metrics = ToolMetrics::new();

        metrics.record_call(Duration::from_millis(100)).await;
        metrics.record_call(Duration::from_millis(200)).await;

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.calls, 2);
        assert_eq!(snapshot.errors, 0);
        assert!((snapshot.avg_duration_ms - 150.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_tool_metrics_record_error() {
        let metrics = ToolMetrics::new();

        metrics.record_call(Duration::from_millis(100)).await;
        metrics.record_error();

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.calls, 1);
        assert_eq!(snapshot.errors, 1);
        assert!((snapshot.error_rate - 1.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_tool_metrics_bounded_queue() {
        let metrics = ToolMetrics::new();

        // Add more than MAX_DURATION_SAMPLES
        for i in 0..(MAX_DURATION_SAMPLES + 100) {
            metrics.record_call(Duration::from_millis(i as u64)).await;
        }

        let durations = metrics.durations_ms.read().await;
        assert_eq!(durations.len(), MAX_DURATION_SAMPLES);
    }

    #[tokio::test]
    async fn test_server_metrics_get_tool() {
        let server_metrics = ServerMetrics::new();

        let tool1 = server_metrics.get_tool("fs_read_script").await;
        let tool1_again = server_metrics.get_tool("fs_read_script").await;

        // Should return same Arc
        assert!(Arc::ptr_eq(&tool1, &tool1_again));
    }

    #[tokio::test]
    async fn test_server_metrics_snapshot() {
        let server_metrics = ServerMetrics::new();

        let tool = server_metrics.get_tool("test_tool").await;
        tool.record_call(Duration::from_millis(50)).await;

        let snapshot = server_metrics.snapshot().await;
        // uptime_secs is u64, just verify snapshot was created successfully
        assert!(snapshot.tools.contains_key("test_tool"));
        assert_eq!(snapshot.tools["test_tool"].calls, 1);
    }

    #[test]
    fn test_tool_metrics_snapshot_empty() {
        let metrics = ToolMetrics::new();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let snapshot = rt.block_on(metrics.snapshot());
        assert_eq!(snapshot.calls, 0);
        assert_eq!(snapshot.errors, 0);
        assert_eq!(snapshot.error_rate, 0.0);
        assert_eq!(snapshot.avg_duration_ms, 0.0);
    }

    // === CONNECTION METRICS TESTS ===

    #[test]
    fn test_connection_metrics_new() {
        let metrics = ConnectionMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.total_checks, 0);
        assert_eq!(snapshot.connected_checks, 0);
        assert_eq!(snapshot.disconnected_checks, 0);
        assert_eq!(snapshot.uptime_ratio, 0.0);
        assert!(!snapshot.last_connected);
    }

    #[test]
    fn test_connection_metrics_record_connected() {
        let metrics = ConnectionMetrics::new();

        metrics.record_status(true);
        metrics.record_status(true);
        metrics.record_status(true);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_checks, 3);
        assert_eq!(snapshot.connected_checks, 3);
        assert_eq!(snapshot.disconnected_checks, 0);
        assert!((snapshot.uptime_ratio - 1.0).abs() < 0.001);
        assert!(snapshot.last_connected);
    }

    #[test]
    fn test_connection_metrics_record_disconnected() {
        let metrics = ConnectionMetrics::new();

        metrics.record_status(false);
        metrics.record_status(false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_checks, 2);
        assert_eq!(snapshot.connected_checks, 0);
        assert_eq!(snapshot.disconnected_checks, 2);
        assert_eq!(snapshot.uptime_ratio, 0.0);
        assert!(!snapshot.last_connected);
    }

    #[test]
    fn test_connection_metrics_mixed_status() {
        let metrics = ConnectionMetrics::new();

        // 3 connected, 2 disconnected = 60% uptime
        metrics.record_status(true);
        metrics.record_status(true);
        metrics.record_status(false);
        metrics.record_status(true);
        metrics.record_status(false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total_checks, 5);
        assert_eq!(snapshot.connected_checks, 3);
        assert_eq!(snapshot.disconnected_checks, 2);
        assert!((snapshot.uptime_ratio - 0.6).abs() < 0.001);
        assert!(!snapshot.last_connected); // Last was disconnected
    }

    #[test]
    fn test_connection_metrics_last_status_tracking() {
        let metrics = ConnectionMetrics::new();

        metrics.record_status(true);
        assert!(metrics.snapshot().last_connected);

        metrics.record_status(false);
        assert!(!metrics.snapshot().last_connected);

        metrics.record_status(true);
        assert!(metrics.snapshot().last_connected);
    }

    #[test]
    fn test_server_metrics_connection_tracking() {
        let server_metrics = ServerMetrics::new();

        server_metrics.record_connection_status(true);
        server_metrics.record_connection_status(true);
        server_metrics.record_connection_status(false);

        let snapshot = server_metrics.connection_snapshot();
        assert_eq!(snapshot.total_checks, 3);
        assert_eq!(snapshot.connected_checks, 2);
        assert_eq!(snapshot.disconnected_checks, 1);
    }

    #[tokio::test]
    async fn test_server_metrics_snapshot_includes_connection() {
        let server_metrics = ServerMetrics::new();

        server_metrics.record_connection_status(true);

        let snapshot = server_metrics.snapshot().await;
        assert_eq!(snapshot.connection.total_checks, 1);
        assert!(snapshot.connection.last_connected);
    }

    // === LATE RESULT METRICS TESTS ===

    #[test]
    fn test_late_result_metrics_new() {
        let metrics = LateResultMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.total, 0);
        assert_eq!(snapshot.successful, 0);
        assert_eq!(snapshot.errors, 0);
    }

    #[test]
    fn test_late_result_metrics_record_successful() {
        let metrics = LateResultMetrics::new();

        metrics.record(false); // successful late result
        metrics.record(false);
        metrics.record(false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total, 3);
        assert_eq!(snapshot.successful, 3);
        assert_eq!(snapshot.errors, 0);
    }

    #[test]
    fn test_late_result_metrics_record_errors() {
        let metrics = LateResultMetrics::new();

        metrics.record(true); // late result with error
        metrics.record(true);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.successful, 0);
        assert_eq!(snapshot.errors, 2);
    }

    #[test]
    fn test_late_result_metrics_mixed() {
        let metrics = LateResultMetrics::new();

        // 3 successful, 2 errors
        metrics.record(false);
        metrics.record(true);
        metrics.record(false);
        metrics.record(true);
        metrics.record(false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total, 5);
        assert_eq!(snapshot.successful, 3);
        assert_eq!(snapshot.errors, 2);
    }

    #[test]
    fn test_server_metrics_late_result_tracking() {
        let server_metrics = ServerMetrics::new();

        server_metrics.record_late_result(false); // successful
        server_metrics.record_late_result(true); // error
        server_metrics.record_late_result(false); // successful

        let snapshot = server_metrics.late_results_snapshot();
        assert_eq!(snapshot.total, 3);
        assert_eq!(snapshot.successful, 2);
        assert_eq!(snapshot.errors, 1);
    }

    #[tokio::test]
    async fn test_server_metrics_snapshot_includes_late_results() {
        let server_metrics = ServerMetrics::new();

        server_metrics.record_late_result(false);
        server_metrics.record_late_result(true);

        let snapshot = server_metrics.snapshot().await;
        assert_eq!(snapshot.late_results.total, 2);
        assert_eq!(snapshot.late_results.successful, 1);
        assert_eq!(snapshot.late_results.errors, 1);
    }

    // === UNKNOWN COMMAND METRICS TESTS ===

    #[test]
    fn test_unknown_command_metrics_new() {
        let metrics = UnknownCommandMetrics::new();
        let snapshot = metrics.snapshot();

        assert_eq!(snapshot.total, 0);
    }

    #[test]
    fn test_unknown_command_metrics_record() {
        let metrics = UnknownCommandMetrics::new();

        metrics.record();
        metrics.record();
        metrics.record();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.total, 3);
    }

    #[test]
    fn test_server_metrics_unknown_command_tracking() {
        let server_metrics = ServerMetrics::new();

        server_metrics.record_unknown_command();
        server_metrics.record_unknown_command();

        let snapshot = server_metrics.unknown_commands_snapshot();
        assert_eq!(snapshot.total, 2);
    }

    #[tokio::test]
    async fn test_server_metrics_snapshot_includes_unknown_commands() {
        let server_metrics = ServerMetrics::new();

        server_metrics.record_unknown_command();
        server_metrics.record_unknown_command();
        server_metrics.record_unknown_command();

        let snapshot = server_metrics.snapshot().await;
        assert_eq!(snapshot.unknown_commands.total, 3);
    }

    // === DEFAULT TRAIT TESTS ===

    #[test]
    fn test_server_metrics_default() {
        // Test the Default trait implementation for ServerMetrics
        let metrics: ServerMetrics = Default::default();

        // Default should be same as new()
        let snapshot = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(metrics.snapshot());

        // Should have empty tools
        assert!(snapshot.tools.is_empty());
        // Should have zero connection checks
        assert_eq!(snapshot.connection.total_checks, 0);
        // Should have zero late results
        assert_eq!(snapshot.late_results.total, 0);
        // Should have zero unknown commands
        assert_eq!(snapshot.unknown_commands.total, 0);
    }

    #[test]
    fn test_server_metrics_default_equals_new() {
        let from_default: ServerMetrics = Default::default();
        let from_new = ServerMetrics::new();

        // Both should have zero totals
        let snapshot_default = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(from_default.snapshot());
        let snapshot_new = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(from_new.snapshot());

        assert_eq!(snapshot_default.tools.len(), snapshot_new.tools.len());
        assert_eq!(
            snapshot_default.connection.total_checks,
            snapshot_new.connection.total_checks
        );
    }

    // === ADDITIONAL COVERAGE TESTS ===

    #[tokio::test]
    async fn test_tool_metrics_single_call() {
        let metrics = ToolMetrics::new();

        metrics.record_call(Duration::from_millis(42)).await;

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.calls, 1);
        assert_eq!(snapshot.errors, 0);
        // With a single sample, avg should equal that sample
        assert!((snapshot.avg_duration_ms - 42.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_tool_metrics_multiple_errors() {
        let metrics = ToolMetrics::new();

        metrics.record_error();
        metrics.record_error();
        metrics.record_error();

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.calls, 0);
        assert_eq!(snapshot.errors, 3);
        // Error rate should be infinite/NaN or 0 with no calls - actually it's 0.0 division check
        // With 0 calls, error_rate calculation might be 0 or needs special handling
        // The implementation divides by calls, so with 0 calls it should be 0
        assert_eq!(snapshot.error_rate, 0.0);
    }

    #[test]
    fn test_server_metrics_snapshot_debug() {
        let server_metrics = ServerMetrics::new();
        server_metrics.record_connection_status(true);

        let snapshot = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(server_metrics.snapshot());

        let debug_str = format!("{:?}", snapshot);
        assert!(debug_str.contains("ServerMetricsSnapshot"));
        assert!(debug_str.contains("uptime_secs"));
        assert!(debug_str.contains("tools"));
    }

    #[test]
    fn test_server_metrics_snapshot_clone() {
        let server_metrics = ServerMetrics::new();
        server_metrics.record_connection_status(true);
        server_metrics.record_late_result(false);

        let snapshot = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(server_metrics.snapshot());

        let cloned = snapshot.clone();
        assert_eq!(cloned.connection.total_checks, 1);
        assert_eq!(cloned.late_results.total, 1);
    }

    #[test]
    fn test_server_metrics_snapshot_serialize() {
        let server_metrics = ServerMetrics::new();
        server_metrics.record_connection_status(true);

        let snapshot = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(server_metrics.snapshot());

        let json = serde_json::to_string(&snapshot).expect("Should serialize");
        assert!(json.contains("uptime_secs"));
        assert!(json.contains("connection"));
        assert!(json.contains("late_results"));
    }
}
