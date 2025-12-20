//! Tool call instrumentation for metrics recording
//!
//! Provides a wrapper to automatically record tool execution metrics.

use crate::metrics::ServerMetrics;
use rmcp::ErrorData;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

/// RAII guard for instrumenting tool calls
///
/// Records tool execution duration and success/failure on drop.
pub struct InstrumentedCall {
    metrics: Arc<ServerMetrics>,
    tool_name: String,
    start: Instant,
    finished: bool,
}

impl InstrumentedCall {
    /// Start instrumenting a tool call
    pub fn start(metrics: Arc<ServerMetrics>, tool_name: impl Into<String>) -> Self {
        Self {
            metrics,
            tool_name: tool_name.into(),
            start: Instant::now(),
            finished: false,
        }
    }

    /// Finish the instrumented call and record metrics
    ///
    /// Must be called explicitly to record success/failure.
    /// If dropped without calling finish(), assumes failure.
    pub async fn finish(mut self, success: bool) {
        self.finished = true;
        let duration = self.start.elapsed();
        let tool_metrics = self.metrics.get_tool(&self.tool_name).await;

        tool_metrics.record_call(duration).await;

        if !success {
            tool_metrics.record_error();
        }
    }

    /// Finish and return the result, recording success/failure based on Result type
    ///
    /// This is a convenience method for the common pattern of instrumenting tool calls.
    pub async fn finish_with<T>(self, result: Result<T, ErrorData>) -> Result<T, ErrorData> {
        self.finish(result.is_ok()).await;
        result
    }
}

impl Drop for InstrumentedCall {
    fn drop(&mut self) {
        // If finish() wasn't called, we can't record async metrics
        // This is a design tradeoff - we rely on explicit finish() calls
        if !self.finished {
            // Log warning in debug builds using tracing (appears in structured logs)
            #[cfg(debug_assertions)]
            warn!(
                tool_name = %self.tool_name,
                "InstrumentedCall dropped without finish() - metrics not recorded"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_instrumented_call_success() {
        let metrics = Arc::new(ServerMetrics::new());

        let call = InstrumentedCall::start(metrics.clone(), "test_tool");
        call.finish(true).await;

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.tools["test_tool"].calls, 1);
        assert_eq!(snapshot.tools["test_tool"].errors, 0);
    }

    #[tokio::test]
    async fn test_instrumented_call_failure() {
        let metrics = Arc::new(ServerMetrics::new());

        let call = InstrumentedCall::start(metrics.clone(), "test_tool");
        call.finish(false).await;

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.tools["test_tool"].calls, 1);
        assert_eq!(snapshot.tools["test_tool"].errors, 1);
    }

    #[tokio::test]
    async fn test_instrumented_call_duration() {
        let metrics = Arc::new(ServerMetrics::new());

        let call = InstrumentedCall::start(metrics.clone(), "test_tool");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        call.finish(true).await;

        let snapshot = metrics.snapshot().await;
        // Duration should be at least 10ms
        assert!(snapshot.tools["test_tool"].avg_duration_ms >= 10.0);
    }
}
