//! Open Cloud DataStore functionality
//!
//! Read and write data from Roblox DataStores via Open Cloud API.
//! Uses the v1 API endpoint format: /datastores/v1/universes/{id}/standard-datastores/datastore/entries/entry

use crate::error::RobloxMcpError;
use crate::http::HttpClient;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use md5;
use serde::{Deserialize, Serialize};

/// Result from reading a DataStore entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStoreEntry {
    /// The value stored in the entry (JSON)
    pub value: serde_json::Value,
    /// Version identifier
    #[serde(default)]
    pub version: String,
    /// Created timestamp (ISO 8601)
    #[serde(default)]
    pub created_time: String,
    /// Last updated timestamp (ISO 8601)
    #[serde(default)]
    pub updated_time: String,
}

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// Get a value from a DataStore
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the DataStore
    /// * `datastore_name` - Name of the DataStore
    /// * `key` - Entry key to retrieve
    /// * `scope` - Optional scope (default: "global")
    ///
    /// # Errors
    /// Returns error if key not found or API call fails
    pub async fn datastore_get(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        let scope = scope.unwrap_or("global");

        // URL encode the key and datastore name for query params
        let encoded_key = urlencoding::encode(key);
        let encoded_datastore = urlencoding::encode(datastore_name);

        // V1 API format: /datastores/v1/universes/{id}/standard-datastores/datastore/entries/entry
        let url = format!(
            "{}/datastores/v1/universes/{}/standard-datastores/datastore/entries/entry?datastoreName={}&entryKey={}&scope={}",
            self.base_url(),
            universe_id,
            encoded_datastore,
            encoded_key,
            scope
        );

        let response = self
            .http()
            .get(&url, &[("x-api-key", self.api_key())])
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

        // The response body IS the value, metadata comes from headers
        let version = response
            .headers
            .get("roblox-entry-version")
            .cloned()
            .unwrap_or_default();

        let created_time = response
            .headers
            .get("roblox-entry-created-time")
            .cloned()
            .unwrap_or_default();

        let updated_time = response
            .headers
            .get("roblox-entry-version-created-time")
            .cloned()
            .unwrap_or_default();

        let value: serde_json::Value = response.json()?;

        Ok(DataStoreEntry {
            value,
            version,
            created_time,
            updated_time,
        })
    }

    /// Set a value in a DataStore
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the DataStore
    /// * `datastore_name` - Name of the DataStore
    /// * `key` - Entry key to set
    /// * `value` - JSON value to store
    /// * `scope` - Optional scope (default: "global")
    ///
    /// # Errors
    /// Returns error if API call fails or unauthorized
    pub async fn datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        key: &str,
        value: serde_json::Value,
        scope: Option<&str>,
    ) -> Result<DataStoreEntry, RobloxMcpError> {
        let scope = scope.unwrap_or("global");

        // URL encode the key and datastore name for query params
        let encoded_key = urlencoding::encode(key);
        let encoded_datastore = urlencoding::encode(datastore_name);

        // V1 API format: /datastores/v1/universes/{id}/standard-datastores/datastore/entries/entry
        let url = format!(
            "{}/datastores/v1/universes/{}/standard-datastores/datastore/entries/entry?datastoreName={}&entryKey={}&scope={}",
            self.base_url(),
            universe_id,
            encoded_datastore,
            encoded_key,
            scope
        );

        // V1 API requires content-md5 header with base64-encoded MD5 of the body
        let body_bytes = serde_json::to_vec(&value).map_err(|e| {
            RobloxMcpError::InvalidStudioData(format!("Failed to serialize value: {}", e))
        })?;
        let md5_digest = md5::compute(&body_bytes);
        let content_md5 = BASE64.encode(md5_digest.as_ref());

        let response = self
            .http()
            .post_binary(
                &url,
                &[
                    ("x-api-key", self.api_key()),
                    ("Content-Type", "application/json"),
                    ("content-md5", &content_md5),
                ],
                body_bytes,
                None,
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

        // Extract metadata from response headers
        let version = response
            .headers
            .get("roblox-entry-version")
            .cloned()
            .unwrap_or_default();

        let created_time = response
            .headers
            .get("roblox-entry-created-time")
            .cloned()
            .unwrap_or_default();

        let updated_time = response
            .headers
            .get("roblox-entry-version-created-time")
            .cloned()
            .unwrap_or_default();

        Ok(DataStoreEntry {
            value,
            version,
            created_time,
            updated_time,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datastore_entry_deserialize() {
        let json = r#"{
            "value": {"coins": 100, "level": 5},
            "version": "v1",
            "createdTime": "2024-01-01T00:00:00Z",
            "updatedTime": "2024-01-02T00:00:00Z"
        }"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.value["coins"], 100);
        assert_eq!(entry.value["level"], 5);
        assert_eq!(entry.version, "v1");
    }

    #[test]
    fn test_datastore_entry_with_missing_fields() {
        let json = r#"{"value": "simple string"}"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.value, "simple string");
        assert_eq!(entry.version, "");
    }

    #[test]
    fn test_datastore_entry_serialize() {
        let entry = DataStoreEntry {
            value: serde_json::json!({"health": 100, "items": ["sword", "shield"]}),
            version: "v2".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T12:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("health"));
        assert!(json.contains("100"));
        assert!(json.contains("sword"));
        assert!(json.contains("v2"));
        assert!(json.contains("createdTime")); // camelCase due to serde rename_all
    }

    #[test]
    fn test_datastore_entry_with_null_value() {
        let json = r#"{"value": null}"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert!(entry.value.is_null());
        assert_eq!(entry.version, "");
        assert_eq!(entry.created_time, "");
        assert_eq!(entry.updated_time, "");
    }

    #[test]
    fn test_datastore_entry_with_array_value() {
        let json = r#"{"value": [1, 2, 3, 4, 5]}"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert!(entry.value.is_array());
        assert_eq!(entry.value.as_array().unwrap().len(), 5);
    }

    #[test]
    fn test_datastore_entry_with_nested_objects() {
        let json = r#"{
            "value": {
                "player": {
                    "stats": {"level": 50, "xp": 12500},
                    "inventory": {"slots": 20, "items": []}
                }
            },
            "version": "v3"
        }"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.value["player"]["stats"]["level"], 50);
        assert_eq!(entry.value["player"]["stats"]["xp"], 12500);
        assert_eq!(entry.value["player"]["inventory"]["slots"], 20);
        assert_eq!(entry.version, "v3");
    }

    #[test]
    fn test_datastore_entry_with_boolean_value() {
        let json = r#"{"value": true}"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.value, true);
    }

    #[test]
    fn test_datastore_entry_with_numeric_value() {
        let json = r#"{"value": 42.5}"#;

        let entry: DataStoreEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.value, 42.5);
    }

    #[test]
    fn test_datastore_entry_clone() {
        let entry = DataStoreEntry {
            value: serde_json::json!({"test": "data"}),
            version: "v1".to_string(),
            created_time: "2024-01-01T00:00:00Z".to_string(),
            updated_time: "2024-01-02T00:00:00Z".to_string(),
        };

        let cloned = entry.clone();
        assert_eq!(cloned.value, entry.value);
        assert_eq!(cloned.version, entry.version);
        assert_eq!(cloned.created_time, entry.created_time);
        assert_eq!(cloned.updated_time, entry.updated_time);
    }

    #[test]
    fn test_datastore_entry_debug() {
        let entry = DataStoreEntry {
            value: serde_json::json!("test"),
            version: "v1".to_string(),
            created_time: "".to_string(),
            updated_time: "".to_string(),
        };

        let debug = format!("{:?}", entry);
        assert!(debug.contains("DataStoreEntry"));
        assert!(debug.contains("value"));
        assert!(debug.contains("version"));
    }

    #[test]
    fn test_datastore_entry_roundtrip() {
        let original = DataStoreEntry {
            value: serde_json::json!({"coins": 1000, "gems": 50}),
            version: "abc123".to_string(),
            created_time: "2024-06-15T10:30:00Z".to_string(),
            updated_time: "2024-06-16T14:45:00Z".to_string(),
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: DataStoreEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.value, original.value);
        assert_eq!(parsed.version, original.version);
        assert_eq!(parsed.created_time, original.created_time);
        assert_eq!(parsed.updated_time, original.updated_time);
    }

    // ========================================
    // Mock-based tests for datastore_get
    // ========================================
    use crate::cloud::OpenCloudClient;
    use crate::http::mock::{MockHttpClient, MockResponse};

    #[tokio::test]
    async fn test_datastore_get_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(
            MockResponse::json(200, serde_json::json!({"coins": 500, "level": 10})).with_headers([
                ("roblox-entry-version".to_string(), "v1234".to_string()),
                (
                    "roblox-entry-created-time".to_string(),
                    "2024-01-01T00:00:00Z".to_string(),
                ),
                (
                    "roblox-entry-version-created-time".to_string(),
                    "2024-01-02T00:00:00Z".to_string(),
                ),
            ]),
        );

        let client = OpenCloudClient::with_http(mock, "test-api-key");

        let result = client
            .datastore_get(123456, "PlayerData", "player_42", None)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.value["coins"], 500);
        assert_eq!(entry.value["level"], 10);
        assert_eq!(entry.version, "v1234");
        assert_eq!(entry.created_time, "2024-01-01T00:00:00Z");
        assert_eq!(entry.updated_time, "2024-01-02T00:00:00Z");
    }

    #[tokio::test]
    async fn test_datastore_get_with_scope() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"data": "test"})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .datastore_get(111, "MyStore", "key123", Some("custom_scope"))
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        // V1 API uses scope as query parameter
        assert!(requests[0].url.contains("scope=custom_scope"));
    }

    #[tokio::test]
    async fn test_datastore_get_default_scope() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .datastore_get(111, "MyStore", "key", None)
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        // V1 API uses scope as query parameter
        assert!(requests[0].url.contains("scope=global"));
    }

    #[tokio::test]
    async fn test_datastore_get_not_found() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(404, b"Entry not found"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .datastore_get(123, "PlayerData", "nonexistent_key", None)
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
    async fn test_datastore_get_unauthorized() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(401, b"Unauthorized"));

        let client = OpenCloudClient::with_http(mock, "bad-key");

        let result = client.datastore_get(123, "Store", "key", None).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, .. } => {
                assert_eq!(status, 401);
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_datastore_get_connection_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Connection timeout"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client.datastore_get(123, "Store", "key", None).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::HttpConnectionError(_)
        ));
    }

    #[tokio::test]
    async fn test_datastore_get_url_encoding() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        // Key with special characters that need URL encoding
        client
            .datastore_get(123, "My Store", "key/with/slashes", None)
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        // URL should contain encoded versions
        assert!(requests[0].url.contains("My%20Store"));
        assert!(requests[0].url.contains("key%2Fwith%2Fslashes"));
    }

    #[tokio::test]
    async fn test_datastore_get_sends_api_key_header() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "secret-api-key-12345");

        client
            .datastore_get(999, "TestStore", "testKey", None)
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "secret-api-key-12345"));
    }

    #[tokio::test]
    async fn test_datastore_get_missing_headers() {
        // Test that missing metadata headers result in empty strings (not errors)
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({"simple": "value"}),
        ));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client.datastore_get(123, "Store", "key", None).await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.version, "");
        assert_eq!(entry.created_time, "");
        assert_eq!(entry.updated_time, "");
    }

    // ========================================
    // Mock-based tests for datastore_set
    // ========================================

    #[tokio::test]
    async fn test_datastore_set_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(
            MockResponse::json(200, serde_json::json!({})).with_headers([
                ("roblox-entry-version".to_string(), "v5678".to_string()),
                (
                    "roblox-entry-created-time".to_string(),
                    "2024-01-01T00:00:00Z".to_string(),
                ),
                (
                    "roblox-entry-version-created-time".to_string(),
                    "2024-06-15T12:00:00Z".to_string(),
                ),
            ]),
        );

        let client = OpenCloudClient::with_http(mock, "test-api-key");

        let value = serde_json::json!({"coins": 1000, "level": 25});
        let result = client
            .datastore_set(123456, "PlayerData", "player_42", value.clone(), None)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.value, value);
        assert_eq!(entry.version, "v5678");
        assert_eq!(entry.created_time, "2024-01-01T00:00:00Z");
        assert_eq!(entry.updated_time, "2024-06-15T12:00:00Z");
    }

    #[tokio::test]
    async fn test_datastore_set_with_scope() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .datastore_set(
                111,
                "MyStore",
                "key123",
                serde_json::json!("test"),
                Some("custom_scope"),
            )
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        // V1 API uses scope as query parameter
        assert!(requests[0].url.contains("scope=custom_scope"));
    }

    #[tokio::test]
    async fn test_datastore_set_default_scope() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .datastore_set(111, "MyStore", "key", serde_json::json!(42), None)
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        // V1 API uses scope as query parameter
        assert!(requests[0].url.contains("scope=global"));
    }

    #[tokio::test]
    async fn test_datastore_set_unauthorized() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(401, b"Unauthorized"));

        let client = OpenCloudClient::with_http(mock, "bad-key");

        let result = client
            .datastore_set(123, "Store", "key", serde_json::json!("value"), None)
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
    async fn test_datastore_set_connection_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::error("Connection timeout"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .datastore_set(123, "Store", "key", serde_json::json!({}), None)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RobloxMcpError::HttpConnectionError(_)
        ));
    }

    #[tokio::test]
    async fn test_datastore_set_url_encoding() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        // Key with special characters that need URL encoding
        client
            .datastore_set(
                123,
                "My Store",
                "key/with/slashes",
                serde_json::json!("test"),
                None,
            )
            .await
            .unwrap();

        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        // URL should contain encoded versions
        assert!(requests[0].url.contains("My%20Store"));
        assert!(requests[0].url.contains("key%2Fwith%2Fslashes"));
    }

    #[tokio::test]
    async fn test_datastore_set_sends_api_key_header() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock.clone(), "secret-api-key-12345");

        client
            .datastore_set(
                999,
                "TestStore",
                "testKey",
                serde_json::json!({"data": true}),
                None,
            )
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "secret-api-key-12345"));
    }

    #[tokio::test]
    async fn test_datastore_set_complex_value() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let complex_value = serde_json::json!({
            "player": {
                "stats": {"level": 50, "xp": 12500},
                "inventory": ["sword", "shield", "potion"],
                "settings": {"sound": true, "music": false}
            }
        });

        let result = client
            .datastore_set(123, "GameData", "player_001", complex_value.clone(), None)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.value, complex_value);
    }

    #[tokio::test]
    async fn test_datastore_set_missing_headers() {
        // Test that missing metadata headers result in empty strings (not errors)
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({})));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .datastore_set(123, "Store", "key", serde_json::json!("value"), None)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.version, "");
        assert_eq!(entry.created_time, "");
        assert_eq!(entry.updated_time, "");
    }
}
