//! Simple authentication provider for existing tokens
//!
//! This provider is used when tokens are already available (e.g., from CLI)
//! and just need to be managed by the SDK.

use super::types::{AuthConfig, AuthError, AuthResult, TokenSet};

/// Provider that uses an existing token set
pub struct TokenProvider {
    token_set: TokenSet,
    auth_config: AuthConfig,
}

impl TokenProvider {
    /// Create a provider with existing tokens and config for refresh
    pub fn with_config(token_set: TokenSet, auth_config: AuthConfig) -> Self {
        Self {
            token_set,
            auth_config,
        }
    }
}

impl TokenProvider {
    pub async fn authenticate(&self) -> AuthResult<TokenSet> {
        Ok(self.token_set.clone())
    }

    pub async fn get_token(&self) -> AuthResult<String> {
        if self.token_set.is_expired() {
            return Err(AuthError::TokenExpired);
        }
        Ok(self.token_set.access_token.clone())
    }

    pub async fn refresh(&self, refresh_token: &str) -> AuthResult<TokenSet> {
        // Use the token endpoint from auth_config to refresh
        let client = reqwest::Client::new();
        let response = client
            .post(&self.auth_config.token_endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.auth_config.client_id),
            ])
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AuthError::NetworkError(format!(
                "Failed to refresh token: {}",
                error_text
            )));
        }

        let token_response: serde_json::Value = response.json().await.map_err(|e| {
            AuthError::InvalidResponse(format!("Failed to parse token response: {}", e))
        })?;

        // Parse token response manually
        let access_token = token_response["access_token"]
            .as_str()
            .ok_or_else(|| AuthError::InvalidResponse("Missing access_token".to_string()))?
            .to_string();

        let refresh_token = token_response["refresh_token"]
            .as_str()
            .map(|s| s.to_string());

        let token_type = token_response["token_type"]
            .as_str()
            .unwrap_or("Bearer")
            .to_string();

        let expires_in = token_response["expires_in"].as_u64();

        let scopes = token_response["scope"]
            .as_str()
            .map(|s| s.split_whitespace().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        Ok(TokenSet::new(
            access_token,
            refresh_token,
            token_type,
            expires_in,
            scopes,
        ))
    }

    pub async fn revoke(&self, token: &str) -> AuthResult<()> {
        if let Some(revoke_endpoint) = &self.auth_config.revoke_endpoint {
            let client = reqwest::Client::new();
            let response = client
                .post(revoke_endpoint)
                .form(&[("token", token), ("client_id", &self.auth_config.client_id)])
                .send()
                .await
                .map_err(|e| AuthError::NetworkError(e.to_string()))?;

            if !response.status().is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return Err(AuthError::NetworkError(format!(
                    "Failed to revoke token: {}",
                    error_text
                )));
            }
        }
        Ok(())
    }

    pub fn name(&self) -> &str {
        "ExistingToken"
    }

    pub fn supports_refresh(&self) -> bool {
        true
    }
}
