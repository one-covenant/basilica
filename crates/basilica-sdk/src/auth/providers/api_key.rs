//! API Key / Client Credentials authentication provider
//!
//! This provider implements the OAuth 2.0 client credentials grant
//! for machine-to-machine authentication without user interaction.

use crate::auth::{
    provider::AuthProvider,
    types::{AuthConfig, AuthError, AuthResult, TokenSet},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Request for client credentials grant
#[derive(Debug, Serialize)]
struct ClientCredentialsRequest {
    grant_type: String,
    client_id: String,
    client_secret: String,
    audience: String,
    scope: Option<String>,
}

/// Response from client credentials grant
#[derive(Debug, Deserialize)]
struct ClientCredentialsResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    scope: Option<String>,
}

/// API Key provider for machine-to-machine authentication
pub struct ApiKeyProvider {
    client_id: String,
    client_secret: String,
    token_endpoint: String,
    audience: String,
    scopes: Vec<String>,
}

impl ApiKeyProvider {
    /// Create a new API key provider
    pub fn new(
        client_id: String,
        client_secret: String,
        token_endpoint: String,
        audience: String,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            client_id,
            client_secret,
            token_endpoint,
            audience,
            scopes,
        }
    }

    /// Create from AuthConfig with client secret
    pub fn from_config(config: AuthConfig, client_secret: String) -> Self {
        Self::new(
            config.client_id,
            client_secret,
            config.token_endpoint,
            basilica_common::auth0_audience().to_string(),
            config.scopes,
        )
    }
}

#[async_trait]
impl AuthProvider for ApiKeyProvider {
    async fn authenticate(&self) -> AuthResult<TokenSet> {
        debug!("Authenticating with client credentials grant");
        
        let client = reqwest::Client::new();
        
        let request = ClientCredentialsRequest {
            grant_type: "client_credentials".to_string(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            audience: self.audience.clone(),
            scope: if self.scopes.is_empty() {
                None
            } else {
                Some(self.scopes.join(" "))
            },
        };
        
        let response = client
            .post(&self.token_endpoint)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(format!("Failed to request token: {}", e)))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::InvalidResponse(format!(
                "Client credentials grant failed: {}",
                error_text
            )));
        }
        
        let token_response: ClientCredentialsResponse = response
            .json()
            .await
            .map_err(|e| AuthError::InvalidResponse(format!("Failed to parse token response: {}", e)))?;
        
        // Calculate expiration time
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + token_response.expires_in;
        
        let scopes = token_response
            .scope
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_else(|| self.scopes.clone());
        
        let token_set = TokenSet::new(
            token_response.access_token,
            None, // No refresh token in client credentials flow
            token_response.token_type,
            Some(expires_at),
            scopes,
        );
        
        info!("Successfully authenticated with client credentials");
        Ok(token_set)
    }
    
    async fn get_token(&self) -> AuthResult<String> {
        // For M2M, we always authenticate fresh (tokens are typically long-lived)
        let token_set = self.authenticate().await?;
        Ok(token_set.access_token)
    }
    
    async fn refresh(&self, _refresh_token: &str) -> AuthResult<TokenSet> {
        // Client credentials don't use refresh tokens, just re-authenticate
        self.authenticate().await
    }
    
    async fn revoke(&self, _token: &str) -> AuthResult<()> {
        // Client credentials tokens typically can't be revoked
        // They expire naturally
        Ok(())
    }
    
    fn supports_refresh(&self) -> bool {
        false // Client credentials flow doesn't support refresh tokens
    }
    
    fn name(&self) -> &str {
        "API Key (Client Credentials)"
    }
}