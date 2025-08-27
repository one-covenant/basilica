use std::net::SocketAddr;
use std::path::PathBuf;

use super::Commands;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};

#[derive(Parser, Debug)]
#[command(author, version, about = "Basilica Miner - Bittensor neuron managing executor fleets", long_about = None)]
pub struct Args {
    /// Configuration file path
    #[arg(short, long, default_value = "miner.toml")]
    pub config: PathBuf,

    #[command(flatten)]
    pub verbosity: Verbosity<InfoLevel>,

    /// Enable prometheus metrics endpoint
    #[arg(long)]
    pub metrics: bool,

    /// Metrics server address
    #[arg(long, default_value = "0.0.0.0:9091")]
    pub metrics_addr: SocketAddr,

    /// Generate sample configuration file
    #[arg(long)]
    pub gen_config: bool,

    /// Subcommands for CLI operations
    #[command(subcommand)]
    pub command: Option<Commands>,
}
