//! Authentication command handlers

use crate::auth::{should_use_device_flow, CallbackServer, DeviceFlow, OAuthFlow, TokenStore};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::print_success;
use std::path::Path;
use tracing::{debug, error, info, warn};

/// Service name for token storage
const SERVICE_NAME: &str = "basilica-cli";

/// Handle login command
pub async fn handle_login(
    device_code: bool,
    _config: &CliConfig,
    _config_path: impl AsRef<Path>,
) -> Result<()> {
    debug!("Starting login process, device_code: {}", device_code);

    // Ensure config file exists (create with defaults if missing)
    CliConfig::ensure_config_exists().await?;

    // Determine which flow to use
    let use_device_flow = device_code || should_use_device_flow();

    // Create authentication configuration with available port
    let auth_config = if use_device_flow {
        // Device flow doesn't need a callback server, so port doesn't matter
        crate::config::create_auth_config_with_port(0)
    } else {
        // Find available port for OAuth callback server
        let port = CallbackServer::find_available_port()
            .map_err(|e| CliError::internal(format!("Failed to find available port: {}", e)))?;
        crate::config::create_auth_config_with_port(port)
    };

    // Initialize token store
    let token_store = TokenStore::new()
        .map_err(|e| CliError::internal(format!("Failed to initialize token store: {}", e)))?;

    let token_set = if use_device_flow {
        info!("Starting device authorization flow");

        let device_flow = DeviceFlow::new(auth_config);

        match device_flow.start_flow().await {
            Ok(tokens) => tokens,
            Err(e) => {
                error!("Device flow authentication failed: {}", e);
                return Err(CliError::internal(format!("Authentication failed: {}", e)));
            }
        }
    } else {
        info!("Starting browser-based OAuth flow");

        let mut oauth_flow = OAuthFlow::new(auth_config);

        match oauth_flow.start_flow().await {
            Ok(tokens) => tokens,
            Err(e) => {
                error!("OAuth flow authentication failed: {}", e);
                return Err(CliError::internal(format!("Authentication failed: {}", e)));
            }
        }
    };

    // Store the tokens securely
    if let Err(e) = token_store.store_tokens(SERVICE_NAME, &token_set).await {
        error!("Failed to store authentication tokens: {}", e);
        return Err(CliError::internal(format!("Failed to store tokens: {}", e)));
    }

    // Log token expiration information
    if let Some(expires_at) = token_set.expires_at {
        let duration = std::time::Duration::from_secs(
            expires_at.saturating_sub(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
        );
        info!(
            "Token expires in approximately {} minutes",
            duration.as_secs() / 60
        );
    }

    info!("Login completed successfully");
    Ok(())
}

/// Handle logout command
pub async fn handle_logout(_config: &CliConfig) -> Result<()> {
    debug!("Starting logout process");

    info!("Logging out...");

    // Initialize token store
    let token_store = TokenStore::new()
        .map_err(|e| CliError::internal(format!("Failed to initialize token store: {}", e)))?;

    // Check if user is currently logged in and get tokens for revocation
    let tokens = match token_store.get_tokens(SERVICE_NAME).await {
        Ok(Some(tokens)) => {
            info!("Found existing tokens, proceeding with logout and revocation");
            Some(tokens)
        }
        Ok(None) => {
            println!("You are not currently logged in.");
            return Ok(());
        }
        Err(e) => {
            warn!("Failed to check login status: {}", e);
            // Continue with logout anyway to ensure cleanup
            None
        }
    };

    // Attempt to revoke tokens with Auth0 before deleting locally
    if let Some(ref token_set) = tokens {
        info!("Attempting to revoke tokens with Auth0...");

        // Create auth config (port doesn't matter for revocation)
        let auth_config = crate::config::create_auth_config_with_port(0);
        let oauth_flow = OAuthFlow::new(auth_config);

        match oauth_flow.revoke_token(token_set).await {
            Ok(()) => {
                info!("Successfully revoked tokens with Auth0");
            }
            Err(e) => {
                warn!("Failed to revoke tokens with Auth0: {}", e);
                info!("Continuing with local token cleanup...");
            }
        }
    }

    // Delete stored authentication tokens
    if let Err(e) = token_store.delete_tokens(SERVICE_NAME).await {
        error!("Failed to delete stored tokens: {}", e);
        return Err(CliError::internal(format!(
            "Failed to clear authentication data: {}",
            e
        )));
    }

    print_success("Logout successful!");
    info!("All authentication tokens have been cleared");

    info!("Logout completed successfully");
    Ok(())
}
