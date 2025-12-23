//! Embedding provider trait and Voyage AI implementation.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::RobloxMcpError;
use crate::http::HttpClient;

use super::config::VoyageConfig;

/// Trait for embedding providers (enables testing with mocks).
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embedding vector for a single text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RobloxMcpError>;

    /// Batch embed multiple texts (more efficient for bulk operations).
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RobloxMcpError>;

    /// Get the dimensionality of embeddings.
    fn dimensions(&self) -> usize;
}

/// Voyage AI embedding client.
///
/// Uses the voyage-code-3 model by default, which is optimized for code retrieval
/// and outperforms OpenAI embeddings by 13-17% on code benchmarks.
pub struct VoyageEmbedder<H: HttpClient> {
    http_client: Arc<H>,
    config: VoyageConfig,
}

impl<H: HttpClient> VoyageEmbedder<H> {
    const API_URL: &'static str = "https://api.voyageai.com/v1/embeddings";

    /// Create a new Voyage AI embedder.
    pub fn new(http_client: Arc<H>, config: VoyageConfig) -> Self {
        Self { http_client, config }
    }
}

/// Request body for Voyage AI embeddings API.
#[derive(Debug, Serialize)]
struct VoyageRequest<'a> {
    input: Vec<&'a str>,
    model: &'a str,
    input_type: &'a str,
    output_dimension: usize,
}

/// Response from Voyage AI embeddings API.
#[derive(Debug, Deserialize)]
struct VoyageResponse {
    data: Vec<EmbeddingData>,
    #[allow(dead_code)]
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    #[allow(dead_code)]
    total_tokens: usize,
}

#[async_trait]
impl<H: HttpClient + Send + Sync + 'static> EmbeddingProvider for VoyageEmbedder<H> {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, RobloxMcpError> {
        let results = self.embed_batch(&[text]).await?;
        results.into_iter().next().ok_or_else(|| {
            RobloxMcpError::CloudApiError("Voyage API returned empty response".into())
        })
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RobloxMcpError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Voyage API allows up to 128 texts per request
        const MAX_BATCH_SIZE: usize = 128;
        if texts.len() > MAX_BATCH_SIZE {
            return Err(RobloxMcpError::CloudApiError(format!(
                "Batch size {} exceeds maximum of {}",
                texts.len(),
                MAX_BATCH_SIZE
            )));
        }

        let request = VoyageRequest {
            input: texts.to_vec(),
            model: &self.config.model,
            input_type: "document",
            output_dimension: self.config.dimensions,
        };

        let body = serde_json::to_value(&request).map_err(|e| {
            RobloxMcpError::CloudApiError(format!("Failed to serialize request: {}", e))
        })?;

        let auth_header = format!("Bearer {}", self.config.api_key());

        let response = self
            .http_client
            .post_json(
                Self::API_URL,
                &[
                    ("Authorization", auth_header.as_str()),
                    ("Content-Type", "application/json"),
                ],
                body,
            )
            .await
            .map_err(|e| RobloxMcpError::CloudApiError(format!("Voyage API request failed: {}", e)))?;

        if !response.is_success() {
            let body_text = response.text().unwrap_or_else(|_| "<binary>".to_string());
            return Err(RobloxMcpError::CloudApiError(format!(
                "Voyage API error: {} - {}",
                response.status,
                body_text
            )));
        }

        let parsed: VoyageResponse = response.json().map_err(|e| {
            RobloxMcpError::CloudApiError(format!("Failed to parse Voyage response: {}", e))
        })?;

        // Sort by index to maintain input order
        let mut embeddings: Vec<_> = parsed.data.into_iter().collect();
        embeddings.sort_by_key(|e| e.index);

        Ok(embeddings.into_iter().map(|e| e.embedding).collect())
    }

    fn dimensions(&self) -> usize {
        self.config.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::{MockHttpClient, MockResponse};

    fn mock_voyage_response(embeddings: Vec<Vec<f32>>) -> serde_json::Value {
        let data: Vec<_> = embeddings
            .into_iter()
            .enumerate()
            .map(|(i, embedding)| {
                serde_json::json!({
                    "embedding": embedding,
                    "index": i
                })
            })
            .collect();

        serde_json::json!({
            "data": data,
            "usage": {
                "total_tokens": 100
            }
        })
    }

    #[tokio::test]
    async fn test_embed_single_text() {
        let mock_client = MockHttpClient::new();
        let expected_embedding = vec![0.1, 0.2, 0.3];
        mock_client.queue_response(MockResponse::json(
            200,
            mock_voyage_response(vec![expected_embedding.clone()]),
        ));

        let config = VoyageConfig {
            api_key: secrecy::Secret::new("test-key".to_string()),
            model: "voyage-code-3".to_string(),
            dimensions: 3,
        };

        let embedder = VoyageEmbedder::new(Arc::new(mock_client), config);
        let result = embedder.embed("test code").await.unwrap();

        assert_eq!(result, expected_embedding);
    }

    #[tokio::test]
    async fn test_embed_batch() {
        let mock_client = MockHttpClient::new();
        let embeddings = vec![vec![0.1, 0.2], vec![0.3, 0.4], vec![0.5, 0.6]];
        mock_client.queue_response(MockResponse::json(
            200,
            mock_voyage_response(embeddings.clone()),
        ));

        let config = VoyageConfig {
            api_key: secrecy::Secret::new("test-key".to_string()),
            model: "voyage-code-3".to_string(),
            dimensions: 2,
        };

        let embedder = VoyageEmbedder::new(Arc::new(mock_client), config);
        let result = embedder
            .embed_batch(&["code1", "code2", "code3"])
            .await
            .unwrap();

        assert_eq!(result, embeddings);
    }

    #[tokio::test]
    async fn test_embed_empty_batch() {
        let mock_client = MockHttpClient::new();
        let config = VoyageConfig {
            api_key: secrecy::Secret::new("test-key".to_string()),
            model: "voyage-code-3".to_string(),
            dimensions: 1024,
        };

        let embedder = VoyageEmbedder::new(Arc::new(mock_client), config);
        let result = embedder.embed_batch(&[]).await.unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_embed_api_error() {
        let mock_client = MockHttpClient::new();
        mock_client.queue_response(MockResponse::success(401, b"Unauthorized"));

        let config = VoyageConfig {
            api_key: secrecy::Secret::new("invalid-key".to_string()),
            model: "voyage-code-3".to_string(),
            dimensions: 1024,
        };

        let embedder = VoyageEmbedder::new(Arc::new(mock_client), config);
        let result = embedder.embed("test").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("401"));
    }
}
