//! Test authentication token functionality

use crate::client::create_authenticated_client;
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};

/// Auth0 userinfo response
#[derive(Debug, Serialize, Deserialize)]
struct UserInfo {
    sub: String, // Subject - unique identifier
    name: Option<String>,
    nickname: Option<String>,
    picture: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
    updated_at: Option<String>,
}

/// Mask sensitive information in user ID (OAuth sub field)
fn mask_user_id(id: &str) -> String {
    // For OAuth providers like google-oauth2|123456789, mask the numeric ID
    if let Some(pipe_idx) = id.find('|') {
        let provider = &id[..=pipe_idx];
        let user_id = &id[pipe_idx + 1..];

        // Mask the user ID part, showing only first and last 2 chars if long enough
        let masked_id = if user_id.len() > 6 {
            format!("{}...{}", &user_id[..2], &user_id[user_id.len() - 2..])
        } else if user_id.len() > 2 {
            format!("{}...", &user_id[..1])
        } else {
            "***".to_string()
        };

        format!("{}{}", provider, masked_id)
    } else {
        // For other ID formats, mask middle portion
        if id.len() > 8 {
            format!("{}...{}", &id[..3], &id[id.len() - 3..])
        } else if id.len() > 4 {
            format!("{}...", &id[..2])
        } else {
            "***".to_string()
        }
    }
}

/// Mask email address to show only domain
fn mask_email(email: &str) -> String {
    if let Some(at_idx) = email.find('@') {
        let domain = &email[at_idx + 1..];
        format!("***@{}", domain)
    } else {
        // If it's not a valid email format, mask it entirely
        "***".to_string()
    }
}

/// Test the authentication token by calling Auth0's /userinfo endpoint
pub async fn handle_test_auth(
    config: &CliConfig,
    _config_path: impl AsRef<Path>,
    no_auth: bool,
) -> Result<()> {
    println!("Testing authentication token...\n");

    // Get the authenticated client
    let client = create_authenticated_client(config, no_auth).await?;

    // Use Auth0 domain from constants
    let userinfo_url = format!("https://{}/userinfo", basilica_common::AUTH0_DOMAIN);

    debug!("Calling Auth0 userinfo endpoint: {}", userinfo_url);

    // Make a direct HTTP request to the userinfo endpoint
    let http_client = reqwest::Client::new();

    // Get the bearer token from our client
    let token = client.get_bearer_token().await.ok_or_else(|| {
        CliError::internal("No authentication token found. Please run 'basilica login' first")
    })?;

    let response = http_client
        .get(&userinfo_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| CliError::internal(format!("Failed to call userinfo endpoint: {}", e)))?;

    let status = response.status();

    if status.is_success() {
        let user_info: UserInfo = response
            .json()
            .await
            .map_err(|e| CliError::internal(format!("Failed to parse userinfo response: {}", e)))?;

        println!("Token is valid!\n");
        println!("User Information:");
        println!("─────────────────");

        // Mask the user ID (OAuth subject)
        println!("  ID: {}", mask_user_id(&user_info.sub));

        // Display masked email if present
        if let Some(email) = &user_info.email {
            println!("  Email: {}", mask_email(email));
            if let Some(verified) = user_info.email_verified {
                println!("  Email Verified: {}", verified);
            }
        }

        if let Some(updated) = &user_info.updated_at {
            println!("  Last Updated: {}", updated);
        }

        println!("\nAuthentication is working correctly!");
        info!("Auth0 token validation successful");
    } else if status.as_u16() == 401 {
        println!("Token is invalid or expired");
        println!("\nPlease run 'basilica login' to get a new token");
        return Err(CliError::internal("Invalid or expired token"));
    } else if status.as_u16() == 429 {
        println!("Rate limited (max 5 requests per minute)");
        println!("Please wait a moment and try again");
        return Err(CliError::internal("Rate limited by Auth0"));
    } else {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        println!("Unexpected error: {}", status);
        println!("Response: {}", error_text);
        return Err(CliError::internal(format!("Unexpected error: {}", status)));
    }

    Ok(())
}

/// Test API authentication by making a request to your Basilica API
pub async fn handle_test_api_auth(
    config: &CliConfig,
    _config_path: impl AsRef<Path>,
    no_auth: bool,
) -> Result<()> {
    println!("Testing Basilica API authentication...\n");

    // Create authenticated client
    let client = create_authenticated_client(config, no_auth).await?;

    // Try to call the health endpoint (which should work even without full auth)
    match client.health_check().await {
        Ok(health) => {
            println!("Successfully connected to Basilica API");
            println!("  Status: {}", health.status);
            println!("  Version: {}", health.version);
            println!("  Timestamp: {}", health.timestamp);

            // Check if we have authentication
            if client.has_auth().await {
                println!("\nAuthentication token is configured");

                // Try to make an authenticated request (when your API supports it)
                // For now, we just confirm the token is set
                println!("  Token is ready for authenticated requests");
            } else {
                println!("\nNo authentication token configured");
                println!("  Run 'basilica login' to authenticate");
            }
        }
        Err(e) => {
            println!("Failed to connect to Basilica API");
            println!("  Error: {}", e);
            return Err(CliError::internal(format!("API connection failed: {}", e)));
        }
    }

    Ok(())
}
