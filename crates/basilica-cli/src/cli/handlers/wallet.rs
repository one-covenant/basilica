//! Wallet information command handlers

use crate::config::CliConfig;
use crate::error::Result;
use crate::wallet;
use dialoguer::Password;
use etcetera::{choose_base_strategy, BaseStrategy};
use std::path::Path;
use tracing::debug;

/// Handle the `wallet` command - show wallet information
pub async fn handle_wallet(config: &CliConfig, wallet_name: Option<String>) -> Result<()> {
    debug!("Showing wallet information");
    // let cache = CliCache::load().await?;

    // Use provided wallet name or fall back to config default
    let wallet_to_use = wallet_name
        .clone()
        .unwrap_or_else(|| config.wallet.default_wallet.clone());

    // Show wallet configuration (display original path with tilde for readability)
    println!("Wallet Configuration:");
    println!("   Wallet name: {wallet_to_use}");

    // Format path for display - show tilde if it's in home directory
    let display_path = if let Ok(strategy) = choose_base_strategy() {
        let home = strategy.home_dir();
        if config.wallet.base_wallet_path.starts_with(home) {
            let relative = config.wallet.base_wallet_path.strip_prefix(home).unwrap();
            format!("~/{}", relative.display())
        } else {
            config.wallet.base_wallet_path.display().to_string()
        }
    } else {
        config.wallet.base_wallet_path.display().to_string()
    };
    println!("   Wallet path: {display_path}");

    // Check if wallet exists (use the already-expanded path from config)
    if wallet::wallet_exists(&config.wallet.base_wallet_path, &wallet_to_use) {
        // Try to load wallet and get addresses
        let password = if wallet_needs_password(&config.wallet.base_wallet_path, &wallet_to_use) {
            match Password::new()
                .with_prompt("Enter wallet password")
                .interact()
            {
                Ok(pw) => Some(pw),
                Err(_) => {
                    eprintln!("Password input canceled. Aborting wallet load.");
                    return Ok(());
                }
            }
        } else {
            None
        };

        match wallet::get_wallet_addresses(
            &config.wallet.base_wallet_path,
            &wallet_to_use,
            password.as_deref(),
        ) {
            Ok(addresses) => {
                println!();
                println!("Wallet Addresses:");
                if let Some(coldkey) = addresses.coldkey {
                    println!("   Coldkey:  {coldkey}");
                } else {
                    println!("   Coldkey:  (Unable to load)");
                }
                // Only display hotkey if it exists
                if let Some(hotkey) = addresses.hotkey {
                    println!("   Hotkey:   {hotkey}");
                }
            }
            Err(e) => {
                println!();
                println!("Warning: Unable to load wallet addresses: {e}");
            }
        }
    } else {
        println!();
        println!("Warning: Wallet not found at configured path");
    }

    println!();

    Ok(())
}

/// Check if wallet needs a password (is encrypted)
fn wallet_needs_password(base_wallet_path: &Path, wallet_name: &str) -> bool {
    // Check if coldkey file exists and might be encrypted
    let coldkey_path = base_wallet_path.join(wallet_name).join("coldkey");
    if coldkey_path.exists() {
        // Try to read the file to determine if it's encrypted (JSON format)
        if let Ok(content) = std::fs::read_to_string(&coldkey_path) {
            return content.trim_start().starts_with('{');
        }
    }
    false
}
