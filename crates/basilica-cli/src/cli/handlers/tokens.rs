//! Token management handlers for the Basilica CLI

use crate::error::CliError;
use basilica_sdk::BasilicaClient;
use console::style;
use dialoguer::Confirm;

/// Handle creating a new token
pub async fn handle_create_token(client: &BasilicaClient, name: String) -> Result<(), CliError> {
    // Check if token already exists
    let existing_key = client.get_api_key().await.map_err(CliError::Api)?;

    if existing_key.is_some() {
        println!(
            "{}",
            style("⚠️  Note: Creating a new token will revoke your existing token.").yellow()
        );
        println!();
    }

    // Create the new token
    let response = client.create_api_key(&name).await.map_err(CliError::Api)?;

    // Display the key with clear formatting
    println!();
    println!("{}", style("Token created successfully!").green().bold());
    println!();
    println!("  {}: {}", style("Name").bold(), response.name);
    println!(
        "  {}: {}",
        style("Created").bold(),
        response.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!();
    println!("{}", style("Token:").bold());
    println!("{}", style(&response.token).cyan());
    println!();
    println!(
        "{}",
        style("⚠️  Save this token securely - it won't be shown again!")
            .yellow()
            .bold()
    );
    println!();
    println!("To use this token:");
    println!("  export BASILICA_API_KEY=\"{}\"", response.token);
    println!();

    Ok(())
}

/// Handle showing current token details
pub async fn handle_show_token(client: &BasilicaClient) -> Result<(), CliError> {
    let key = client.get_api_key().await.map_err(CliError::Api)?;

    if let Some(key) = key {
        println!();
        println!("{}", style("Current Token:").bold());
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
        println!();
    } else {
        println!();
        println!("No token exists.");
        println!();
        println!("Create one with:");
        println!("  {} token create <name>", style("basilica").cyan());
        println!();
    }

    Ok(())
}

/// Handle revoking the token
pub async fn handle_revoke_token(
    client: &BasilicaClient,
    skip_confirm: bool,
) -> Result<(), CliError> {
    // Check if token exists first
    let key = client.get_api_key().await.map_err(CliError::Api)?;

    if key.is_none() {
        println!();
        println!("No token exists to revoke.");
        println!();
        return Ok(());
    }

    // Confirm revocation if not skipped
    if !skip_confirm {
        let confirmed = Confirm::new()
            .with_prompt("Are you sure you want to revoke your token?")
            .default(false)
            .interact()
            .map_err(|e| CliError::Internal(e.into()))?;

        if !confirmed {
            println!("Revocation cancelled.");
            return Ok(());
        }
    }

    // Revoke the token
    client.revoke_api_key().await.map_err(CliError::Api)?;

    println!();
    println!("{}", style("Token revoked successfully.").green());
    println!();

    Ok(())
}
