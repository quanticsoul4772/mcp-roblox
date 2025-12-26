//! RunPod API client for TRELLIS text-to-3D generation.

use crate::http::HttpClient;
use crate::trellis::config::TrellisConfig;
use crate::trellis::glb_parser::GlbMesh;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// TRELLIS client errors.
#[derive(Debug)]
pub enum TrellisError {
    /// HTTP request failed
    Http(String),
    /// API returned an error
    Api { status: u16, message: String },
    /// Job failed during generation
    JobFailed(String),
    /// Job timed out waiting for completion
    Timeout,
    /// Failed to parse response
    ParseError(String),
    /// GLB parsing failed
    GlbParseError(String),
}

impl std::fmt::Display for TrellisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {}", e),
            Self::Api { status, message } => write!(f, "API error ({}): {}", status, message),
            Self::JobFailed(msg) => write!(f, "Job failed: {}", msg),
            Self::Timeout => write!(f, "Job timed out"),
            Self::ParseError(e) => write!(f, "Parse error: {}", e),
            Self::GlbParseError(e) => write!(f, "GLB parse error: {}", e),
        }
    }
}

impl std::error::Error for TrellisError {}

/// Request body for RunPod /run endpoint.
#[derive(Debug, Serialize)]
struct RunRequest<'a> {
    input: RunInput<'a>,
}

#[derive(Debug, Serialize)]
struct RunInput<'a> {
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simplify: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    texture_size: Option<u32>,
}

/// Response from RunPod /run endpoint.
#[derive(Debug, Deserialize)]
struct RunResponse {
    id: String,
    #[allow(dead_code)]
    status: String,
}

/// Response from RunPod /status endpoint.
#[derive(Debug, Deserialize)]
struct StatusResponse {
    #[allow(dead_code)]
    id: String,
    status: String,
    #[serde(default)]
    output: Option<JobOutput>,
    #[serde(default)]
    error: Option<String>,
}

/// Job output containing the generated mesh data.
#[derive(Debug, Deserialize)]
struct JobOutput {
    glb_base64: Option<String>,
    #[allow(dead_code)]
    vertex_count: Option<u32>,
    #[allow(dead_code)]
    face_count: Option<u32>,
    #[allow(dead_code)]
    generation_time_seconds: Option<f32>,
    error: Option<String>,
}

/// TRELLIS RunPod client.
pub struct TrellisClient<H: HttpClient> {
    config: TrellisConfig,
    http: Arc<H>,
}

impl<H: HttpClient> TrellisClient<H> {
    /// Create a new TRELLIS client.
    pub fn new(http: Arc<H>, config: TrellisConfig) -> Self {
        Self { config, http }
    }

    /// Generate a 3D mesh from a text prompt.
    ///
    /// This is a blocking operation that:
    /// 1. Submits a job to RunPod serverless
    /// 2. Polls for job completion
    /// 3. Decodes and parses the GLB mesh data
    pub async fn generate_mesh(&self, prompt: &str) -> Result<GlbMesh, TrellisError> {
        info!("Starting TRELLIS generation for prompt: {}", prompt);

        // Step 1: Submit job
        let job_id = self.submit_job(prompt).await?;
        info!("Job submitted: {}", job_id);

        // Step 2: Poll for completion
        let output = self.wait_for_completion(&job_id).await?;
        info!("Job completed");

        // Step 3: Decode and parse GLB
        let glb_base64 = output
            .glb_base64
            .ok_or_else(|| TrellisError::JobFailed("No GLB data in response".to_string()))?;

        let glb_bytes = BASE64
            .decode(&glb_base64)
            .map_err(|e| TrellisError::ParseError(format!("Base64 decode failed: {}", e)))?;

        let mesh = GlbMesh::from_bytes(&glb_bytes)
            .map_err(|e| TrellisError::GlbParseError(e.to_string()))?;

        info!(
            "GLB parsed: {} vertices, {} faces",
            mesh.vertex_count(),
            mesh.face_count()
        );

        Ok(mesh)
    }

    /// Submit a job to RunPod serverless.
    async fn submit_job(&self, prompt: &str) -> Result<String, TrellisError> {
        let url = format!("{}/{}/run", self.config.base_url, self.config.endpoint_id);

        let body = RunRequest {
            input: RunInput {
                prompt,
                seed: None,
                simplify: Some(0.95),
                texture_size: Some(1024),
            },
        };

        let body_json =
            serde_json::to_value(&body).map_err(|e| TrellisError::ParseError(e.to_string()))?;

        let auth_header = format!("Bearer {}", self.config.api_key());
        let response = self
            .http
            .post_json(&url, &[("Authorization", &auth_header)], body_json)
            .await
            .map_err(|e| TrellisError::Http(e.to_string()))?;

        if response.status != 200 && response.status != 201 {
            let msg = String::from_utf8_lossy(&response.body).to_string();
            return Err(TrellisError::Api {
                status: response.status,
                message: msg,
            });
        }

        let result: RunResponse = serde_json::from_slice(&response.body)
            .map_err(|e| TrellisError::ParseError(e.to_string()))?;

        Ok(result.id)
    }

    /// Wait for a job to complete.
    async fn wait_for_completion(&self, job_id: &str) -> Result<JobOutput, TrellisError> {
        let url = format!(
            "{}/{}/status/{}",
            self.config.base_url, self.config.endpoint_id, job_id
        );

        for attempt in 0..self.config.max_poll_attempts {
            let auth_header = format!("Bearer {}", self.config.api_key());
            let response = self
                .http
                .get(&url, &[("Authorization", &auth_header)])
                .await
                .map_err(|e| TrellisError::Http(e.to_string()))?;

            if response.status != 200 {
                let msg = String::from_utf8_lossy(&response.body).to_string();
                return Err(TrellisError::Api {
                    status: response.status,
                    message: msg,
                });
            }

            let status: StatusResponse = serde_json::from_slice(&response.body)
                .map_err(|e| TrellisError::ParseError(e.to_string()))?;

            debug!(
                "Job {} status: {} (attempt {})",
                job_id,
                status.status,
                attempt + 1
            );

            match status.status.as_str() {
                "COMPLETED" => {
                    let output = status
                        .output
                        .ok_or_else(|| TrellisError::JobFailed("No output in completed job".to_string()))?;

                    if let Some(error) = output.error {
                        return Err(TrellisError::JobFailed(error));
                    }

                    return Ok(output);
                }
                "FAILED" => {
                    let error_msg = status.error.unwrap_or_else(|| "Unknown error".to_string());
                    return Err(TrellisError::JobFailed(error_msg));
                }
                "IN_QUEUE" | "IN_PROGRESS" => {
                    // Continue polling
                }
                _ => {
                    warn!("Unknown job status: {}", status.status);
                }
            }

            tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
        }

        Err(TrellisError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::{MockHttpClient, MockResponse};
    use secrecy::Secret;

    fn test_config() -> TrellisConfig {
        TrellisConfig {
            api_key: Secret::new("test-key".to_string()),
            endpoint_id: "test-endpoint".to_string(),
            base_url: "https://api.runpod.ai/v2".to_string(),
            max_poll_attempts: 3,
            poll_interval_ms: 10,
        }
    }

    #[tokio::test]
    async fn test_submit_job_success() {
        let http = MockHttpClient::new();
        http.queue_response(MockResponse::success(
            200,
            br#"{"id": "job-123", "status": "IN_QUEUE"}"#.to_vec(),
        ));
        let client = TrellisClient::new(Arc::new(http), test_config());

        let result = client.submit_job("a wooden chest").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "job-123");
    }

    #[tokio::test]
    async fn test_submit_job_api_error() {
        let http = MockHttpClient::new();
        http.queue_response(MockResponse::success(
            401,
            br#"{"error": "Unauthorized"}"#.to_vec(),
        ));
        let client = TrellisClient::new(Arc::new(http), test_config());

        let result = client.submit_job("a wooden chest").await;
        assert!(matches!(result, Err(TrellisError::Api { status: 401, .. })));
    }

    #[tokio::test]
    async fn test_wait_for_completion_success() {
        let http = MockHttpClient::new();
        // First poll: IN_PROGRESS
        http.queue_response(MockResponse::success(
            200,
            br#"{"id": "job-123", "status": "IN_PROGRESS"}"#.to_vec(),
        ));
        // Second poll: COMPLETED
        http.queue_response(MockResponse::success(
            200,
            br#"{"id": "job-123", "status": "COMPLETED", "output": {"glb_base64": "dGVzdA==", "vertex_count": 100, "face_count": 50}}"#.to_vec(),
        ));
        let client = TrellisClient::new(Arc::new(http), test_config());

        let result = client.wait_for_completion("job-123").await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.glb_base64, Some("dGVzdA==".to_string()));
    }

    #[tokio::test]
    async fn test_wait_for_completion_failed() {
        let http = MockHttpClient::new();
        http.queue_response(MockResponse::success(
            200,
            br#"{"id": "job-123", "status": "FAILED", "error": "GPU OOM"}"#.to_vec(),
        ));
        let client = TrellisClient::new(Arc::new(http), test_config());

        let result = client.wait_for_completion("job-123").await;
        assert!(matches!(result, Err(TrellisError::JobFailed(msg)) if msg == "GPU OOM"));
    }

    #[tokio::test]
    async fn test_wait_for_completion_timeout() {
        let http = MockHttpClient::new();
        // All polls return IN_QUEUE
        for _ in 0..3 {
            http.queue_response(MockResponse::success(
                200,
                br#"{"id": "job-123", "status": "IN_QUEUE"}"#.to_vec(),
            ));
        }
        let client = TrellisClient::new(Arc::new(http), test_config());

        let result = client.wait_for_completion("job-123").await;
        assert!(matches!(result, Err(TrellisError::Timeout)));
    }

    #[test]
    fn test_trellis_error_display() {
        assert_eq!(
            TrellisError::Http("connection failed".to_string()).to_string(),
            "HTTP error: connection failed"
        );
        assert_eq!(
            TrellisError::Api {
                status: 401,
                message: "unauthorized".to_string()
            }
            .to_string(),
            "API error (401): unauthorized"
        );
        assert_eq!(
            TrellisError::JobFailed("GPU OOM".to_string()).to_string(),
            "Job failed: GPU OOM"
        );
        assert_eq!(TrellisError::Timeout.to_string(), "Job timed out");
        assert_eq!(
            TrellisError::ParseError("invalid json".to_string()).to_string(),
            "Parse error: invalid json"
        );
        assert_eq!(
            TrellisError::GlbParseError("invalid header".to_string()).to_string(),
            "GLB parse error: invalid header"
        );
    }
}
