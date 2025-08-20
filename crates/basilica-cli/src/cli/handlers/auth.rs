//! Authentication command handlers

use crate::auth::{should_use_device_flow, CallbackServer, DeviceFlow, OAuthFlow, TokenStore};
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::print_success;
use crate::progress::{complete_spinner_and_clear, complete_spinner_error, create_spinner};
use std::path::Path;
use tracing::{debug, warn};

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
        let spinner = create_spinner("Requesting device code...");
        let device_flow = DeviceFlow::new(auth_config);

        match device_flow.start_flow().await {
            Ok(tokens) => {
                complete_spinner_and_clear(spinner);
                tokens
            }
            Err(e) => {
                complete_spinner_error(spinner, "Authentication failed");
                return Err(CliError::auth(format!("Authentication failed: {}", e))
                    .with_suggestion(
                        "Try 'basilica login' without --device-code for browser flow",
                    ));
            }
        }
    } else {
        let mut oauth_flow = OAuthFlow::new(auth_config);

        match oauth_flow.start_flow().await {
            Ok(tokens) => tokens,
            Err(e) => {
                return Err(CliError::auth(format!("Authentication failed: {}", e))
                    .with_suggestion("Try 'basilica login --device-code' for device flow"));
            }
        }
    };

    let spinner = create_spinner("Storing authentication tokens...");

    // Store the tokens securely
    if let Err(e) = token_store.store_tokens(SERVICE_NAME, &token_set).await {
        complete_spinner_error(spinner, "Failed to store tokens");
        return Err(CliError::internal(format!("Failed to store tokens: {}", e)));
    }

    complete_spinner_and_clear(spinner);

    print_success("⛪ Login successful!");
    Ok(())
}

/// Handle logout command
pub async fn handle_logout(_config: &CliConfig) -> Result<()> {
    let spinner = create_spinner("Checking authentication status...");

    // Initialize token store
    let token_store = TokenStore::new()
        .map_err(|e| CliError::internal(format!("Failed to initialize token store: {}", e)))?;

    // Check if user is currently logged in and get tokens for revocation
    let tokens = match token_store.get_tokens(SERVICE_NAME).await {
        Ok(Some(tokens)) => {
            complete_spinner_and_clear(spinner);
            Some(tokens)
        }
        Ok(None) => {
            complete_spinner_and_clear(spinner);
            println!("You are not currently logged in.");
            return Ok(());
        }
        Err(e) => {
            warn!("Failed to check login status: {}", e);
            complete_spinner_error(spinner, "Failed to check status");
            // Continue with logout anyway to ensure cleanup
            None
        }
    };

    // Attempt to revoke tokens with Auth0 before deleting locally
    if let Some(ref token_set) = tokens {
        let spinner = create_spinner("Revoking authentication tokens...");

        // Create auth config (port doesn't matter for revocation)
        let auth_config = crate::config::create_auth_config_with_port(0);
        let oauth_flow = OAuthFlow::new(auth_config);

        match oauth_flow.revoke_token(token_set).await {
            Ok(()) => {
                complete_spinner_and_clear(spinner);
            }
            Err(e) => {
                warn!("Failed to revoke tokens with Auth0: {}", e);
                complete_spinner_error(spinner, "Token revocation failed, continuing cleanup");
            }
        }
    }

    let spinner = create_spinner("Clearing local authentication data...");

    // Delete stored authentication tokens
    if let Err(e) = token_store.delete_tokens(SERVICE_NAME).await {
        complete_spinner_error(spinner, "Failed to clear tokens");
        return Err(CliError::internal(format!(
            "Failed to clear authentication data: {}",
            e
        )));
    }

    complete_spinner_and_clear(spinner);
    print_success("⛪ Logout successful!");
    Ok(())
}
