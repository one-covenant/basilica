//! OAuth 2.0 with PKCE (Proof Key for Code Exchange) implementation
//!
//! This module implements the OAuth 2.0 authorization code flow with PKCE
//! for secure authentication without requiring client secrets.

use super::callback_server::CallbackServer;
use super::types::{AuthConfig, AuthError, AuthResult, TokenSet};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use console::{style, Term};
use crate::output::{print_info, print_success};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, AuthorizationCode, ClientId,
    CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope,
    TokenResponse, TokenUrl,
};
use rand::Rng;
use sha2::{Digest, Sha256};
use tracing::{debug, info};
use webbrowser;

/// OAuth flow implementation with PKCE support
pub struct OAuthFlow {
    config: AuthConfig,
    code_verifier: Option<String>,
    code_challenge: Option<String>,
    state: Option<String>,
}

/// Generate cryptographically secure random 32-byte verifier
pub fn generate_pkce_verifier() -> String {
    debug!("Generating PKCE code verifier");

    let mut rng = rand::thread_rng();
    let verifier_bytes: [u8; 32] = rng.gen();
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    debug!("PKCE verifier generated: {} bytes", verifier.len());
    verifier
}

/// Create SHA256 hash of verifier in base64url encoding
pub fn generate_pkce_challenge(verifier: &str) -> String {
    debug!("Generating PKCE code challenge from verifier");

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge_bytes = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(challenge_bytes);

    debug!("PKCE challenge generated: {} bytes", challenge.len());
    challenge
}

/// Generate random state parameter
pub fn generate_state() -> String {
    debug!("Generating state parameter for CSRF protection");

    let mut rng = rand::thread_rng();
    let state_bytes: [u8; 32] = rng.gen();
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    debug!("State parameter generated: {} bytes", state.len());
    state
}

impl OAuthFlow {
    /// Create a new OAuth flow instance
    pub fn new(config: AuthConfig) -> Self {
        debug!(
            "Initializing OAuth flow with client_id: {}",
            config.client_id
        );
        Self {
            config,
            code_verifier: None,
            code_challenge: None,
            state: None,
        }
    }

    /// Generate PKCE code verifier and challenge
    fn generate_pkce_pair(&mut self) -> AuthResult<()> {
        debug!("Generating PKCE verifier and challenge");

        // Generate cryptographically secure random verifier
        let verifier = generate_pkce_verifier();
        let challenge = generate_pkce_challenge(&verifier);

        self.code_verifier = Some(verifier);
        self.code_challenge = Some(challenge);

        debug!("PKCE pair generated successfully");
        Ok(())
    }

    /// Generate a secure random state parameter (instance method)
    fn generate_state_internal(&mut self) -> AuthResult<String> {
        let state = generate_state();
        self.state = Some(state.clone());
        Ok(state)
    }

    /// Build the authorization URL for the OAuth provider
    pub fn build_auth_url(&mut self) -> AuthResult<String> {
        debug!("Building authorization URL");

        // Generate PKCE parameters if not already generated
        if self.code_verifier.is_none() || self.code_challenge.is_none() {
            self.generate_pkce_pair()?;
        }

        // Generate state parameter if not already generated
        if self.state.is_none() {
            self.generate_state_internal()?;
        }

        // Create OAuth2 client
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            None, // No client secret for PKCE flow
            AuthUrl::new(self.config.auth_endpoint.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?,
            Some(
                TokenUrl::new(self.config.token_endpoint.clone()).map_err(|e| {
                    AuthError::ConfigError(format!("Invalid token endpoint: {}", e))
                })?,
            ),
        )
        .set_redirect_uri(
            RedirectUrl::new(self.config.redirect_uri.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid redirect URI: {}", e)))?,
        );

        // Build authorization request
        let pkce_verifier = PkceCodeVerifier::new(self.code_verifier.as_ref().unwrap().clone());
        let pkce_challenge = PkceCodeChallenge::from_code_verifier_sha256(&pkce_verifier);

        let mut auth_request = client
            .authorize_url(|| CsrfToken::new(self.state.as_ref().unwrap().clone()))
            .set_pkce_challenge(pkce_challenge);

        // Add scopes
        for scope in &self.config.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        // Add additional parameters
        for (key, value) in &self.config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }

        // Add audience parameter from basilica-common
        auth_request = auth_request.add_extra_param("audience", basilica_common::AUTH0_AUDIENCE);

        let (auth_url, _csrf_token) = auth_request.url();
        let url_string = auth_url.to_string();

        Ok(url_string)
    }

    /// Start the OAuth flow by opening the browser and starting callback server
    pub async fn start_flow(&mut self) -> AuthResult<TokenSet> {
        info!("Starting OAuth flow");

        // Build authorization URL (this also generates PKCE and state)
        let auth_url = self.build_auth_url()?;

        // Start callback server
        let port = CallbackServer::find_available_port()?;
        let callback_server = CallbackServer::new(port, std::time::Duration::from_secs(300));

        // Get the expected state for CSRF verification
        let expected_state = self
            .state
            .as_ref()
            .ok_or_else(|| AuthError::ConfigError("State not set for OAuth flow".to_string()))?;

        // Open browser to authorization URL
        print_info("Opening browser for sign in...");
        print_info("Browser didn't open? Use the URL below to sign in:");
        
        // Use console's style for dimmed text instead of ANSI codes
        println!("{}", style(&auth_url).dim());

        webbrowser::open(&auth_url)
            .map_err(|e| AuthError::ConfigError(format!("Failed to open browser: {}", e)))?;

        print_info("Waiting for authentication...");

        // Wait for callback with authorization code
        let callback_data = callback_server.start_and_wait(expected_state).await?;

        // Clear the last lines (including "Opening browser" and "Browser didn't open")
        let term = Term::stdout();
        term.clear_last_lines(6)
            .map_err(|e| AuthError::ConfigError(format!("Terminal error: {}", e)))?;

        print_success("Login successful!");

        // Extract the authorization code
        let code = callback_data.code.ok_or_else(|| {
            AuthError::CallbackServerError("No authorization code received".to_string())
        })?;

        // Exchange authorization code for tokens
        let token_set = self.exchange_code_for_token(&code).await?;

        info!("OAuth flow completed successfully");
        Ok(token_set)
    }

    /// Exchange authorization code for access token
    async fn exchange_code_for_token(&self, code: &str) -> AuthResult<TokenSet> {
        debug!("Exchanging authorization code for tokens");

        let code_verifier = self
            .code_verifier
            .as_ref()
            .ok_or_else(|| AuthError::PkceError("Code verifier not generated".to_string()))?;

        // Create OAuth2 client
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            None, // No client secret for PKCE flow
            AuthUrl::new(self.config.auth_endpoint.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?,
            Some(
                TokenUrl::new(self.config.token_endpoint.clone()).map_err(|e| {
                    AuthError::ConfigError(format!("Invalid token endpoint: {}", e))
                })?,
            ),
        )
        .set_redirect_uri(
            RedirectUrl::new(self.config.redirect_uri.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid redirect URI: {}", e)))?,
        );

        // Exchange code for token
        let token_response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier.clone()))
            .request_async(async_http_client)
            .await
            .map_err(|e| AuthError::NetworkError(format!("Token exchange failed: {}", e)))?;

        // Extract token information
        let access_token = token_response.access_token().secret().to_string();
        let refresh_token = token_response
            .refresh_token()
            .map(|rt| rt.secret().to_string());
        let token_type = "Bearer".to_string();
        let expires_in = token_response
            .expires_in()
            .map(|duration| duration.as_secs());
        let scopes = token_response
            .scopes()
            .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| self.config.scopes.clone());

        let token_set = TokenSet {
            access_token,
            refresh_token,
            token_type,
            expires_at: expires_in.map(|secs| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + secs
            }),
            scopes,
        };

        info!("Token exchange completed successfully");
        Ok(token_set)
    }

    /// Refresh an expired access token using the refresh token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> AuthResult<TokenSet> {
        debug!("Refreshing access token");

        // Create OAuth2 client
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            None, // No client secret for PKCE flow
            AuthUrl::new(self.config.auth_endpoint.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?,
            Some(
                TokenUrl::new(self.config.token_endpoint.clone()).map_err(|e| {
                    AuthError::ConfigError(format!("Invalid token endpoint: {}", e))
                })?,
            ),
        );

        // Refresh token
        let token_response = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(async_http_client)
            .await
            .map_err(|e| AuthError::NetworkError(format!("Token refresh failed: {}", e)))?;

        // Extract token information
        let access_token = token_response.access_token().secret().to_string();
        let new_refresh_token = token_response
            .refresh_token()
            .map(|rt| rt.secret().to_string())
            .or_else(|| Some(refresh_token.to_string())); // Keep old refresh token if new one not provided
        let token_type = "Bearer".to_string();
        let expires_in = token_response
            .expires_in()
            .map(|duration| duration.as_secs());
        let scopes = token_response
            .scopes()
            .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| self.config.scopes.clone());

        let token_set = TokenSet {
            access_token,
            refresh_token: new_refresh_token,
            token_type,
            expires_at: expires_in.map(|secs| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + secs
            }),
            scopes,
        };

        info!("Token refresh completed successfully");
        Ok(token_set)
    }

    /// Public API: Exchange authorization code for tokens (static method)
    pub async fn exchange_code_for_tokens(
        config: &AuthConfig,
        code: &str,
        code_verifier: &str,
    ) -> AuthResult<TokenSet> {
        debug!("Exchanging authorization code for tokens (static)");

        // Create OAuth2 client
        let client =
            BasicClient::new(
                ClientId::new(config.client_id.clone()),
                None, // No client secret for PKCE flow
                AuthUrl::new(config.auth_endpoint.clone())
                    .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?,
                Some(TokenUrl::new(config.token_endpoint.clone()).map_err(|e| {
                    AuthError::ConfigError(format!("Invalid token endpoint: {}", e))
                })?),
            )
            .set_redirect_uri(
                RedirectUrl::new(config.redirect_uri.clone())
                    .map_err(|e| AuthError::ConfigError(format!("Invalid redirect URI: {}", e)))?,
            );

        // Exchange code for token
        let token_response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier.to_string()))
            .request_async(async_http_client)
            .await
            .map_err(|e| AuthError::NetworkError(format!("Token exchange failed: {}", e)))?;

        // Extract token information
        let access_token = token_response.access_token().secret().to_string();
        let refresh_token = token_response
            .refresh_token()
            .map(|rt| rt.secret().to_string());
        let token_type = "Bearer".to_string();
        let expires_in = token_response
            .expires_in()
            .map(|duration| duration.as_secs());
        let scopes = token_response
            .scopes()
            .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| config.scopes.clone());

        let token_set = TokenSet {
            access_token,
            refresh_token,
            token_type,
            expires_at: expires_in.map(|secs| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + secs
            }),
            scopes,
        };

        info!("Token exchange completed successfully (static)");
        Ok(token_set)
    }
}
