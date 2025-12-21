//! Production HTTP client implementation using reqwest

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use super::{FormContent, HttpClient, HttpResponse, MultipartForm};
use crate::error::RobloxMcpError;

/// Production HTTP client using reqwest with connection pooling
pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    /// Create a new HTTP client with sensible defaults for API usage
    pub fn new() -> Result<Self, RobloxMcpError> {
        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(5)
            .pool_idle_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(RobloxMcpError::from_reqwest)?;

        Ok(Self { client })
    }

    /// Convert reqwest response to our HttpResponse type
    async fn convert_response(response: reqwest::Response) -> Result<HttpResponse, RobloxMcpError> {
        let status = response.status().as_u16();

        let mut headers = HashMap::new();
        for (name, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                headers.insert(name.to_string(), v.to_string());
            }
        }

        let body = response
            .bytes()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

impl std::fmt::Debug for ReqwestHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestHttpClient")
            .field("client", &"<reqwest::Client>")
            .finish()
    }
}

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, RobloxMcpError> {
        let mut req = self.client.get(url);

        for (name, value) in headers {
            req = req.header(*name, *value);
        }

        let response = req.send().await.map_err(RobloxMcpError::from_reqwest)?;
        Self::convert_response(response).await
    }

    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<HttpResponse, RobloxMcpError> {
        let mut req = self.client.post(url);

        for (name, value) in headers {
            req = req.header(*name, *value);
        }

        let response = req
            .json(&body)
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        Self::convert_response(response).await
    }

    async fn post_binary(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
        query: Option<&[(&str, &str)]>,
    ) -> Result<HttpResponse, RobloxMcpError> {
        let mut req = self.client.post(url);

        for (name, value) in headers {
            req = req.header(*name, *value);
        }

        if let Some(params) = query {
            req = req.query(params);
        }

        let response = req
            .body(body)
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        Self::convert_response(response).await
    }

    async fn post_multipart(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        form: MultipartForm,
    ) -> Result<HttpResponse, RobloxMcpError> {
        let mut req_form = reqwest::multipart::Form::new();

        for part in form.parts {
            match part.content {
                FormContent::Text(text) => {
                    req_form = req_form.text(part.name, text);
                }
                FormContent::File {
                    filename,
                    content_type,
                    data,
                } => {
                    let file_part = reqwest::multipart::Part::bytes(data)
                        .file_name(filename)
                        .mime_str(&content_type)
                        .map_err(|e| RobloxMcpError::ConfigError(e.to_string()))?;
                    req_form = req_form.part(part.name, file_part);
                }
            }
        }

        let mut req = self.client.post(url);

        for (name, value) in headers {
            req = req.header(*name, *value);
        }

        let response = req
            .multipart(req_form)
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        Self::convert_response(response).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reqwest_client_debug() {
        let client = ReqwestHttpClient::new().unwrap();
        let debug = format!("{:?}", client);
        assert!(debug.contains("ReqwestHttpClient"));
    }

    #[test]
    fn test_new_creates_client() {
        let client = ReqwestHttpClient::new();
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_get_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/test")
            .with_status(200)
            .with_body(r#"{"result": "ok"}"#)
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let response = client
            .get(&format!("{}/api/test", server.url()), &[])
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        assert!(response.is_success());
        let text = response.text().unwrap();
        assert!(text.contains("result"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_with_headers() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/auth")
            .match_header("Authorization", "Bearer token123")
            .match_header("X-Custom-Header", "custom-value")
            .with_status(200)
            .with_body("authorized")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let headers = [
            ("Authorization", "Bearer token123"),
            ("X-Custom-Header", "custom-value"),
        ];
        let response = client
            .get(&format!("{}/api/auth", server.url()), &headers)
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_error_status() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/not-found")
            .with_status(404)
            .with_body("Not Found")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let response = client
            .get(&format!("{}/api/not-found", server.url()), &[])
            .await
            .unwrap();

        assert_eq!(response.status, 404);
        assert!(!response.is_success());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_server_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/error")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let response = client
            .get(&format!("{}/api/error", server.url()), &[])
            .await
            .unwrap();

        assert_eq!(response.status, 500);
        assert!(!response.is_success());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_json_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/data")
            .match_header("content-type", "application/json")
            .with_status(201)
            .with_body(r#"{"id": 123}"#)
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let body = serde_json::json!({"name": "test", "value": 42});
        let response = client
            .post_json(&format!("{}/api/data", server.url()), &[], body)
            .await
            .unwrap();

        assert_eq!(response.status, 201);
        assert!(response.is_success());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_json_with_headers() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/data")
            .match_header("x-api-key", "secret-key")
            .with_status(200)
            .with_body(r#"{"success": true}"#)
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let headers = [("x-api-key", "secret-key")];
        let body = serde_json::json!({"data": "value"});
        let response = client
            .post_json(&format!("{}/api/data", server.url()), &headers, body)
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_binary_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/upload")
            .with_status(200)
            .with_body("uploaded")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let binary_data = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
        let headers = [("Content-Type", "application/octet-stream")];
        let response = client
            .post_binary(
                &format!("{}/api/upload", server.url()),
                &headers,
                binary_data,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_binary_with_query_params() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/upload")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("version".into(), "1".into()),
                mockito::Matcher::UrlEncoded("type".into(), "file".into()),
            ]))
            .with_status(200)
            .with_body("ok")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let query_params = [("version", "1"), ("type", "file")];
        let response = client
            .post_binary(
                &format!("{}/api/upload", server.url()),
                &[],
                b"binary content".to_vec(),
                Some(&query_params),
            )
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_multipart_text_field() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/form")
            .with_status(200)
            .with_body("form received")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let form = MultipartForm::new()
            .text("field1", "value1")
            .text("field2", "value2");

        let response = client
            .post_multipart(&format!("{}/api/form", server.url()), &[], form)
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_multipart_file_field() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/upload")
            .with_status(200)
            .with_body(r#"{"uploaded": true}"#)
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let form = MultipartForm::new().file(
            "document",
            "test.txt",
            "text/plain",
            b"file content here".to_vec(),
        );

        let response = client
            .post_multipart(&format!("{}/api/upload", server.url()), &[], form)
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_multipart_mixed_fields() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/mixed")
            .with_status(201)
            .with_body("created")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let form = MultipartForm::new()
            .text("description", "My file upload")
            .text("tags", "test,upload")
            .file(
                "attachment",
                "data.bin",
                "application/octet-stream",
                vec![0x00, 0x01, 0x02, 0x03],
            );

        let response = client
            .post_multipart(&format!("{}/api/mixed", server.url()), &[], form)
            .await
            .unwrap();

        assert_eq!(response.status, 201);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_response_headers_captured() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/headers")
            .with_status(200)
            .with_header("X-Custom-Response", "custom-value")
            .with_header("X-Request-Id", "12345")
            .with_body("ok")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let response = client
            .get(&format!("{}/api/headers", server.url()), &[])
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        // Headers should be captured (case-insensitive keys)
        assert!(
            response.headers.contains_key("x-custom-response")
                || response.headers.contains_key("X-Custom-Response")
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_connection_error() {
        let client = ReqwestHttpClient::new().unwrap();
        // Try to connect to a port that's not listening
        let result = client.get("http://127.0.0.1:59999/nonexistent", &[]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_empty_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/empty")
            .with_status(204)
            .with_body("")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let response = client
            .get(&format!("{}/api/empty", server.url()), &[])
            .await
            .unwrap();

        assert_eq!(response.status, 204);
        assert!(response.body.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_json_error_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/validate")
            .with_status(422)
            .with_body(r#"{"error": "validation_failed", "details": ["field1 required"]}"#)
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let body = serde_json::json!({});
        let response = client
            .post_json(&format!("{}/api/validate", server.url()), &[], body)
            .await
            .unwrap();

        assert_eq!(response.status, 422);
        assert!(!response.is_success());
        let text = response.text().unwrap();
        assert!(text.contains("validation_failed"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_post_binary_large_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/large")
            .with_status(200)
            .with_body("received")
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        // Create a 100KB body
        let large_body = vec![0xAB; 100 * 1024];
        let response = client
            .post_binary(
                &format!("{}/api/large", server.url()),
                &[],
                large_body,
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_response_json_parsing() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/json")
            .with_status(200)
            .with_body(r#"{"name": "test", "count": 42, "active": true}"#)
            .create_async()
            .await;

        let client = ReqwestHttpClient::new().unwrap();
        let response = client
            .get(&format!("{}/api/json", server.url()), &[])
            .await
            .unwrap();

        let parsed: serde_json::Value = response.json().unwrap();
        assert_eq!(parsed["name"], "test");
        assert_eq!(parsed["count"], 42);
        assert_eq!(parsed["active"], true);
        mock.assert_async().await;
    }
}
