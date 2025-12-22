//! HTTP Bridge Authentication
//!
//! Provides ephemeral token-based authentication for the HTTP bridge
//! to prevent unauthorized access from other localhost processes.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use subtle::ConstantTimeEq;

/// Length of the random bytes used for token generation
const TOKEN_BYTES: usize = 32;

/// Ephemeral authentication token for HTTP bridge
///
/// Generated at server startup and required for all authenticated endpoints.
/// Uses cryptographically secure random generation and constant-time comparison.
#[derive(Clone)]
pub struct AuthToken {
    /// Base64-encoded token string
    token: String,
    /// Raw bytes for constant-time comparison
    token_bytes: Vec<u8>,
}

impl AuthToken {
    /// Generate a new cryptographically secure authentication token
    ///
    /// Uses the system's cryptographic random number generator
    /// and encodes the result as URL-safe base64.
    pub fn generate() -> Self {
        let mut bytes = [0u8; TOKEN_BYTES];
        rand::thread_rng().fill(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let token_bytes = token.as_bytes().to_vec();
        Self { token, token_bytes }
    }

    /// Get the token string for display/logging
    ///
    /// This should be logged at startup so the Roblox Studio plugin
    /// can be configured with the token.
    pub fn as_str(&self) -> &str {
        &self.token
    }

    /// Validate a candidate token using constant-time comparison
    ///
    /// Returns true if the candidate matches the stored token.
    /// Uses constant-time comparison to prevent timing attacks.
    pub fn validate(&self, candidate: &str) -> bool {
        let candidate_bytes = candidate.as_bytes();
        // Constant-time comparison prevents timing attacks
        self.token_bytes.ct_eq(candidate_bytes).into()
    }

    /// Create a token from an existing string (for testing)
    #[cfg(test)]
    pub fn from_string(token: String) -> Self {
        let token_bytes = token.as_bytes().to_vec();
        Self { token, token_bytes }
    }
}

impl std::fmt::Debug for AuthToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't expose the actual token in debug output
        f.debug_struct("AuthToken")
            .field("token", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_creates_unique_tokens() {
        let token1 = AuthToken::generate();
        let token2 = AuthToken::generate();

        // Tokens should be different
        assert_ne!(token1.as_str(), token2.as_str());
    }

    #[test]
    fn test_generate_creates_valid_base64() {
        let token = AuthToken::generate();
        let decoded = URL_SAFE_NO_PAD.decode(token.as_str());
        assert!(decoded.is_ok());
        assert_eq!(decoded.unwrap().len(), TOKEN_BYTES);
    }

    #[test]
    fn test_validate_accepts_correct_token() {
        let token = AuthToken::generate();
        let token_str = token.as_str().to_string();

        assert!(token.validate(&token_str));
    }

    #[test]
    fn test_validate_rejects_wrong_token() {
        let token = AuthToken::generate();
        let wrong = "definitely-not-the-right-token";

        assert!(!token.validate(wrong));
    }

    #[test]
    fn test_validate_rejects_empty_token() {
        let token = AuthToken::generate();
        assert!(!token.validate(""));
    }

    #[test]
    fn test_validate_rejects_similar_token() {
        let token = AuthToken::generate();
        let mut similar = token.as_str().to_string();
        // Change one character
        if let Some(first) = similar.chars().next() {
            let replacement = if first == 'A' { 'B' } else { 'A' };
            similar = replacement.to_string() + &similar[1..];
        }

        assert!(!token.validate(&similar));
    }

    #[test]
    fn test_debug_does_not_expose_token() {
        let token = AuthToken::generate();
        let debug = format!("{:?}", token);

        // Should contain REDACTED, not the actual token
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains(token.as_str()));
    }

    #[test]
    fn test_clone_preserves_validation() {
        let token = AuthToken::generate();
        let cloned = token.clone();
        let token_str = token.as_str().to_string();

        assert!(cloned.validate(&token_str));
        assert_eq!(token.as_str(), cloned.as_str());
    }

    #[test]
    fn test_from_string_creates_valid_token() {
        let custom_token = "my-custom-test-token".to_string();
        let token = AuthToken::from_string(custom_token.clone());

        assert_eq!(token.as_str(), "my-custom-test-token");
        assert!(token.validate("my-custom-test-token"));
        assert!(!token.validate("wrong-token"));
    }

    #[test]
    fn test_token_length_is_reasonable() {
        let token = AuthToken::generate();
        // Base64 of 32 bytes is approximately 43 characters
        let len = token.as_str().len();
        assert!(len >= 40 && len <= 50, "Token length {} is unexpected", len);
    }
}
