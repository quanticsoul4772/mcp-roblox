//! Tool call instrumentation for metrics recording
//!
//! Provides a wrapper to automatically record tool execution metrics.

use crate::metrics::ServerMetrics;
use rmcp::ErrorData;
use std::sync::Arc;
use std::time::Instant;

#[cfg(debug_assertions)]
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

    #[tokio::test]
    async fn test_instrumented_call_dropped_without_finish() {
        let metrics = Arc::new(ServerMetrics::new());

        // Create and immediately drop without calling finish()
        {
            let _call = InstrumentedCall::start(metrics.clone(), "dropped_tool");
            // Drop happens here without finish() - triggers warning in debug builds
        }

        // Metrics should NOT be recorded since finish() was never called
        let snapshot = metrics.snapshot().await;
        // The tool should not appear in metrics (or have 0 calls if it does)
        assert!(
            !snapshot.tools.contains_key("dropped_tool")
                || snapshot.tools["dropped_tool"].calls == 0
        );
    }

    #[tokio::test]
    async fn test_instrumented_call_finish_with_ok() {
        let metrics = Arc::new(ServerMetrics::new());

        let call = InstrumentedCall::start(metrics.clone(), "ok_tool");
        let result: Result<String, ErrorData> = Ok("success".to_string());
        let returned = call.finish_with(result).await;

        assert!(returned.is_ok());
        assert_eq!(returned.unwrap(), "success");

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.tools["ok_tool"].calls, 1);
        assert_eq!(snapshot.tools["ok_tool"].errors, 0);
    }

    #[tokio::test]
    async fn test_instrumented_call_finish_with_err() {
        let metrics = Arc::new(ServerMetrics::new());

        let call = InstrumentedCall::start(metrics.clone(), "err_tool");
        let result: Result<String, ErrorData> = Err(ErrorData::internal_error("test error", None));
        let returned = call.finish_with(result).await;

        assert!(returned.is_err());

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.tools["err_tool"].calls, 1);
        assert_eq!(snapshot.tools["err_tool"].errors, 1);
    }

    #[tokio::test]
    async fn test_instrumented_call_multiple_calls_same_tool() {
        let metrics = Arc::new(ServerMetrics::new());

        // Make multiple calls to the same tool
        for i in 0..5 {
            let call = InstrumentedCall::start(metrics.clone(), "multi_tool");
            // Alternate success/failure
            call.finish(i % 2 == 0).await;
        }

        let snapshot = metrics.snapshot().await;
        assert_eq!(snapshot.tools["multi_tool"].calls, 5);
        // 3 successes (0, 2, 4), 2 failures (1, 3)
        assert_eq!(snapshot.tools["multi_tool"].errors, 2);
    }
}
