//! HTTP client abstraction for testability
//!
//! This module provides a trait-based abstraction over HTTP operations,
//! allowing production code to use reqwest while tests can inject mocks.

mod reqwest_client;

pub use reqwest_client::ReqwestHttpClient;

use crate::error::RobloxMcpError;
use async_trait::async_trait;
use std::collections::HashMap;

/// HTTP response abstraction
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body as bytes
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Check if status indicates success (2xx)
    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Parse body as JSON
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, RobloxMcpError> {
        serde_json::from_slice(&self.body).map_err(|e| e.into())
    }

    /// Get body as string
    pub fn text(&self) -> Result<String, RobloxMcpError> {
        String::from_utf8(self.body.clone())
            .map_err(|e| RobloxMcpError::InvalidStudioData(e.to_string()))
    }
}

/// Multipart form data for uploads
#[derive(Debug, Clone)]
pub struct MultipartForm {
    pub parts: Vec<FormPart>,
}

impl MultipartForm {
    pub fn new() -> Self {
        Self { parts: vec![] }
    }

    pub fn text(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.parts.push(FormPart {
            name: name.into(),
            content: FormContent::Text(value.into()),
        });
        self
    }

    pub fn file(
        mut self,
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self {
        self.parts.push(FormPart {
            name: name.into(),
            content: FormContent::File {
                filename: filename.into(),
                content_type: content_type.into(),
                data,
            },
        });
        self
    }
}

impl Default for MultipartForm {
    fn default() -> Self {
        Self::new()
    }
}

/// Individual form part
#[derive(Debug, Clone)]
pub struct FormPart {
    pub name: String,
    pub content: FormContent,
}

/// Form part content types
#[derive(Debug, Clone)]
pub enum FormContent {
    Text(String),
    File {
        filename: String,
        content_type: String,
        data: Vec<u8>,
    },
}

/// Abstraction over HTTP operations for testability
///
/// This trait allows us to inject mock HTTP clients in tests while using
/// the real reqwest client in production.
#[async_trait]
pub trait HttpClient: Send + Sync + 'static {
    /// Perform a GET request
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, RobloxMcpError>;

    /// Perform a POST request with JSON body
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> Result<HttpResponse, RobloxMcpError>;

    /// Perform a POST request with binary body
    async fn post_binary(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
        query: Option<&[(&str, &str)]>,
    ) -> Result<HttpResponse, RobloxMcpError>;

    /// Perform a POST request with multipart form data
    async fn post_multipart(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        form: MultipartForm,
    ) -> Result<HttpResponse, RobloxMcpError>;

    /// Perform a DELETE request
    async fn delete(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, RobloxMcpError>;
}

#[cfg(test)]
pub mod mock;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_response_is_success() {
        assert!(HttpResponse {
            status: 200,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());
        assert!(HttpResponse {
            status: 201,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());
        assert!(HttpResponse {
            status: 299,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());
        assert!(!HttpResponse {
            status: 400,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());
        assert!(!HttpResponse {
            status: 500,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());
    }

    #[test]
    fn test_http_response_json() {
        let response = HttpResponse {
            status: 200,
            headers: Default::default(),
            body: br#"{"key": "value"}"#.to_vec(),
        };
        let parsed: serde_json::Value = response.json().unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn test_http_response_text() {
        let response = HttpResponse {
            status: 200,
            headers: Default::default(),
            body: b"hello world".to_vec(),
        };
        assert_eq!(response.text().unwrap(), "hello world");
    }

    #[test]
    fn test_multipart_form_builder() {
        let form = MultipartForm::new().text("field1", "value1").file(
            "file1",
            "test.txt",
            "text/plain",
            b"content".to_vec(),
        );

        assert_eq!(form.parts.len(), 2);
        assert!(matches!(&form.parts[0].content, FormContent::Text(s) if s == "value1"));
        assert!(
            matches!(&form.parts[1].content, FormContent::File { filename, .. } if filename == "test.txt")
        );
    }

    #[test]
    fn test_http_response_text_invalid_utf8() {
        let response = HttpResponse {
            status: 200,
            headers: Default::default(),
            body: vec![0xFF, 0xFE, 0x00, 0x01], // Invalid UTF-8 bytes
        };
        let result = response.text();
        assert!(result.is_err());
    }

    #[test]
    fn test_http_response_text_empty() {
        let response = HttpResponse {
            status: 200,
            headers: Default::default(),
            body: vec![],
        };
        assert_eq!(response.text().unwrap(), "");
    }

    #[test]
    fn test_http_response_json_invalid() {
        let response = HttpResponse {
            status: 200,
            headers: Default::default(),
            body: b"not valid json".to_vec(),
        };
        let result: Result<serde_json::Value, _> = response.json();
        assert!(result.is_err());
    }

    #[test]
    fn test_multipart_form_default() {
        let form = MultipartForm::default();
        assert!(form.parts.is_empty());
    }

    #[test]
    fn test_multipart_form_multiple_files() {
        let form = MultipartForm::new()
            .file("file1", "doc1.pdf", "application/pdf", vec![1, 2, 3])
            .file("file2", "doc2.pdf", "application/pdf", vec![4, 5, 6]);

        assert_eq!(form.parts.len(), 2);
    }

    #[test]
    fn test_form_content_debug() {
        let text_content = FormContent::Text("test".to_string());
        let debug = format!("{:?}", text_content);
        assert!(debug.contains("Text"));

        let file_content = FormContent::File {
            filename: "test.txt".to_string(),
            content_type: "text/plain".to_string(),
            data: vec![1, 2, 3],
        };
        let debug = format!("{:?}", file_content);
        assert!(debug.contains("File"));
        assert!(debug.contains("test.txt"));
    }

    #[test]
    fn test_form_part_debug() {
        let part = FormPart {
            name: "field".to_string(),
            content: FormContent::Text("value".to_string()),
        };
        let debug = format!("{:?}", part);
        assert!(debug.contains("FormPart"));
        assert!(debug.contains("field"));
    }

    #[test]
    fn test_http_response_status_boundaries() {
        // Test 199 (not success)
        assert!(!HttpResponse {
            status: 199,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());

        // Test 200 (success)
        assert!(HttpResponse {
            status: 200,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());

        // Test 300 (not success - redirect)
        assert!(!HttpResponse {
            status: 300,
            headers: Default::default(),
            body: vec![]
        }
        .is_success());
    }

    #[test]
    fn test_http_response_debug() {
        let response = HttpResponse {
            status: 404,
            headers: Default::default(),
            body: b"not found".to_vec(),
        };
        let debug = format!("{:?}", response);
        assert!(debug.contains("HttpResponse"));
        assert!(debug.contains("404"));
    }

    #[test]
    fn test_http_response_clone() {
        let original = HttpResponse {
            status: 200,
            headers: {
                let mut h = std::collections::HashMap::new();
                h.insert("content-type".to_string(), "application/json".to_string());
                h
            },
            body: b"test body".to_vec(),
        };
        let cloned = original.clone();
        assert_eq!(cloned.status, 200);
        assert_eq!(cloned.body, b"test body");
        assert_eq!(
            cloned.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
    }
}
