//! Open Cloud DataStore functionality
//!
//! Read and write data from Roblox DataStores via Open Cloud API.

use crate::error::RobloxMcpError;
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

impl super::OpenCloudClient {
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

        // URL encode the key and datastore name
        let encoded_key = urlencoding::encode(key);
        let encoded_datastore = urlencoding::encode(datastore_name);

        let url = format!(
            "{}/cloud/v2/universes/{}/data-stores/{}/scopes/{}/entries/{}",
            self.base_url(),
            universe_id,
            encoded_datastore,
            scope,
            encoded_key
        );

        let response = self
            .client()
            .get(&url)
            .header("x-api-key", self.api_key())
            .send()
            .await
            .map_err(RobloxMcpError::from_reqwest)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RobloxMcpError::OpenCloudError {
                status: status.as_u16(),
                message: body,
            });
        }

        // The response body IS the value, metadata comes from headers
        let version = response
            .headers()
            .get("roblox-entry-version")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let created_time = response
            .headers()
            .get("roblox-entry-created-time")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let updated_time = response
            .headers()
            .get("roblox-entry-version-created-time")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let value: serde_json::Value = response.json().await.map_err(RobloxMcpError::from_reqwest)?;

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
}
