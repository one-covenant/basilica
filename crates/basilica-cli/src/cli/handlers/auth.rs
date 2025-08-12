//! Authentication command handlers

use crate::auth::{AuthConfig, DeviceFlow, OAuthFlow, TokenStore, should_use_device_flow};
use crate::cli::commands::AuthAction;
use crate::config::CliConfig;
use crate::error::{CliError, Result};
use std::path::Path;
use tracing::{debug, info, warn, error};

/// Service name for token storage
const SERVICE_NAME: &str = "basilica-cli";

/// Load authentication configuration from auth.toml
async fn load_auth_config() -> Result<AuthConfig> {
    let config_dir = CliConfig::config_dir()?;
    let auth_config_path = config_dir.parent()
        .ok_or_else(|| CliError::internal("Unable to find config directory parent"))?
        .join("config")
        .join("auth.toml");
    
    if !auth_config_path.exists() {
        return Err(CliError::internal(format!(
            "Auth configuration not found at {}. Please ensure config/auth.toml exists.",
            auth_config_path.display()
        )));
    }

    let _content = tokio::fs::read_to_string(&auth_config_path)
        .await
        .map_err(|e| CliError::internal(format!(
            "Failed to read auth configuration: {}", e
        )))?;

    // For now, we'll create a basic AuthConfig with default values
    // This can be extended to parse the TOML and extract Auth0 configuration
    // TODO: Parse the TOML properly and extract OAuth configuration from auth0 section
    let auth_config = AuthConfig {
        client_id: "basilica-cli".to_string(),
        auth_endpoint: "https://your-domain.auth0.com/oauth/authorize".to_string(),
        token_endpoint: "https://your-domain.auth0.com/oauth/token".to_string(),
        device_auth_endpoint: Some("https://your-domain.auth0.com/oauth/device/code".to_string()),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()],
        additional_params: std::collections::HashMap::new(),
    };

    Ok(auth_config)
}

/// Handle authentication commands
pub async fn handle_auth(
    action: AuthAction,
    config: &CliConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    match action {
        AuthAction::Login { device_code } => handle_login(device_code, config, config_path).await,
        AuthAction::Logout => handle_logout(config, config_path).await,
    }
}

/// Handle login command
async fn handle_login(
    device_code: bool,
    _config: &CliConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    debug!("Starting login process, device_code: {}", device_code);

    // Load authentication configuration
    let auth_config = match load_auth_config().await {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load auth configuration: {}", e);
            return Err(e);
        }
    };

    // Initialize token store
    let token_store = TokenStore::new().map_err(|e| {
        CliError::internal(format!("Failed to initialize token store: {}", e))
    })?;

    // Determine which flow to use
    let use_device_flow = device_code || should_use_device_flow();

    let token_set = if use_device_flow {
        info!("Using device authorization flow");
        println!("🔐 Starting device authorization flow...");
        
        let device_flow = DeviceFlow::new(auth_config).map_err(|e| {
            CliError::internal(format!("Failed to initialize device flow: {}", e))
        })?;

        match device_flow.start_flow().await {
            Ok(tokens) => {
                println!("✅ Successfully logged in with device flow!");
                tokens
            }
            Err(e) => {
                error!("Device flow authentication failed: {}", e);
                return Err(CliError::internal(format!(
                    "Authentication failed: {}", e
                )));
            }
        }
    } else {
        info!("Using browser OAuth flow");
        println!("🔐 Starting browser-based OAuth flow...");
        
        let mut oauth_flow = OAuthFlow::new(auth_config);

        match oauth_flow.start_flow().await {
            Ok(tokens) => {
                println!("✅ Successfully logged in with browser flow!");
                tokens
            }
            Err(e) => {
                error!("OAuth flow authentication failed: {}", e);
                return Err(CliError::internal(format!(
                    "Authentication failed: {}", e
                )));
            }
        }
    };

    // Store the tokens securely
    if let Err(e) = token_store.store_tokens(SERVICE_NAME, &token_set).await {
        error!("Failed to store authentication tokens: {}", e);
        return Err(CliError::internal(format!(
            "Failed to store tokens: {}", e
        )));
    }

    // Display token information (without sensitive details)
    if let Some(expires_at) = token_set.expires_at {
        let duration = std::time::Duration::from_secs(expires_at.saturating_sub(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));
        println!("🕐 Token expires in approximately {} minutes", duration.as_secs() / 60);
    }

    info!("Login completed successfully");
    Ok(())
}

/// Handle logout command
async fn handle_logout(_config: &CliConfig, _config_path: impl AsRef<Path>) -> Result<()> {
    debug!("Starting logout process");

    println!("🔐 Logging out...");

    // Initialize token store
    let token_store = TokenStore::new().map_err(|e| {
        CliError::internal(format!("Failed to initialize token store: {}", e))
    })?;

    // Check if user is currently logged in
    match token_store.get_tokens(SERVICE_NAME).await {
        Ok(Some(_tokens)) => {
            info!("Found existing tokens, proceeding with logout");
        }
        Ok(None) => {
            println!("ℹ️  You are not currently logged in.");
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
            "Failed to clear authentication data: {}", e
        )));
    }

    // TODO: Revoke tokens with auth server if supported
    // This would require storing the auth config and implementing token revocation
    // For now, we just clear local tokens

    println!("✅ Successfully logged out!");
    info!("All authentication tokens have been cleared");
    
    info!("Logout completed successfully");
    Ok(())
}