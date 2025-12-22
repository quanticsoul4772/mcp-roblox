//! Open Cloud OrderedDataStore functionality
//!
//! OrderedDataStores store key-value pairs with sortable numerical values,
//! commonly used for leaderboards and ranking systems.
//! Uses v1 API: /ordered-data-stores/v1/universes/{universeId}/orderedDataStores/{orderedDataStore}/scopes/{scope}/entries

use crate::error::RobloxMcpError;
use crate::http::HttpClient;
use serde::{Deserialize, Serialize};

/// Parameters for listing ordered datastore entries
#[derive(Debug, Clone, Default)]
pub struct OrderedDataStoreListParams<'a> {
    /// Universe ID containing the datastore
    pub universe_id: u64,
    /// Name of the ordered datastore
    pub datastore_name: &'a str,
    /// Scope within the datastore (default: "global")
    pub scope: Option<&'a str>,
    /// Maximum entries to return per page (1-100)
    pub max_page_size: Option<u32>,
    /// Token for pagination (from previous response)
    pub page_token: Option<&'a str>,
    /// Sort order: "desc" (default) or "asc"
    pub order_by: Option<&'a str>,
    /// Optional filter expression for value ranges
    pub filter: Option<&'a str>,
}

/// Entry in an ordered datastore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedDataStoreEntry {
    /// Full path to this entry
    pub path: String,
    /// Unique identifier for this entry (the key)
    pub id: String,
    /// The numerical value stored for this entry
    pub value: i64,
}

/// Response from listing ordered datastore entries
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderedDataStoreList {
    /// List of entries returned
    pub entries: Vec<OrderedDataStoreEntry>,
    /// Token for fetching the next page (if more results exist)
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// Response from increment operation
#[derive(Debug, Clone, Deserialize)]
pub struct IncrementResponse {
    pub value: i64,
}

impl<H: HttpClient> super::OpenCloudClient<H> {
    /// List entries from an ordered datastore
    ///
    /// Returns entries sorted by value in descending order by default,
    /// commonly used to retrieve leaderboard rankings.
    ///
    /// # Arguments
    /// * `params` - Parameters for the list operation
    pub async fn ordered_datastore_list(
        &self,
        params: OrderedDataStoreListParams<'_>,
    ) -> Result<OrderedDataStoreList, RobloxMcpError> {
        let scope = params.scope.unwrap_or("global");
        let encoded_datastore = urlencoding::encode(params.datastore_name);
        let encoded_scope = urlencoding::encode(scope);

        // Build URL with query parameters
        let mut url = format!(
            "{}/ordered-data-stores/v1/universes/{}/orderedDataStores/{}/scopes/{}/entries",
            self.base_url(),
            params.universe_id,
            encoded_datastore,
            encoded_scope
        );

        // Add query parameters
        let mut query_params = Vec::new();
        if let Some(size) = params.max_page_size {
            query_params.push(format!("max_page_size={}", size.min(100)));
        }
        if let Some(token) = params.page_token {
            query_params.push(format!("page_token={}", urlencoding::encode(token)));
        }
        if let Some(order) = params.order_by {
            query_params.push(format!("order_by={}", order));
        }
        if let Some(f) = params.filter {
            query_params.push(format!("filter={}", urlencoding::encode(f)));
        }
        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

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

        response.json()
    }

    /// Set a value in an ordered datastore
    ///
    /// Creates or updates an entry with the specified key and value.
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the datastore
    /// * `datastore_name` - Name of the ordered datastore
    /// * `scope` - Scope within the datastore (default: "global")
    /// * `entry_id` - Unique identifier for the entry (key)
    /// * `value` - Numerical value to store
    pub async fn ordered_datastore_set(
        &self,
        universe_id: u64,
        datastore_name: &str,
        scope: Option<&str>,
        entry_id: &str,
        value: i64,
    ) -> Result<OrderedDataStoreEntry, RobloxMcpError> {
        let scope = scope.unwrap_or("global");
        let encoded_datastore = urlencoding::encode(datastore_name);
        let encoded_scope = urlencoding::encode(scope);
        let encoded_id = urlencoding::encode(entry_id);

        let url = format!(
            "{}/ordered-data-stores/v1/universes/{}/orderedDataStores/{}/scopes/{}/entries/{}",
            self.base_url(),
            universe_id,
            encoded_datastore,
            encoded_scope,
            encoded_id
        );

        let body = serde_json::json!({ "value": value });

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

        response.json()
    }

    /// Increment a value in an ordered datastore
    ///
    /// Atomically increments the value for the specified entry.
    /// Creates the entry with the increment value if it doesn't exist.
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the datastore
    /// * `datastore_name` - Name of the ordered datastore
    /// * `scope` - Scope within the datastore (default: "global")
    /// * `entry_id` - Unique identifier for the entry (key)
    /// * `increment` - Amount to add to the current value
    pub async fn ordered_datastore_increment(
        &self,
        universe_id: u64,
        datastore_name: &str,
        scope: Option<&str>,
        entry_id: &str,
        increment: i64,
    ) -> Result<OrderedDataStoreEntry, RobloxMcpError> {
        let scope = scope.unwrap_or("global");
        let encoded_datastore = urlencoding::encode(datastore_name);
        let encoded_scope = urlencoding::encode(scope);
        let encoded_id = urlencoding::encode(entry_id);

        let url = format!(
            "{}/ordered-data-stores/v1/universes/{}/orderedDataStores/{}/scopes/{}/entries/{}:increment",
            self.base_url(),
            universe_id,
            encoded_datastore,
            encoded_scope,
            encoded_id
        );

        let body = serde_json::json!({ "amount": increment });

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

        // Parse increment response and convert to entry
        let increment_resp: IncrementResponse = response.json()?;
        Ok(OrderedDataStoreEntry {
            path: format!(
                "universes/{}/orderedDataStores/{}/scopes/{}/entries/{}",
                universe_id, datastore_name, scope, entry_id
            ),
            id: entry_id.to_string(),
            value: increment_resp.value,
        })
    }

    /// Delete an entry from an ordered datastore
    ///
    /// # Arguments
    /// * `universe_id` - Universe ID containing the datastore
    /// * `datastore_name` - Name of the ordered datastore
    /// * `scope` - Scope within the datastore (default: "global")
    /// * `entry_id` - Unique identifier for the entry to delete
    pub async fn ordered_datastore_delete(
        &self,
        universe_id: u64,
        datastore_name: &str,
        scope: Option<&str>,
        entry_id: &str,
    ) -> Result<(), RobloxMcpError> {
        let scope = scope.unwrap_or("global");
        let encoded_datastore = urlencoding::encode(datastore_name);
        let encoded_scope = urlencoding::encode(scope);
        let encoded_id = urlencoding::encode(entry_id);

        let url = format!(
            "{}/ordered-data-stores/v1/universes/{}/orderedDataStores/{}/scopes/{}/entries/{}",
            self.base_url(),
            universe_id,
            encoded_datastore,
            encoded_scope,
            encoded_id
        );

        let response = self
            .http()
            .delete(&url, &[("x-api-key", self.api_key())])
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::OpenCloudClient;
    use crate::http::mock::{MockHttpClient, MockResponse};

    #[test]
    fn test_ordered_datastore_entry_deserialize() {
        let json = r#"{
            "path": "universes/123/orderedDataStores/Leaderboard/scopes/global/entries/player1",
            "id": "player1",
            "value": 1500
        }"#;

        let entry: OrderedDataStoreEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id, "player1");
        assert_eq!(entry.value, 1500);
        assert!(entry.path.contains("Leaderboard"));
    }

    #[test]
    fn test_ordered_datastore_list_deserialize() {
        let json = r#"{
            "entries": [
                {"path": "p1", "id": "player1", "value": 1500},
                {"path": "p2", "id": "player2", "value": 1200}
            ],
            "nextPageToken": "abc123"
        }"#;

        let list: OrderedDataStoreList = serde_json::from_str(json).unwrap();
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].value, 1500);
        assert_eq!(list.next_page_token, Some("abc123".to_string()));
    }

    #[test]
    fn test_ordered_datastore_list_no_token() {
        let json = r#"{"entries": []}"#;

        let list: OrderedDataStoreList = serde_json::from_str(json).unwrap();
        assert!(list.entries.is_empty());
        assert!(list.next_page_token.is_none());
    }

    #[tokio::test]
    async fn test_ordered_datastore_list_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({
                "entries": [
                    {"path": "p1", "id": "player1", "value": 1500},
                    {"path": "p2", "id": "player2", "value": 1200}
                ]
            }),
        ));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "Leaderboard",
                max_page_size: Some(10),
                ..Default::default()
            })
            .await;

        assert!(result.is_ok());
        let list = result.unwrap();
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].value, 1500);
    }

    #[tokio::test]
    async fn test_ordered_datastore_list_with_pagination() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({
                "entries": [{"path": "p", "id": "p1", "value": 100}],
                "nextPageToken": "next123"
            }),
        ));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        let result = client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "LB",
                max_page_size: Some(1),
                page_token: Some("prev_token"),
                ..Default::default()
            })
            .await;

        assert!(result.is_ok());
        let requests = mock.requests();
        assert!(requests[0].url.contains("page_token=prev_token"));
        assert!(requests[0].url.contains("max_page_size=1"));
    }

    #[tokio::test]
    async fn test_ordered_datastore_set_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({
                "path": "universes/123/orderedDataStores/LB/scopes/global/entries/player1",
                "id": "player1",
                "value": 2000
            }),
        ));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_set(123, "LB", None, "player1", 2000)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.id, "player1");
        assert_eq!(entry.value, 2000);
    }

    #[tokio::test]
    async fn test_ordered_datastore_increment_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"value": 150})));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_increment(123, "LB", None, "player1", 50)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.value, 150);
        assert_eq!(entry.id, "player1");
    }

    #[tokio::test]
    async fn test_ordered_datastore_delete_success() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(204, b""));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_delete(123, "LB", None, "player1")
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ordered_datastore_not_found() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(404, b"Entry not found"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_delete(123, "LB", None, "nonexistent")
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, .. } => {
                assert_eq!(status, 404);
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_ordered_datastore_url_encoding() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"entries": []})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "My Leaderboard",
                scope: Some("custom scope"),
                ..Default::default()
            })
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0].url.contains("My%20Leaderboard"));
        assert!(requests[0].url.contains("custom%20scope"));
    }

    // ========================================
    // Additional coverage tests for error paths
    // ========================================

    #[tokio::test]
    async fn test_ordered_datastore_list_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(500, b"Internal Server Error"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "LB",
                ..Default::default()
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, .. } => {
                assert_eq!(status, 500);
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_ordered_datastore_set_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(400, b"Invalid value"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_set(123, "LB", None, "player1", -100)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 400);
                assert!(message.contains("Invalid"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_ordered_datastore_increment_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(404, b"Entry not found"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_increment(123, "LB", None, "missing", 10)
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
    async fn test_ordered_datastore_delete_error() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::success(403, b"Forbidden"));

        let client = OpenCloudClient::with_http(mock, "test-key");

        let result = client
            .ordered_datastore_delete(123, "LB", Some("custom"), "entry")
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RobloxMcpError::OpenCloudError { status, message } => {
                assert_eq!(status, 403);
                assert!(message.contains("Forbidden"));
            }
            e => panic!("Expected OpenCloudError, got {e:?}"),
        }
    }

    #[tokio::test]
    async fn test_ordered_datastore_list_with_filter() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"entries": []})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "LB",
                filter: Some("value > 100"),
                ..Default::default()
            })
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0].url.contains("filter="));
        assert!(requests[0].url.contains("value"));
    }

    #[tokio::test]
    async fn test_ordered_datastore_list_with_order_by() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"entries": []})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "LB",
                order_by: Some("asc"),
                ..Default::default()
            })
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0].url.contains("order_by=asc"));
    }

    #[tokio::test]
    async fn test_ordered_datastore_set_with_scope() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(
            200,
            serde_json::json!({
                "path": "universes/123/orderedDataStores/LB/scopes/custom/entries/p1",
                "id": "p1",
                "value": 500
            }),
        ));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        let result = client
            .ordered_datastore_set(123, "LB", Some("custom"), "p1", 500)
            .await;

        assert!(result.is_ok());
        let requests = mock.requests();
        assert!(requests[0].url.contains("/scopes/custom/"));
    }

    #[tokio::test]
    async fn test_ordered_datastore_increment_with_scope() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"value": 200})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        let result = client
            .ordered_datastore_increment(123, "LB", Some("scoped"), "p1", 100)
            .await;

        assert!(result.is_ok());
        let entry = result.unwrap();
        assert_eq!(entry.value, 200);
        let requests = mock.requests();
        assert!(requests[0].url.contains("/scopes/scoped/"));
        assert!(requests[0].url.contains(":increment"));
    }

    #[tokio::test]
    async fn test_ordered_datastore_max_page_size_capped() {
        let mock = MockHttpClient::new();
        mock.queue_response(MockResponse::json(200, serde_json::json!({"entries": []})));

        let client = OpenCloudClient::with_http(mock.clone(), "test-key");

        // Request 500, but should be capped at 100
        client
            .ordered_datastore_list(OrderedDataStoreListParams {
                universe_id: 123,
                datastore_name: "LB",
                max_page_size: Some(500),
                ..Default::default()
            })
            .await
            .unwrap();

        let requests = mock.requests();
        assert!(requests[0].url.contains("max_page_size=100"));
    }

    #[test]
    fn test_increment_response_deserialize() {
        let json = r#"{"value": 12345}"#;
        let resp: IncrementResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.value, 12345);
    }

    #[test]
    fn test_increment_response_negative() {
        let json = r#"{"value": -500}"#;
        let resp: IncrementResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.value, -500);
    }

    #[test]
    fn test_ordered_datastore_entry_serialize() {
        let entry = OrderedDataStoreEntry {
            path: "universes/1/orderedDataStores/LB/scopes/global/entries/p1".to_string(),
            id: "p1".to_string(),
            value: 999,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("p1"));
        assert!(json.contains("999"));
    }

    #[test]
    fn test_ordered_datastore_list_params_default() {
        let params = OrderedDataStoreListParams::default();
        assert_eq!(params.universe_id, 0);
        assert!(params.datastore_name.is_empty());
        assert!(params.scope.is_none());
        assert!(params.max_page_size.is_none());
        assert!(params.page_token.is_none());
        assert!(params.order_by.is_none());
        assert!(params.filter.is_none());
    }
}
