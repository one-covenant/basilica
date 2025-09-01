//! Authentication command handlers

use crate::config::CliConfig;
use crate::error::{CliError, Result};
use crate::output::{banner, compress_path, print_success};
use crate::progress::{complete_spinner_and_clear, complete_spinner_error, create_spinner};
use basilica_api::models::auth::AuthConfig;
use basilica_api::services::{
    is_container_runtime, is_ssh_session, is_wsl_environment, ServiceClient, ServiceClientConfig,
};
use tracing::{debug, warn};

/// Create auth config for CLI
pub fn create_auth_config_for_cli() -> AuthConfig {
    AuthConfig {
        client_id: "basilica-client".to_string(),
        auth_endpoint: "https://auth.basilica.io/oauth/authorize".to_string(),
        token_endpoint: "https://auth.basilica.io/oauth/token".to_string(),
        device_auth_endpoint: Some("https://auth.basilica.io/oauth/device/code".to_string()),
        revoke_endpoint: Some("https://auth.basilica.io/oauth/revoke".to_string()),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec![
            "deployment:manage".to_string(),
            "rental:manage".to_string(),
            "executor:read".to_string(),
        ],
        auth0_domain: Some("basilica.auth0.com".to_string()),
        auth0_audience: Some("https://api.basilica.io".to_string()),
    }
}

/// Handle login command
pub async fn handle_login(device_code: bool, config: &CliConfig) -> Result<()> {
    debug!("Starting login process, device_code: {}", device_code);

    println!(
        "{}",
        console::style("⛪ Basilica - Sacred Compute ⛪")
            .red()
            .bold()
    );
    println!();

    // Create service client configuration
    let cache_dir = CliConfig::data_dir()
        .map_err(|e| CliError::internal(format!("Failed to get cache directory: {}", e)))?;

    let service_config = ServiceClientConfig {
        auth_config: create_auth_config_for_cli(),
        ssh_config: None,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
    };

    let service_client = ServiceClient::new(service_config);
    let auth_service = service_client.auth();

    // Determine which flow to use
    let use_device_flow =
        device_code || is_wsl_environment() || is_ssh_session() || is_container_runtime();

    let spinner = if use_device_flow {
        create_spinner("Starting device authentication flow...")
    } else {
        create_spinner("Starting browser authentication flow...")
    };

    let token_set = if use_device_flow {
        // Use device flow
        spinner.set_message("Starting device authentication flow...");

        // Start device flow
        let device_auth = match auth_service.start_device_flow().await {
            Ok(device_auth) => device_auth,
            Err(e) => {
                complete_spinner_error(spinner, "Failed to start device flow");
                return Err(
                    CliError::auth(format!("Device flow initialization failed: {}", e))
                        .with_suggestion(
                            "Try 'basilica login' without --device-code for browser flow",
                        ),
                );
            }
        };

        complete_spinner_and_clear(spinner);

        // Display user instructions
        println!("Please visit the following URL to authenticate:");
        println!("  {}", device_auth.verification_uri);
        println!();
        println!("Enter this code when prompted: {}", device_auth.user_code);
        println!();

        let spinner = create_spinner("Waiting for authentication...");

        // Poll for completion
        match auth_service
            .poll_device_flow(&device_auth.device_code)
            .await
        {
            Ok(tokens) => {
                complete_spinner_and_clear(spinner);
                tokens
            }
            Err(e) => {
                complete_spinner_error(spinner, "Device authentication failed");
                return Err(
                    CliError::auth(format!("Device authentication failed: {}", e)).with_suggestion(
                        "Please ensure you completed the authentication in your browser",
                    ),
                );
            }
        }
    } else {
        // Use browser flow with callback server
        let state = uuid::Uuid::new_v4().to_string();

        // Get auth URL and PKCE challenge
        let (_auth_url, _pkce) = match auth_service.get_auth_url(&state).await {
            Ok((url, pkce)) => (url, pkce),
            Err(e) => {
                complete_spinner_error(spinner, "Failed to generate auth URL");
                return Err(CliError::auth(format!(
                    "Failed to start browser authentication: {}",
                    e
                ))
                .with_suggestion("Try 'basilica login --device-code' for device flow"));
            }
        };

        complete_spinner_and_clear(spinner);

        // For now, fall back to device flow as browser flow needs concrete implementation
        println!("Browser authentication not yet implemented with new services.");
        println!("Falling back to device flow...");
        println!();

        // Start device flow as fallback
        let device_auth = match auth_service.start_device_flow().await {
            Ok(device_auth) => device_auth,
            Err(e) => {
                return Err(CliError::auth(format!("Authentication failed: {}", e))
                    .with_suggestion("Please check your internet connection and try again"));
            }
        };

        // Display user instructions
        println!("Please visit the following URL to authenticate:");
        println!("  {}", device_auth.verification_uri);
        println!();
        println!("Enter this code when prompted: {}", device_auth.user_code);
        println!();

        let spinner = create_spinner("Waiting for authentication...");

        // Poll for completion
        match auth_service
            .poll_device_flow(&device_auth.device_code)
            .await
        {
            Ok(tokens) => {
                complete_spinner_and_clear(spinner);
                tokens
            }
            Err(e) => {
                complete_spinner_error(spinner, "Authentication failed");
                return Err(CliError::auth(format!("Authentication failed: {}", e))
                    .with_suggestion(
                        "Please ensure you completed the authentication in your browser",
                    ));
            }
        }
    };

    // Store tokens using the auth service
    let spinner = create_spinner("Storing authentication tokens...");

    if let Err(e) = auth_service.store_token(token_set).await {
        complete_spinner_error(spinner, "Failed to store tokens");
        return Err(CliError::internal(format!("Failed to store tokens: {}", e)));
    }

    complete_spinner_and_clear(spinner);

    print_success("⛪ Login successful!");
    println!();

    // Check and generate SSH keys if they don't exist
    let spinner = create_spinner("Checking SSH keys...");

    // First check if keys already exist
    if config.ssh.ssh_keys_exist() {
        complete_spinner_and_clear(spinner);
        debug!("SSH keys already exist");
    } else {
        // Keys don't exist, generate them
        spinner.set_message("Generating SSH keys for GPU rentals...");

        match crate::ssh::ensure_ssh_keys_exist(&config.ssh).await {
            Ok(()) => {
                complete_spinner_and_clear(spinner);
                // Show user where keys were generated
                print_success("🔑 SSH keys generated successfully!");
                println!("  Location: {}", compress_path(&config.ssh.key_path));
                println!();
            }
            Err(e) => {
                complete_spinner_error(spinner, "Failed to generate SSH keys");
                warn!("Failed to generate SSH keys: {}", e);
                println!();
                println!("⚠️  SSH keys could not be generated automatically.");
                println!("   You can generate them manually with:");
                println!("   ssh-keygen -f ~/.ssh/basilica_rsa");
                println!();
                // Don't fail the login, just warn
            }
        }
    }

    // Show helpful command suggestions
    banner::print_command_suggestions();

    Ok(())
}

/// Handle logout command
pub async fn handle_logout(_config: &CliConfig) -> Result<()> {
    let spinner = create_spinner("Checking authentication status...");

    // Create service client configuration
    let cache_dir = CliConfig::data_dir()
        .map_err(|e| CliError::internal(format!("Failed to get cache directory: {}", e)))?;

    let service_config = ServiceClientConfig {
        auth_config: create_auth_config_for_cli(),
        ssh_config: None,
        cache_dir: Some(cache_dir.to_string_lossy().to_string()),
    };

    let service_client = ServiceClient::new(service_config);
    let auth_service = service_client.auth();

    // Check if user is currently logged in
    let _current_token = match auth_service.get_token().await {
        Ok(Some(token_set)) => {
            complete_spinner_and_clear(spinner);
            Some(token_set)
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

    // Clear local authentication data
    let spinner = create_spinner("Clearing local authentication data...");

    // Clear tokens using auth service
    if let Err(e) = auth_service.clear_token().await {
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
