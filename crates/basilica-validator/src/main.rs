#![allow(dead_code)]

//! # Basilca Validator
//!
//! Bittensor neuron for verifying and scoring miners/executors.

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    // Initialize tracing with structured fields
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(
            tracing_subscriber::fmt::layer(), // .with_target(true)
                                              // .with_thread_ids(true)
                                              // .with_thread_names(true)
                                              // .with_file(true)
                                              // .with_line_number(true),
        )
        .init();

    let args = Args::parse();

    args.run().await
}
