//! Configuration for AI features.

use secrecy::{ExposeSecret, Secret};

use crate::error::RobloxMcpError;

/// Voyage AI configuration for embedding generation.
#[derive(Clone)]
pub struct VoyageConfig {
    /// API key for Voyage AI (protected from logging)
    pub api_key: Secret<String>,
    /// Embedding model to use (default: voyage-code-3)
    pub model: String,
    /// Embedding dimensions (default: 1024)
    pub dimensions: usize,
}

impl VoyageConfig {
    /// Load configuration from environment variables.
    ///
    /// # Required
    /// - `VOYAGE_API_KEY`: Voyage AI API key
    ///
    /// # Optional
    /// - `VOYAGE_MODEL`: Model name (default: `voyage-code-3`)
    /// - `VOYAGE_DIMENSIONS`: Dimensions (default: `1024`)
    pub fn from_env() -> Result<Self, RobloxMcpError> {
        let api_key = std::env::var("VOYAGE_API_KEY").map_err(|_| {
            RobloxMcpError::ConfigError("VOYAGE_API_KEY environment variable not set".into())
        })?;

        let model =
            std::env::var("VOYAGE_MODEL").unwrap_or_else(|_| "voyage-code-3".to_string());

        let dimensions = std::env::var("VOYAGE_DIMENSIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024);

        Ok(Self {
            api_key: Secret::new(api_key),
            model,
            dimensions,
        })
    }

    /// Get the API key (exposed for HTTP requests).
    pub fn api_key(&self) -> &str {
        self.api_key.expose_secret()
    }
}

impl std::fmt::Debug for VoyageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoyageConfig")
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("dimensions", &self.dimensions)
            .finish()
    }
}

/// Neo4j configuration for knowledge graph storage.
#[derive(Clone)]
pub struct Neo4jConfig {
    /// Connection URI (e.g., `neo4j+s://xxx.databases.neo4j.io`)
    pub uri: String,
    /// Username (default: `neo4j`)
    pub username: String,
    /// Password (protected from logging)
    pub password: Secret<String>,
    /// Database name (default: `neo4j`)
    pub database: String,
}

impl Neo4jConfig {
    /// Load configuration from environment variables.
    ///
    /// # Required
    /// - `NEO4J_URI`: Connection URI
    /// - `NEO4J_PASSWORD`: Password
    ///
    /// # Optional
    /// - `NEO4J_USERNAME`: Username (default: `neo4j`)
    /// - `NEO4J_DATABASE`: Database name (default: `neo4j`)
    pub fn from_env() -> Result<Self, RobloxMcpError> {
        let uri = std::env::var("NEO4J_URI").map_err(|_| {
            RobloxMcpError::ConfigError("NEO4J_URI environment variable not set".into())
        })?;

        let password = std::env::var("NEO4J_PASSWORD").map_err(|_| {
            RobloxMcpError::ConfigError("NEO4J_PASSWORD environment variable not set".into())
        })?;

        let username =
            std::env::var("NEO4J_USERNAME").unwrap_or_else(|_| "neo4j".to_string());

        let database =
            std::env::var("NEO4J_DATABASE").unwrap_or_else(|_| "neo4j".to_string());

        Ok(Self {
            uri,
            username,
            password: Secret::new(password),
            database,
        })
    }

    /// Get the password (exposed for connection).
    pub fn password(&self) -> &str {
        self.password.expose_secret()
    }
}

impl std::fmt::Debug for Neo4jConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Neo4jConfig")
            .field("uri", &self.uri)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("database", &self.database)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_voyage_config_debug_redacts_api_key() {
        let config = VoyageConfig {
            api_key: Secret::new("secret-key".to_string()),
            model: "voyage-code-3".to_string(),
            dimensions: 1024,
        };
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("secret-key"));
    }

    #[test]
    fn test_neo4j_config_debug_redacts_password() {
        let config = Neo4jConfig {
            uri: "neo4j+s://test.neo4j.io".to_string(),
            username: "neo4j".to_string(),
            password: Secret::new("secret-password".to_string()),
            database: "neo4j".to_string(),
        };
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("secret-password"));
    }

    #[test]
    fn test_voyage_config_defaults() {
        // Clear any existing env vars
        env::remove_var("VOYAGE_MODEL");
        env::remove_var("VOYAGE_DIMENSIONS");
        env::set_var("VOYAGE_API_KEY", "test-key");

        let config = VoyageConfig::from_env().unwrap();
        assert_eq!(config.model, "voyage-code-3");
        assert_eq!(config.dimensions, 1024);

        env::remove_var("VOYAGE_API_KEY");
    }

    #[test]
    fn test_neo4j_config_defaults() {
        env::remove_var("NEO4J_USERNAME");
        env::remove_var("NEO4J_DATABASE");
        env::set_var("NEO4J_URI", "neo4j+s://test.neo4j.io");
        env::set_var("NEO4J_PASSWORD", "test-password");

        let config = Neo4jConfig::from_env().unwrap();
        assert_eq!(config.username, "neo4j");
        assert_eq!(config.database, "neo4j");

        env::remove_var("NEO4J_URI");
        env::remove_var("NEO4J_PASSWORD");
    }
}
