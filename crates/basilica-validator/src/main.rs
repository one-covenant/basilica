#![allow(dead_code)]

//! # Basilca Validator
//!
//! Bittensor neuron for verifying and scoring miners/executors.

use anyhow::Result;
use clap::Parser;

mod api;
mod bittensor_core;
mod cli;
mod collateral;
mod config;
mod gpu;
mod journal;
mod metrics;
mod miner_prover;
mod persistence;
mod rental;
mod ssh;
mod validation;

use cli::Args;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging using the unified system with dynamic binary name
    let log_filter = format!("{}=info", env!("CARGO_BIN_NAME").replace("-", "_"));
    basilica_common::logging::init_logging(&args.verbosity, &log_filter)?;

    args.run().await
}
