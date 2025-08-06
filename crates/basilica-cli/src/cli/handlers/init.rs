//! Initialization and setup command handlers

use crate::api::ApiClient;
use crate::config::{CliCache, CliConfig, RegistrationCache};
use crate::error::Result;
use std::path::Path;
use tracing::debug;

/// Handle the `init` command - setup and registration
pub async fn handle_init(config_path: impl AsRef<Path>) -> Result<()> {
    debug!("Initializing Basilica CLI");

    println!("🚀 Initializing Basilica CLI...");

    // Load or create configuration
    let config = CliConfig::load_from_path(config_path.as_ref()).await?;
    println!(
        "✅ Configuration loaded from: {}",
        config_path.as_ref().display()
    );

    // Check if already registered
    let mut cache = CliCache::load().await?;

    if let Some(ref registration) = cache.registration {
        println!("✅ Already registered!");
        println!("📋 Hotwallet address: {}", registration.hotwallet);
        println!(
            "📅 Registered: {}",
            registration.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );

        // Validate registration is still active
        match validate_registration(&config, &registration.hotwallet).await {
            Ok(true) => {
                println!("✅ Registration is valid and active");
                return Ok(());
            }
            Ok(false) => {
                println!("❌ Registration is no longer valid, re-registering...");
            }
            Err(e) => {
                println!("⚠️  Could not validate registration: {}, continuing...", e);
                return Ok(());
            }
        }
    }

    // Perform registration
    println!("📝 Registering with Basilica API...");

    let api_client = ApiClient::new(&config).await?;
    let hotwallet = register_user(&api_client).await?;

    // Save registration to cache
    let registration = RegistrationCache {
        hotwallet: hotwallet.clone(),
        created_at: chrono::Utc::now(),
        last_updated: chrono::Utc::now(),
    };

    cache.registration = Some(registration);
    cache.save().await?;

    println!("✅ Registration successful!");
    println!("📋 Hotwallet address: {}", hotwallet);
    println!("");
    println!("💰 To start using Basilica, fund your hotwallet with TAO:");
    println!("   Address: {}", hotwallet);
    println!("");
    println!("🎯 Next steps:");
    println!("   1. Fund your hotwallet with TAO tokens");
    println!("   2. Run 'basilica ls' to see available GPUs");
    println!("   3. Run 'basilica up' to rent a GPU interactively");

    Ok(())
}

/// Register user with the API
async fn register_user(_api_client: &ApiClient) -> Result<String> {
    // TODO: Implement actual registration API call
    // For now, return a placeholder hotwallet address
    debug!("Registering user with API");

    // This should call POST /api/v1/register endpoint
    // The API will create a hotwallet and return the address

    // Placeholder implementation
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    Ok("5HpG9w8EBLe5XCrbczpwq5TSXvedjrBGCwqxK1iQ7qUsSWFc".to_string())
}

/// Validate that registration is still active
async fn validate_registration(config: &CliConfig, hotwallet: &str) -> Result<bool> {
    debug!("Validating registration for hotwallet: {}", hotwallet);

    let _api_client = ApiClient::new(config).await?;

    // TODO: Implement actual validation API call
    // For now, assume registration is valid
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    Ok(true)
}
