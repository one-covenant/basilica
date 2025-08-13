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
    sub: String,                    // Subject - unique identifier
    name: Option<String>,
    nickname: Option<String>,
    picture: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
    updated_at: Option<String>,
}

/// Test the authentication token by calling Auth0's /userinfo endpoint
pub async fn handle_test_auth(
    config: &CliConfig,
    _config_path: impl AsRef<Path>,
) -> Result<()> {
    println!("Testing authentication token...\n");

    // Get the authenticated client
    let client = create_authenticated_client(config).await?;
    
    // Get Auth0 domain from config
    let auth_config = CliConfig::load_auth_config().await
        .map_err(|e| CliError::internal(format!("Failed to load auth config: {}", e)))?;
    
    let domain = auth_config.auth0.domain;
    let userinfo_url = format!("https://{}/userinfo", domain);
    
    debug!("Calling Auth0 userinfo endpoint: {}", userinfo_url);
    
    // Make a direct HTTP request to the userinfo endpoint
    let http_client = reqwest::Client::new();
    
    // Get the bearer token from our client
    let token = client.get_bearer_token().await
        .ok_or_else(|| CliError::internal("No authentication token found. Please run 'basilica login' first"))?;
    
    let response = http_client
        .get(&userinfo_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| CliError::internal(format!("Failed to call userinfo endpoint: {}", e)))?;
    
    let status = response.status();
    
    if status.is_success() {
        let user_info: UserInfo = response.json().await
            .map_err(|e| CliError::internal(format!("Failed to parse userinfo response: {}", e)))?;
        
        println!("Token is valid!\n");
        println!("User Information:");
        println!("─────────────────");
        println!("  ID: {}", user_info.sub);
        
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
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
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
) -> Result<()> {
    println!("Testing Basilica API authentication...\n");

    // Create authenticated client
    let client = create_authenticated_client(config).await?;
    
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