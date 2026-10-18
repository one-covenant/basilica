//! Token refresh functionality
//!
//! This module provides the core token refresh logic that is shared
//! between the SDK and CLI to avoid code duplication.

use super::types::{AuthError, AuthResult, TokenSet};
use oauth2::{basic::BasicClient, AuthUrl, ClientId, RefreshToken, TokenResponse, TokenUrl};
use tracing::{debug, info};

/// Refresh an expired access token using the refresh token
///
/// This function calls the token endpoint to exchange a refresh token
/// for a new access token and refresh token pair.
///
/// # Arguments
/// * `refresh_token` - The refresh token to use for renewal
/// * `client_id` - Optional OAuth client ID (defaults to Auth0 client ID)
/// * `token_endpoint` - Optional token endpoint URL (defaults to Auth0 endpoint)
///
/// # Returns
/// A new TokenSet with fresh access and refresh tokens
pub async fn refresh_access_token(
    refresh_token: &str,
    client_id: Option<&str>,
    token_endpoint: Option<&str>,
) -> AuthResult<TokenSet> {
    debug!("Refreshing access token");

    // Use provided values or fall back to Auth0 defaults
    let client_id = client_id.unwrap_or_else(|| basilica_common::auth0_client_id());
    let token_endpoint = token_endpoint.map(|s| s.to_string()).unwrap_or_else(|| {
        let domain = basilica_common::auth0_domain();
        format!("https://{}/oauth/token", domain)
    });

    // Create OAuth2 client
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(
            AuthUrl::new(format!(
                "https://{}/authorize",
                basilica_common::auth0_domain()
            ))
            .map_err(|e| AuthError::ConfigError(format!("Invalid auth endpoint: {}", e)))?,
        )
        .set_token_uri(
            TokenUrl::new(token_endpoint.clone())
                .map_err(|e| AuthError::ConfigError(format!("Invalid token endpoint: {}", e)))?,
        );

    // Refresh token using oauth2 crate
    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AuthError::NetworkError(format!("Token client creation failed: {}", e)))?;
    let token_response = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|e| AuthError::NetworkError(format!("Token refresh failed: {}", e)))?;

    // Extract token information
    let access_token = token_response.access_token().secret().to_string();
    let new_refresh_token = token_response
        .refresh_token()
        .map(|rt| rt.secret().to_string())
        .unwrap_or_else(|| refresh_token.to_string()); // Keep old refresh token if new one not provided

    // Create TokenSet
    let token_set = TokenSet::new(access_token, new_refresh_token);

    info!("Token refresh completed successfully");
    Ok(token_set)
}

#[cfg(test)]
mod tests {
    use super::refresh_access_token;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn refresh_keeps_existing_refresh_token_when_provider_omits_rotation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_raw(
                        r#"{"access_token":"new-access","token_type":"Bearer"}"#,
                        "application/json",
                    ),
            )
            .mount(&server)
            .await;

        let tokens = refresh_access_token(
            "existing-refresh",
            Some("test-client"),
            Some(&format!("{}/token", server.uri())),
        )
        .await
        .unwrap();
        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token, "existing-refresh");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = String::from_utf8(requests[0].body.clone()).unwrap();
        assert!(body
            .split('&')
            .any(|field| field == "grant_type=refresh_token"));
        assert!(body
            .split('&')
            .any(|field| field == "client_id=test-client"));
        assert!(body
            .split('&')
            .any(|field| field == "refresh_token=existing-refresh"));
    }

    #[tokio::test]
    async fn token_client_does_not_follow_provider_redirects() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/leak", server.uri()).as_str()),
            )
            .mount(&server)
            .await;

        assert!(refresh_access_token(
            "existing-refresh",
            Some("test-client"),
            Some(&format!("{}/token", server.uri())),
        )
        .await
        .is_err());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/token");
    }
}
