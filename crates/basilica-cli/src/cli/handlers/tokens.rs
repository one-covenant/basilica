//! Token management handlers for the Basilica CLI

use crate::error::CliError;
use basilica_sdk::BasilicaClient;
use console::style;
use dialoguer::{Confirm, Input, Select};

/// Handle creating a new token
pub async fn handle_create_token(client: &BasilicaClient, name: Option<String>) -> Result<(), CliError> {
    // Get name interactively if not provided
    let name = match name {
        Some(n) => n,
        None => {
            // Check existing keys to help user choose unique name
            let existing_keys = client.list_api_keys().await.map_err(CliError::Api)?;

            if !existing_keys.is_empty() {
                println!();
                println!("{}", style("Existing API keys:").bold());
                for key in &existing_keys {
                    println!("  • {}", key.name);
                }
                println!();
            }

            Input::new()
                .with_prompt("Enter a name for this API key")
                .interact_text()
                .map_err(|e| CliError::Internal(e.into()))?
        }
    };

    // Check if we're at the limit
    let existing_keys = client.list_api_keys().await.map_err(CliError::Api)?;
    if existing_keys.len() >= 10 {
        println!(
            "{}",
            style("⚠️  You have reached the maximum number of API keys (10). Please delete one first.").yellow()
        );
        return Ok(());
    } else if existing_keys.len() >= 8 {
        println!(
            "{}",
            style(format!("⚠️  You have {} API keys. Maximum allowed is 10.", existing_keys.len())).yellow()
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
    println!("  export BASILICA_API_TOKEN=\"{}\"", response.token);
    println!();

    Ok(())
}

/// Handle listing all tokens
pub async fn handle_list_tokens(client: &BasilicaClient) -> Result<(), CliError> {
    let keys = client.list_api_keys().await.map_err(CliError::Api)?;

    if keys.is_empty() {
        println!();
        println!("No API keys exist.");
        println!();
        println!("Create one with:");
        println!("  {} token create [name]", style("basilica").cyan());
        println!();
    } else {
        println!();
        println!("{}", style("API Keys:").bold());
        println!();

        for (i, key) in keys.iter().enumerate() {
            println!("  {}. {}", i + 1, style(&key.name).cyan().bold());
            println!(
                "     {}: {}",
                style("Created").dim(),
                key.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!(
                "     {}: {}",
                style("Last used").dim(),
                key.last_used_at
                    .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "Never".to_string())
            );
            if i < keys.len() - 1 {
                println!();
            }
        }
        println!();
    }

    Ok(())
}

/// Handle revoking a token
pub async fn handle_revoke_token(
    client: &BasilicaClient,
    name: Option<String>,
    skip_confirm: bool,
) -> Result<(), CliError> {
    // Get all keys
    let keys = client.list_api_keys().await.map_err(CliError::Api)?;

    if keys.is_empty() {
        println!();
        println!("No API keys exist to revoke.");
        println!();
        return Ok(());
    }

    // Determine which key to delete
    let key_name = match name {
        Some(n) => n,
        None => {
            // Interactive selection
            let options: Vec<String> = keys
                .iter()
                .map(|k| {
                    let last_used = k
                        .last_used_at
                        .map(|d| format!("last used: {}", d.format("%Y-%m-%d")))
                        .unwrap_or_else(|| "never used".to_string());
                    format!("{} ({})", k.name, last_used)
                })
                .collect();

            let selection = Select::new()
                .with_prompt("Select an API key to revoke")
                .items(&options)
                .default(0)
                .interact()
                .map_err(|e| CliError::Internal(e.into()))?;

            keys[selection].name.clone()
        }
    };

    // Confirm deletion if not skipped
    if !skip_confirm {
        let confirmed = Confirm::new()
            .with_prompt(format!("Are you sure you want to delete '{}'?", key_name))
            .default(false)
            .interact()
            .map_err(|e| CliError::Internal(e.into()))?;

        if !confirmed {
            println!("Deletion cancelled.");
            return Ok(());
        }
    }

    // Delete the specific token
    client.revoke_api_key(&key_name).await.map_err(CliError::Api)?;

    println!();
    println!("{}", style(format!("API key '{}' deleted successfully.", key_name)).green());
    println!();

    Ok(())
}
