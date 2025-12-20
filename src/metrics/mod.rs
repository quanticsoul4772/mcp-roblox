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

            let p95 = sorted.get(p95_idx.min(sorted.len() - 1)).copied().unwrap_or(0);
            let p99 = sorted.get(p99_idx.min(sorted.len() - 1)).copied().unwrap_or(0);

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

/// Server-wide metrics collector
#[derive(Debug)]
pub struct ServerMetrics {
    /// Metrics by tool name
    tools: RwLock<std::collections::HashMap<String, Arc<ToolMetrics>>>,
    /// Server start time
    started_at: std::time::Instant,
}

impl ServerMetrics {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(std::collections::HashMap::new()),
            started_at: std::time::Instant::now(),
        }
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
}
