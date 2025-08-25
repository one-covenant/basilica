//! Main entry point for the Basilica CLI

use basilica_cli::cli::Args;
use clap::Parser;

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if let Err(e) = args.run().await {
        // Display message directly without "Error:" prefix
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
