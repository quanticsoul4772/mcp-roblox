//! Open Cloud MessagingService functionality
//!
//! Publish messages to Roblox MessagingService topics via Open Cloud API.

use crate::error::RobloxMcpError;
use crate::http::HttpClient;
use serde::{Deserialize, Serialize};

/// Result from publishing a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePublishResult {
    /// Whether the message was successfully published
    pub success: bool,
    /// The topic the message was published to
    pub topic: String,
}

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// Publish a message to a MessagingService topic
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID to publish the message to
    /// * `topic` - Topic name to publish to
    /// * `message` - Message content (will be JSON stringified)
    ///
    /// # Errors
    /// Returns error if API call fails or unauthorized
    ///
    /// # Notes
    /// Messages are delivered to all servers in the experience that are
    /// subscribed to the topic. Message size is limited by Roblox API.
    pub async fn messaging_publish(
        &self,
        universe_id: u64,
        topic: &str,
        message: serde_json::Value,
    ) -> Result<MessagePublishResult, RobloxMcpError> {
        let encoded_topic = urlencoding::encode(topic);

        let url = format!(
            "{}/messaging-service/v1/universes/{}/topics/{}",
            self.base_url(),
            universe_id,
            encoded_topic
        );

        // The message body must contain a "message" field with the stringified JSON
        let body = serde_json::json!({
            "message": message.to_string()
        });

        let response = self
            .http()
            .post_json(
                &url,
                &[
                    ("x-api-key", self.api_key()),
                    ("Content-Type", "application/json"),
                ],
                body,
            )
            .await?;

        if !response.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| "[failed to read body]".into());
            return Err(RobloxMcpError::OpenCloudError {
                status: response.status,
                message: body,
            });
        }

        Ok(MessagePublishResult {
            success: true,
            topic: topic.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::OpenCloudClient;
    use crate::http::mock::{MockHttpClient, MockResponse};

    #[test]
    fn test_message_publish_result_serialize() {
        let result = MessagePublishResult {
            success: true,
            topic: "GameEvents".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"topic\":\"GameEvents\""));
    }

    #[test]
    fn test_message_publish_result_deserialize() {
        let json = r#"{"success": true, "topic": "PlayerUpdates"}"#;
        let result: MessagePublishResult = serde_json::from_str(json).unwrap();

        assert!(result.success);
        assert_eq!(result.topic, "PlayerUpdates");
    }

    #[test]
    fn test_message_publish_result_clone() {
        let result = MessagePublishResult {
            success: true,
            topic: "Test".to_string(),
        };

        let cloned = result.clone();
        assert_eq!(cloned.success, result.success);
        assert_eq!(cloned.topic, result.topic);
    }

    #[test]
    fn test_message_publish_result_debug() {
        let result = MessagePublishResult {
            success: true,
            topic: "Debug".to_string(),
        };

        let debug = format!("{:?}", result);
        assert!(debug.contains("MessagePublishResult"));
        assert!(debug.contains("success"));
        assert!(debug.contains("topic"));
    }

    #[tokio::test]
    async fn test_messaging_publish_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock, "test-api-key");

        let result = client
            .messaging_publish(
                123456,
                "GameEvents",
                serde_json::json!({"event": "player_joined", "player_id": 42}),
            )
            .await;

        assert!(result.is_ok());
        let publish_result = result.unwrap();
        assert!(publish_result.success);
        assert_eq!(publish_result.topic, "GameEvents");
    }

    #[tokio::test]
    async fn test_messaging_publish_url_encoding() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        // Topic with special characters
        client
            .messaging_publish(123, "Topic/With/Slashes", serde_json::json!("test"))
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].url.contains("Topic%2FWith%2FSlashes"));
    }

    #[tokio::test]
    async fn test_messaging_publish_sends_api_key() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "secret-api-key-xyz");

        client
            .messaging_publish(999, "TestTopic", serde_json::json!({"data": true}))
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "secret-api-key-xyz"));
    }

    #[tokio::test]
    async fn test_messaging_publish_unauthorized() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(401, b"Unauthorized"));

        let client = OpenCloudClient::with_http(mock, "bad-key");

        let result = client
            .messaging_publish(123, "Topic", serde_json::json!("message"))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, .. } => {
                assert_eq!(status, 401);
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_messaging_publish_not_found() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(404, b"Universe not found"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .messaging_publish(999999, "Topic", serde_json::json!("message"))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 404);
                assert!(message.contains("not found"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_messaging_publish_connection_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Connection timeout"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .messaging_publish(123, "Topic", serde_json::json!({}))
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::HttpConnectionError(_)
        ));
    }

    #[tokio::test]
    async fn test_messaging_publish_complex_message() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let complex_message = serde_json::json!({
            "event": "game_update",
            "data": {
                "scores": [100, 200, 300],
                "players": ["Alice", "Bob"],
                "metadata": {
                    "timestamp": "2024-01-01T00:00:00Z",
                    "version": 1
                }
            }
        });

        let result = client
            .messaging_publish(123, "GameUpdates", complex_message)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_messaging_publish_rate_limited() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(429, b"Rate limit exceeded"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .messaging_publish(123, "Topic", serde_json::json!("message"))
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 429);
                assert!(message.contains("Rate limit"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_messaging_publish_empty_topic() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        // Empty topic should still work (URL encoding handles it)
        let result = client
            .messaging_publish(123, "", serde_json::json!("message"))
            .await;

        assert!(result.is_ok());
        let requests = mock.requests();
        assert!(requests[0].url.contains("/topics/"));
    }

    #[tokio::test]
    async fn test_messaging_publish_special_characters_in_message() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock, "test-key");

        // Message with special characters, unicode, and escapes
        let message = serde_json::json!({
            "greeting": "Hello, 世界! 🎮",
            "special": "Line1\nLine2\tTabbed",
            "quotes": "He said \"Hello\""
        });

        let result = client.messaging_publish(123, "Topic", message).await;
        assert!(result.is_ok());
    }
}
