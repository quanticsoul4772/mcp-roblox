//! Mock HTTP client for testing
//!
//! Provides a mock implementation of HttpClient that returns pre-configured responses.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::error::RobloxMcpError;
use super::{HttpClient, HttpResponse, MultipartForm};

/// Internal shared state for MockHttpClient
struct MockState {
    responses: VecDeque<MockResponse>,
    requests: Vec<MockRequest>,
}

/// Mock HTTP client for testing
///
/// Queues responses that are returned in FIFO order when requests are made.
/// If no responses are queued, returns an error.
///
/// Clone is cheap - all clones share the same internal state via Arc.
#[derive(Clone)]
pub struct MockHttpClient {
    state: Arc<Mutex<MockState>>,
}

/// A queued mock response
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub response: HttpResponse,
    /// Optional error to return instead of response
    pub error: Option<String>,
}

impl MockResponse {
    /// Create a successful response
    pub fn success(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            response: HttpResponse {
                status,
                headers: Default::default(),
                body: body.into(),
            },
            error: None,
        }
    }

    /// Create a successful JSON response
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self::success(status, serde_json::to_vec(&value).unwrap())
    }

    /// Create an error response
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            response: HttpResponse {
                status: 0,
                headers: Default::default(),
                body: vec![],
            },
            error: Some(message.into()),
        }
    }

    /// Add headers to the response
    pub fn with_headers(mut self, headers: impl IntoIterator<Item = (String, String)>) -> Self {
        self.response.headers.extend(headers);
        self
    }
}

/// Recorded request for verification
#[derive(Debug, Clone)]
pub struct MockRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl MockHttpClient {
    /// Create a new mock client
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                responses: VecDeque::new(),
                requests: Vec::new(),
            })),
        }
    }

    /// Queue a response to be returned on the next request
    pub fn queue_response(&self, response: MockResponse) {
        self.state.lock().unwrap().responses.push_back(response);
    }

    /// Queue multiple responses
    pub fn queue_responses(&self, responses: impl IntoIterator<Item = MockResponse>) {
        let mut state = self.state.lock().unwrap();
        for response in responses {
            state.responses.push_back(response);
        }
    }

    /// Get all recorded requests
    pub fn requests(&self) -> Vec<MockRequest> {
        self.state.lock().unwrap().requests.clone()
    }

    /// Get the last recorded request
    pub fn last_request(&self) -> Option<MockRequest> {
        self.state.lock().unwrap().requests.last().cloned()
    }

    /// Clear recorded requests
    pub fn clear_requests(&self) {
        self.state.lock().unwrap().requests.clear();
    }

    /// Get next queued response or error
    fn next_response(&self) -> Result<HttpResponse, RobloxMcpError> {
        let mock_response = self
            .state
            .lock()
            .unwrap()
            .responses
            .pop_front()
            .ok_or_else(|| {
                RobloxMcpError::ConfigError("MockHttpClient: No response queued".into())
            })?;

        if let Some(error) = mock_response.error {
            return Err(RobloxMcpError::HttpConnectionError(error));
        }

        Ok(mock_response.response)
    }

    /// Record a request
    fn record_request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) {
        self.state.lock().unwrap().requests.push(MockRequest {
            method: method.to_string(),
            url: url.to_string(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body,
        });
    }
}

impl Default for MockHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MockHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap();
        f.debug_struct("MockHttpClient")
            .field("queued_responses", &state.responses.len())
            .field("recorded_requests", &state.requests.len())
            .finish()
    }
}

#[async_trait]
impl HttpClient for MockHttpClient {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, RobloxMcpError> {
        self.record_request("GET", url, headers, None);
        self.next_response()
    }

    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<HttpResponse, RobloxMcpError> {
        self.record_request("POST", url, headers, Some(serde_json::to_vec(&body).unwrap()));
        self.next_response()
    }

    async fn post_binary(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
        _query: Option<&[(&str, &str)]>,
    ) -> Result<HttpResponse, RobloxMcpError> {
        self.record_request("POST", url, headers, Some(body));
        self.next_response()
    }

    async fn post_multipart(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        _form: MultipartForm,
    ) -> Result<HttpResponse, RobloxMcpError> {
        self.record_request("POST_MULTIPART", url, headers, None);
        self.next_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_client_returns_queued_response() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(200, b"hello"));

        let response = mock.get("http://test.com", &[]).await.unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"hello");
    }

    #[tokio::test]
    async fn test_mock_client_returns_json_response() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"key": "value"})));

        let response = mock.get("http://test.com", &[]).await.unwrap();
        let parsed: serde_json::Value = response.json().unwrap();

        assert_eq!(parsed["key"], "value");
    }

    #[tokio::test]
    async fn test_mock_client_returns_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Connection refused"));

        let result = mock.get("http://test.com", &[]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_client_records_requests() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(200, b"ok"));

        mock.get("http://test.com/api", &[("Authorization", "Bearer token")]).await.unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].url, "http://test.com/api");
        assert!(requests[0].headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer token"));
    }

    #[tokio::test]
    async fn test_mock_client_fifo_order() {
        let mock = MockHttpClient::new();
        mock.queue_responses([
            MockResponse::success(200, b"first"),
            MockResponse::success(201, b"second"),
        ]);

        let r1 = mock.get("http://test.com", &[]).await.unwrap();
        let r2 = mock.get("http://test.com", &[]).await.unwrap();

        assert_eq!(r1.body, b"first");
        assert_eq!(r2.body, b"second");
    }

    #[tokio::test]
    async fn test_mock_client_no_response_queued() {
        let mock = MockHttpClient::new();

        let result = mock.get("http://test.com", &[]).await;

        assert!(result.is_err());
    }

    #[test]
    fn test_mock_response_with_headers() {
        let response = MockResponse::success(200, b"body")
            .with_headers([("Content-Type".to_string(), "application/json".to_string())]);

        assert_eq!(response.response.headers.get("Content-Type"), Some(&"application/json".to_string()));
    }

    #[tokio::test]
    async fn test_mock_client_clone_shares_state() {
        let mock1 = MockHttpClient::new();
        mock1.queue_response(MockResponse::success(200, b"shared"));

        let mock2 = mock1.clone();

        // Use mock2 to make the request
        mock2.get("http://test.com/shared", &[]).await.unwrap();

        // Verify that mock1 can see the recorded request (shared state)
        let requests = mock1.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "http://test.com/shared");
    }
}
