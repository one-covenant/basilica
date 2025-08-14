//! Authentication command handlers

use crate::auth::{should_use_device_flow, AuthConfig, DeviceFlow, OAuthFlow, TokenStore};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::print_success;
use std::path::Path;
use tracing::{debug, error, info, warn};

/// Service name for token storage
const SERVICE_NAME: &str = "basilica-cli";

/// Load authentication configuration
fn load_auth_config() -> AuthConfig {
    // Use the static configuration
    use crate::config::AuthConfig as ConfigAuthConfig;
    ConfigAuthConfig::to_auth_config()
}

/// Handle login command
pub async fn handle_login(
    device_code: bool,
    _config: &CliConfig,
    _config_path: impl AsRef<Path>,
) -> Result<()> {
    debug!("Starting login process, device_code: {}", device_code);

    // Load authentication configuration
    let auth_config = load_auth_config();

    // Initialize token store
    let token_store = TokenStore::new()
        .map_err(|e| CliError::internal(format!("Failed to initialize token store: {}", e)))?;

    // Determine which flow to use
    let use_device_flow = device_code || should_use_device_flow();

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

    // Check if user is currently logged in
    match token_store.get_tokens(SERVICE_NAME).await {
        Ok(Some(_tokens)) => {
            info!("Found existing tokens, proceeding with logout");
        }
        Ok(None) => {
            println!("You are not currently logged in.");
            return Ok(());
        }
        Err(e) => {
            warn!("Failed to check login status: {}", e);
            // Continue with logout anyway to ensure cleanup
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

    // TODO: Revoke tokens with auth server if supported
    // This would require storing the auth config and implementing token revocation
    // For now, we just clear local tokens

    print_success("Logout successful!");
    info!("All authentication tokens have been cleared");

    info!("Logout completed successfully");
    Ok(())
}
