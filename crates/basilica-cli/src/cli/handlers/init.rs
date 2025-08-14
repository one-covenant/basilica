//! Initialization and setup command handlers

use crate::client::create_authenticated_client;
use crate::config::CliConfig;
use crate::error::Result;
use crate::output::{print_auth, print_error, print_init, print_success};
use tracing::debug;

/// Handle the `init` command - setup and authentication check
pub async fn handle_init(config: &CliConfig, no_auth: bool) -> Result<()> {
    debug!("Initializing Basilica CLI");

    print_init("Initializing Basilica CLI...");

    // Check if user is authenticated
    print_auth("Checking authentication status...");
    
    let api_client = create_authenticated_client(config, no_auth).await?;
    
    // Check if we have authentication
    if api_client.has_auth().await {
        print_success("Authentication is configured!");
        
        // Try to validate by calling health endpoint
        match api_client.health_check().await {
            Ok(health) => {
                print_success("Successfully connected to Basilica API");
                println!("   Status: {}", health.status);
                println!("   Version: {}", health.version);
            }
            Err(e) => {
                println!("⚠️  Could not connect to API: {}", e);
                println!("   Please check your network connection and try again.");
            }
        }
    } else {
        print_error("Not authenticated!");
        println!();
        println!("Please run 'basilica login' to authenticate with Auth0.");
        println!("This will allow you to access the Basilica API.");
        return Ok(());
    }

    println!();
    println!("🎯 Next steps:");
    println!("   1. Run 'basilica ls' to see available GPUs");
    println!("   2. Run 'basilica up' to rent a GPU interactively");
    println!("   3. Run 'basilica ssh <rental-id>' to connect to your rental");

    Ok(())
}

