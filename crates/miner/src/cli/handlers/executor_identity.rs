//! Executor identity-aware CLI handlers
//!
//! This module provides CLI command handlers that support the dual identifier
//! system (UUID + HUID) for executor management operations.

use clap::{Args, Subcommand};

/// Executor commands with UUID/HUID support
#[derive(Debug, Clone, Subcommand)]
pub enum ExecutorIdentityCommand {
    /// List executors with optional filtering
    List {
        /// Filter by UUID or HUID prefix (min 3 chars)
        #[clap(short, long)]
        filter: Option<String>,

        /// Show verbose output with UUID + HUID
        #[clap(short, long)]
        verbose: bool,

        /// Output format
        #[clap(short = 'o', long, value_enum, default_value = "table")]
        output: OutputFormat,
    },

    /// Show detailed information about an executor
    Show {
        /// Executor UUID or HUID prefix (min 3 chars)
        executor_id: String,

        /// Output format
        #[clap(short = 'o', long, value_enum, default_value = "text")]
        output: OutputFormat,
    },

    /// Assign an executor to a validator
    Assign {
        /// Executor UUID or HUID prefix (min 3 chars)
        executor_id: String,

        /// Validator address
        validator: String,
    },

    /// Remove an executor assignment
    Unassign {
        /// Executor UUID or HUID prefix (min 3 chars)
        executor_id: String,
    },

    /// Manage executor identity
    Identity(IdentitySubcommand),
}

/// Identity-specific subcommands
#[derive(Debug, Clone, Args)]
#[clap(about = "Manage executor identities")]
pub struct IdentitySubcommand {
    #[clap(subcommand)]
    pub command: IdentityOperation,
}

/// Identity operations
#[derive(Debug, Clone, Subcommand)]
pub enum IdentityOperation {
    /// Show current executor identity
    Show {
        /// Output format
        #[clap(short = 'o', long, value_enum, default_value = "text")]
        output: OutputFormat,
    },

    /// Generate a new identity (for testing)
    Generate {
        /// Number of identities to generate
        #[clap(default_value = "1")]
        count: usize,

        /// Output format
        #[clap(short = 'o', long, value_enum, default_value = "table")]
        output: OutputFormat,
    },

    /// Search for executors by identifier
    Search {
        /// Search query (UUID or HUID prefix)
        query: String,

        /// Show all matches even if ambiguous
        #[clap(short, long)]
        all: bool,
    },
}

/// Output format for commands
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable table format
    Table,
    /// Plain text format
    Text,
    /// JSON format
    Json,
    /// Compact format (HUID only)
    Compact,
    /// Verbose format (UUID + HUID)
    Verbose,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_parsing() {
        use clap::ValueEnum;

        assert_eq!(
            OutputFormat::from_str("table", false).unwrap(),
            OutputFormat::Table
        );
        assert_eq!(
            OutputFormat::from_str("json", false).unwrap(),
            OutputFormat::Json
        );
    }
}
