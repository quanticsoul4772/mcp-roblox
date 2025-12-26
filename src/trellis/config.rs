//! TRELLIS RunPod configuration.

use secrecy::{ExposeSecret, Secret};

/// Configuration for TRELLIS via RunPod serverless.
pub struct TrellisConfig {
    /// RunPod API key
    pub api_key: Secret<String>,
    /// RunPod serverless endpoint ID
    pub endpoint_id: String,
    /// Base URL for RunPod API
    pub base_url: String,
    /// Maximum poll attempts for job completion
    pub max_poll_attempts: u32,
    /// Poll interval in milliseconds
    pub poll_interval_ms: u64,
}

impl TrellisConfig {
    /// Load configuration from environment variables.
    ///
    /// Required:
    /// - `RUNPOD_API_KEY`: RunPod API key
    /// - `TRELLIS_ENDPOINT_ID`: RunPod serverless endpoint ID
    ///
    /// Optional:
    /// - `RUNPOD_BASE_URL`: Base URL (default: https://api.runpod.ai/v2)
    /// - `TRELLIS_MAX_POLL_ATTEMPTS`: Max polls (default: 120 = 10 min at 5s intervals)
    /// - `TRELLIS_POLL_INTERVAL_MS`: Poll interval (default: 5000ms)
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("RUNPOD_API_KEY").ok()?;
        let endpoint_id = std::env::var("TRELLIS_ENDPOINT_ID").ok()?;

        Some(Self {
            api_key: Secret::new(api_key),
            endpoint_id,
            base_url: std::env::var("RUNPOD_BASE_URL")
                .unwrap_or_else(|_| "https://api.runpod.ai/v2".to_string()),
            max_poll_attempts: std::env::var("TRELLIS_MAX_POLL_ATTEMPTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(120), // 120 attempts * 5s = 10 minutes max
            poll_interval_ms: std::env::var("TRELLIS_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000), // 5 seconds
        })
    }

    /// Get the API key (exposed for HTTP headers).
    pub fn api_key(&self) -> &str {
        self.api_key.expose_secret()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::Secret;
    use serial_test::serial;
    use std::env;

    // Helper to clean up env vars
    fn cleanup_env() {
        env::remove_var("RUNPOD_API_KEY");
        env::remove_var("TRELLIS_ENDPOINT_ID");
        env::remove_var("RUNPOD_BASE_URL");
        env::remove_var("TRELLIS_MAX_POLL_ATTEMPTS");
        env::remove_var("TRELLIS_POLL_INTERVAL_MS");
    }

    #[test]
    #[serial]
    fn test_from_env_missing_api_key() {
        cleanup_env();
        assert!(TrellisConfig::from_env().is_none());
    }

    #[test]
    #[serial]
    fn test_from_env_missing_endpoint_id() {
        cleanup_env();
        env::set_var("RUNPOD_API_KEY", "test-key");
        let result = TrellisConfig::from_env();
        cleanup_env();
        assert!(result.is_none());
    }

    #[test]
    #[serial]
    fn test_from_env_success() {
        cleanup_env();
        env::set_var("RUNPOD_API_KEY", "test-api-key");
        env::set_var("TRELLIS_ENDPOINT_ID", "test-endpoint");

        let config = TrellisConfig::from_env().unwrap();
        assert_eq!(config.api_key(), "test-api-key");
        assert_eq!(config.endpoint_id, "test-endpoint");
        assert_eq!(config.base_url, "https://api.runpod.ai/v2");
        assert_eq!(config.max_poll_attempts, 120);
        assert_eq!(config.poll_interval_ms, 5000);

        cleanup_env();
    }

    #[test]
    #[serial]
    fn test_from_env_custom_values() {
        cleanup_env();
        env::set_var("RUNPOD_API_KEY", "custom-key");
        env::set_var("TRELLIS_ENDPOINT_ID", "custom-endpoint");
        env::set_var("RUNPOD_BASE_URL", "https://custom.api.com");
        env::set_var("TRELLIS_MAX_POLL_ATTEMPTS", "60");
        env::set_var("TRELLIS_POLL_INTERVAL_MS", "3000");

        let config = TrellisConfig::from_env().unwrap();
        assert_eq!(config.base_url, "https://custom.api.com");
        assert_eq!(config.max_poll_attempts, 60);
        assert_eq!(config.poll_interval_ms, 3000);

        cleanup_env();
    }

    #[test]
    #[serial]
    fn test_from_env_invalid_poll_attempts() {
        cleanup_env();
        env::set_var("RUNPOD_API_KEY", "key");
        env::set_var("TRELLIS_ENDPOINT_ID", "endpoint");
        env::set_var("TRELLIS_MAX_POLL_ATTEMPTS", "not-a-number");

        let config = TrellisConfig::from_env().unwrap();
        assert_eq!(config.max_poll_attempts, 120); // Falls back to default

        cleanup_env();
    }

    #[test]
    fn test_api_key_method() {
        let config = TrellisConfig {
            api_key: Secret::new("secret-key".to_string()),
            endpoint_id: "endpoint".to_string(),
            base_url: "https://api.runpod.ai/v2".to_string(),
            max_poll_attempts: 120,
            poll_interval_ms: 5000,
        };
        assert_eq!(config.api_key(), "secret-key");
    }
}
