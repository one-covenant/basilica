//! Wallet information command handlers

use crate::config::{CliCache, CliConfig};
use crate::error::Result;
use std::path::Path;
use tracing::debug;

/// Handle the `wallet` command - show wallet information
pub async fn handle_wallet(config_path: impl AsRef<Path>) -> Result<()> {
    debug!("Showing wallet information");

    let config = CliConfig::load_from_path(config_path.as_ref()).await?;
    let cache = CliCache::load().await?;

    println!("💳 Basilica Wallet Information");
    println!();

    // Show wallet configuration
    println!("📁 Wallet Configuration:");
    println!("   Default wallet: {}", config.wallet.default_wallet);
    println!("   Wallet path: {}", config.wallet.wallet_path.display());
    println!();

    // Show registration information if available
    if let Some(registration) = cache.registration {
        println!("🔗 Registration Information:");
        println!("   Hotwallet address: {}", registration.hotwallet);
        println!(
            "   Registered: {}",
            registration.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "   Last updated: {}",
            registration.last_updated.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!();

        // TODO: Query actual balance
        println!("💰 Balance:");
        println!("   TAO: Checking...");
        println!("   (Use the hotwallet address above to check balance on Bittensor explorer)");
    } else {
        println!("❌ Not registered yet.");
        println!("   Run 'basilica init' to register and create a hotwallet.");
    }

    Ok(())
}
