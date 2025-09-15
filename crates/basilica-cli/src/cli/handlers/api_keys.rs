//! API Key management handlers for the Basilica CLI

use crate::error::CliError;
use basilica_sdk::BasilicaClient;
use console::style;
use dialoguer::Confirm;

/// Handle creating a new API key
pub async fn handle_create_key(client: &BasilicaClient, name: String) -> Result<(), CliError> {
    // Check if key already exists
    let existing_key = client
        .get_api_key()
        .await
        .map_err(|e| CliError::Api(e))?;

    if existing_key.is_some() {
        println!(
            "{}",
            style("⚠️  Note: Creating a new key will revoke your existing key.").yellow()
        );
        println!();
    }

    // Create the new API key
    let response = client
        .create_api_key(&name)
        .await
        .map_err(|e| CliError::Api(e))?;

    // Display the key with clear formatting
    println!();
    println!("{}", style("API Key created successfully!").green().bold());
    println!();
    println!("  {}: {}", style("Name").bold(), response.name);
    println!("  {}: {}", style("Created").bold(), response.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!();
    println!("{}", style("Token:").bold());
    println!("{}", style(&response.token).cyan());
    println!();
    println!("{}", style("⚠️  Save this token securely - it won't be shown again!").yellow().bold());
    println!();
    println!("To use this key:");
    println!("  export BASILICA_API_KEY=\"{}\"", response.token);
    println!();

    Ok(())
}

/// Handle showing current API key details
pub async fn handle_show_key(client: &BasilicaClient) -> Result<(), CliError> {
    let key = client
        .get_api_key()
        .await
        .map_err(|e| CliError::Api(e))?;

    if let Some(key) = key {
        println!();
        println!("{}", style("Current API Key:").bold());
        println!("  {}: {}", style("Name").bold(), key.name);
        println!(
            "  {}: {}",
            style("Created").bold(),
            key.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "  {}: {}",
            style("Last used").bold(),
            key.last_used_at
                .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "Never".to_string())
        );
        println!("  {}: {}", style("Prefix").bold(), key.prefix);
        println!();
    } else {
        println!();
        println!("No API key exists.");
        println!();
        println!("Create one with:");
        println!("  {} api-key create <name>", style("basilica").cyan());
        println!();
    }

    Ok(())
}

/// Handle revoking the API key
pub async fn handle_revoke_key(
    client: &BasilicaClient,
    skip_confirm: bool,
) -> Result<(), CliError> {
    // Check if key exists first
    let key = client
        .get_api_key()
        .await
        .map_err(|e| CliError::Api(e))?;

    if key.is_none() {
        println!();
        println!("No API key exists to revoke.");
        println!();
        return Ok(());
    }

    // Confirm revocation if not skipped
    if !skip_confirm {
        let confirmed = Confirm::new()
            .with_prompt("Are you sure you want to revoke your API key?")
            .default(false)
            .interact()
            .map_err(|e| CliError::Internal(e.into()))?;

        if !confirmed {
            println!("Revocation cancelled.");
            return Ok(());
        }
    }

    // Revoke the key
    client
        .revoke_api_key()
        .await
        .map_err(|e| CliError::Api(e))?;

    println!();
    println!("{}", style("API key revoked successfully.").green());
    println!();

    Ok(())
}