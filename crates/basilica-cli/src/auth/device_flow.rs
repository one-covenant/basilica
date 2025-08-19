//! Device Authorization Grant implementation (RFC 8628)
//!
//! This module implements OAuth 2.0 Device Authorization Grant for
//! devices that lack a web browser or have limited input capabilities.

use super::types::{AuthConfig, AuthError, AuthResult, TokenSet};
use crate::output::print_info;
use console::{style, Term};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Device authorization response from the OAuth provider
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: Option<u64>,
}

/// Device authorization request
#[derive(Debug, Serialize)]
struct DeviceAuthRequest {
    client_id: String,
    scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    audience: Option<String>,
}

/// Token request for device flow
#[derive(Debug, Serialize)]
struct DeviceTokenRequest {
    grant_type: String,
    device_code: String,
    client_id: String,
}

/// Device flow polling response
#[derive(Debug, Deserialize)]
struct PollResponse {
    error: Option<String>,
    error_description: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
}

/// Device authorization flow implementation
pub struct DeviceFlow {
    config: AuthConfig,
}

impl DeviceFlow {
    /// Create a new device flow instance
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    /// Initiate device authorization flow
    pub async fn initiate_device_auth(&self) -> AuthResult<DeviceAuthResponse> {
        let device_endpoint = self.config.device_auth_endpoint.as_ref().ok_or_else(|| {
            AuthError::ConfigError("Device authorization endpoint not configured".to_string())
        })?;

        let scope = self.config.scopes.join(" ");
        let request_body = DeviceAuthRequest {
            client_id: self.config.client_id.clone(),
            scope,
            audience: Some(basilica_common::auth0_audience().to_string()),
        };

        let client = reqwest::Client::new();
        let response = client
            .post(device_endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&request_body)
            .send()
            .await
            .map_err(|e| AuthError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AuthError::InvalidResponse(format!(
                "Device auth request failed: {}",
                error_text
            )));
        }

        let auth_response: DeviceAuthResponse = response.json().await.map_err(|e| {
            AuthError::InvalidResponse(format!("Failed to parse device auth response: {}", e))
        })?;

        Ok(auth_response)
    }

    /// Display user instructions for device authorization
    pub fn display_user_instructions(&self, response: &DeviceAuthResponse) -> AuthResult<()> {
        let formatted_code = response.user_code.clone();

        println!("1. Visit: {}", style(&response.verification_uri).dim());
        println!("2. Enter code: {}", style(&formatted_code).bold());

        if let Some(complete_uri) = &response.verification_uri_complete {
            println!(
                "\n   Or visit this direct link: {}",
                style(complete_uri).dim()
            );
        }

        println!();
        print_info("Waiting for authentication...");
        print_info("Press Ctrl+C to cancel");

        Ok(())
    }

    /// Poll for device authorization completion
    pub async fn poll_for_token(
        &self,
        device_code: &str,
        interval: Duration,
    ) -> AuthResult<TokenSet> {
        let client = reqwest::Client::new();
        let mut current_interval = interval;
        let start_time = Instant::now();
        let timeout_duration = Duration::from_secs(600); // 10 minute timeout

        let request_body = DeviceTokenRequest {
            grant_type: "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            device_code: device_code.to_string(),
            client_id: self.config.client_id.clone(),
        };

        loop {
            // Check for timeout
            if start_time.elapsed() > timeout_duration {
                return Err(AuthError::Timeout);
            }

            // Wait for the specified interval
            tokio::time::sleep(current_interval).await;

            // Make the poll request
            let response = client
                .post(&self.config.token_endpoint)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .form(&request_body)
                .send()
                .await
                .map_err(|e| AuthError::NetworkError(e.to_string()))?;

            let response_text = response
                .text()
                .await
                .map_err(|e| AuthError::NetworkError(e.to_string()))?;

            match self.handle_poll_response(&response_text)? {
                Some(token_set) => return Ok(token_set),
                None => {
                    // Check if we need to slow down
                    if let Ok(poll_response) = serde_json::from_str::<PollResponse>(&response_text)
                    {
                        if poll_response.error.as_deref() == Some("slow_down") {
                            current_interval = Duration::from_secs(current_interval.as_secs() + 5);
                            print_info(&format!(
                                "Rate limited, slowing down polling interval to {} seconds",
                                current_interval.as_secs()
                            ));
                        }
                    }
                    continue;
                }
            }
        }
    }

    /// Start complete device flow
    pub async fn start_flow(&self) -> AuthResult<TokenSet> {
        // Step 1: Initiate device authorization
        let device_response = self.initiate_device_auth().await?;

        // Step 2: Display instructions to user
        self.display_user_instructions(&device_response)?;

        // Step 3: Poll for token with the specified interval (default to 5 seconds)
        let poll_interval = Duration::from_secs(device_response.interval.unwrap_or(5));
        let token_set = self
            .poll_for_token(&device_response.device_code, poll_interval)
            .await?;

        // Clear the authorization instructions using console crate
        let term = Term::stdout();
        // We need to clear about 6-8 lines depending on if there's a complete URI
        let lines_to_clear = if device_response.verification_uri_complete.is_some() {
            8
        } else {
            6
        };
        term.clear_last_lines(lines_to_clear)
            .map_err(|e| AuthError::ConfigError(format!("Terminal error: {}", e)))?;

        Ok(token_set)
    }

    /// Handle different polling responses (authorization_pending, slow_down, etc.)
    fn handle_poll_response(&self, response_body: &str) -> AuthResult<Option<TokenSet>> {
        let poll_response: PollResponse = serde_json::from_str(response_body).map_err(|e| {
            AuthError::InvalidResponse(format!("Failed to parse poll response: {}", e))
        })?;

        // Handle error responses
        if let Some(error) = &poll_response.error {
            match error.as_str() {
                "authorization_pending" => {
                    // Still waiting for user to authorize
                    return Ok(None);
                }
                "slow_down" => {
                    // Need to slow down polling - handled by caller
                    return Ok(None);
                }
                "access_denied" => {
                    return Err(AuthError::AuthorizationDenied(
                        poll_response
                            .error_description
                            .unwrap_or_else(|| "User denied authorization".to_string()),
                    ));
                }
                "expired_token" => {
                    return Err(AuthError::DeviceFlowError(
                        "Device code expired".to_string(),
                    ));
                }
                _ => {
                    return Err(AuthError::DeviceFlowError(format!(
                        "Unknown error: {} - {}",
                        error,
                        poll_response
                            .error_description
                            .unwrap_or_else(|| "No description".to_string())
                    )));
                }
            }
        }

        // Handle successful response
        if let Some(access_token) = poll_response.access_token {
            let scopes = poll_response
                .scope
                .map(|s| {
                    s.split_whitespace()
                        .map(|scope| scope.to_string())
                        .collect()
                })
                .unwrap_or_else(|| self.config.scopes.clone());

            let token_set = TokenSet::new(
                access_token,
                poll_response.refresh_token,
                poll_response
                    .token_type
                    .unwrap_or_else(|| "Bearer".to_string()),
                poll_response.expires_in,
                scopes,
            );

            Ok(Some(token_set))
        } else {
            // No error but also no access token - this shouldn't happen
            Err(AuthError::InvalidResponse(
                "Response contains neither error nor access token".to_string(),
            ))
        }
    }
}
