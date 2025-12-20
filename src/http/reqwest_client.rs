//! Production HTTP client implementation using reqwest

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use crate::error::RobloxMcpError;
use super::{FormContent, HttpClient, HttpResponse, MultipartForm};

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

        let body = response.bytes().await.map_err(RobloxMcpError::from_reqwest)?;

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
                FormContent::File { filename, content_type, data } => {
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
}
