use basilica_validator::rental::types::RentalState;
use clap::Subcommand;
use std::path::PathBuf;

/// Main CLI commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage CLI configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show wallet information and balance
    Wallet {
        /// Optional wallet name to use (overrides default from config)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// List available GPU resources
    Ls {
        #[command(flatten)]
        filters: ListFilters,
    },

    /// Provision and start GPU instances
    Up {
        /// Target executor UID/HUID
        target: String,

        #[command(flatten)]
        options: UpOptions,
    },

    /// List active rentals and their status
    Ps {
        #[command(flatten)]
        filters: PsFilters,
    },

    /// Check instance status
    Status {
        /// Rental UID/HUID
        target: String,
    },

    /// View instance logs
    Logs {
        /// Rental UID/HUID
        target: String,

        #[command(flatten)]
        options: LogsOptions,
    },

    /// Terminate instances
    Down {
        /// Rental UID/HUID to terminate
        targets: Vec<String>,
    },

    /// Execute commands on instances
    Exec {
        /// Rental UID/HUID
        target: String,

        /// Command to execute
        command: String,
    },

    /// SSH into instances
    Ssh {
        /// Rental UID/HUID
        target: String,

        #[command(flatten)]
        options: SshOptions,
    },

    /// Copy files to/from instances
    Cp {
        /// Source path (local or remote)
        source: String,

        /// Destination path (local or remote)
        destination: String,
    },

    /// Run validator (delegates to basilica-validator)
    #[command(disable_help_flag = true)]
    Validator {
        /// Arguments to pass to basilica-validator
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run miner (delegates to basilica-miner)
    #[command(disable_help_flag = true)]
    Miner {
        /// Arguments to pass to basilica-miner
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run executor (delegates to basilica-executor)
    #[command(disable_help_flag = true)]
    Executor {
        /// Arguments to pass to basilica-executor
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Log in to Basilica
    Login {
        /// Use device authorization flow (for WSL, SSH, containers)
        #[arg(long)]
        device_code: bool,
    },

    /// Log out of Basilica
    Logout,

    /// Test authentication token
    #[cfg(debug_assertions)]
    TestAuth {
        /// Test against Basilica API instead of Auth0
        #[arg(long)]
        api: bool,
    },
}

/// Configuration management actions
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current configuration
    Show,

    /// Set configuration value
    Set {
        /// Configuration key
        key: String,

        /// Configuration value
        value: String,
    },

    /// Get configuration value
    Get {
        /// Configuration key
        key: String,
    },

    /// Reset configuration to defaults
    Reset,
}

/// Filters for listing GPUs
#[derive(clap::Args, Debug)]
pub struct ListFilters {
    /// Minimum GPU count
    #[arg(long)]
    pub gpu_min: Option<u32>,

    /// Maximum GPU count
    #[arg(long)]
    pub gpu_max: Option<u32>,

    /// GPU type filter (e.g., h100, a100)
    #[arg(long)]
    pub gpu_type: Option<String>,

    /// Maximum price per hour
    #[arg(long)]
    pub price_max: Option<f64>,

    /// Minimum memory in GB
    #[arg(long)]
    pub memory_min: Option<u32>,
}

/// Options for provisioning instances
#[derive(clap::Args, Debug)]
pub struct UpOptions {
    /// GPU type requirement
    #[arg(long)]
    pub gpu_type: Option<String>,

    /// Minimum GPU count
    #[arg(long)]
    pub gpu_min: Option<u32>,

    /// Docker image to run
    #[arg(long)]
    pub image: Option<String>,

    /// Environment variables (KEY=VALUE)
    #[arg(long)]
    pub env: Vec<String>,

    /// Instance name
    #[arg(long)]
    pub name: Option<String>,

    /// SSH public key file path
    #[arg(long)]
    pub ssh_key: Option<PathBuf>,

    /// Port mappings (host:container)
    #[arg(long)]
    pub ports: Vec<String>,

    /// CPU cores
    #[arg(long)]
    pub cpu_cores: Option<f64>,

    /// Memory in MB
    #[arg(long)]
    pub memory_mb: Option<i64>,

    /// Command to run
    #[arg(long)]
    pub command: Vec<String>,
}

/// Filters for listing active rentals
#[derive(clap::Args, Debug)]
pub struct PsFilters {
    /// Filter by status (defaults to 'active' if not specified)
    #[arg(long, value_enum)]
    pub status: Option<RentalState>,

    /// Filter by GPU type
    #[arg(long)]
    pub gpu_type: Option<String>,

    /// Minimum GPU count
    #[arg(long)]
    pub min_gpu_count: Option<u32>,
}

/// Options for viewing logs
#[derive(clap::Args, Debug)]
pub struct LogsOptions {
    /// Follow logs in real-time
    #[arg(short, long)]
    pub follow: bool,

    /// Number of lines to tail
    #[arg(long)]
    pub tail: Option<u32>,
}

/// Options for SSH connections
#[derive(clap::Args, Debug)]
pub struct SshOptions {
    /// Local port forwarding (local_port:remote_host:remote_port)
    #[arg(short = 'L', long)]
    pub local_forward: Vec<String>,

    /// Remote port forwarding (remote_port:local_host:local_port)
    #[arg(short = 'R', long)]
    pub remote_forward: Vec<String>,

    /// SSH command to run
    #[arg(last = true)]
    pub command: Vec<String>,
}
