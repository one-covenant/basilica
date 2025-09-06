use basilica_validator::rental::types::RentalState;
use clap::{Subcommand, ValueHint};
use std::path::PathBuf;

/// Main CLI commands
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// List available GPU resources
    #[command(alias = "list")]
    Ls {
        #[command(flatten)]
        filters: ListFilters,
    },

    /// Provision and start GPU instances
    #[command(alias = "start")]
    Up {
        /// Target executor UID/HUID (optional)
        target: Option<String>,

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
        /// Rental UID/HUID (optional)
        target: Option<String>,
    },

    /// View instance logs
    Logs {
        /// Rental UID/HUID (optional)
        target: Option<String>,

        #[command(flatten)]
        options: LogsOptions,
    },

    /// Terminate instance
    #[command(alias = "stop")]
    Down {
        /// Rental UID/HUID to terminate (optional)
        target: Option<String>,
    },

    /// Execute commands on instances
    Exec {
        /// Command to execute
        command: String,

        /// Rental UID/HUID (optional)
        #[arg(long)]
        target: Option<String>,
    },

    /// SSH into instances
    #[command(alias = "connect")]
    Ssh {
        /// Rental UID/HUID (optional)
        target: Option<String>,

        #[command(flatten)]
        options: SshOptions,
    },

    /// Copy files to/from instances
    Cp {
        /// Source path (local or remote)
        #[arg(value_hint = ValueHint::AnyPath)]
        source: String,

        /// Destination path (local or remote)
        #[arg(value_hint = ValueHint::AnyPath)]
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

impl Commands {
    /// Check if this command requires authentication
    pub fn requires_auth(&self) -> bool {
        match self {
            // GPU rental commands require authentication
            Commands::Ls { .. }
            | Commands::Up { .. }
            | Commands::Ps { .. }
            | Commands::Status { .. }
            | Commands::Logs { .. }
            | Commands::Down { .. }
            | Commands::Exec { .. }
            | Commands::Ssh { .. }
            | Commands::Cp { .. } => true,

            // Authentication and delegation commands don't require auth
            Commands::Login { .. }
            | Commands::Logout
            | Commands::Validator { .. }
            | Commands::Miner { .. }
            | Commands::Executor { .. } => false,

            // Test auth command requires authentication
            #[cfg(debug_assertions)]
            Commands::TestAuth { .. } => true,
        }
    }
}

/// Filters for listing GPUs
#[derive(clap::Args, Debug, Clone)]
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

    /// Show detailed GPU information (full GPU names)
    #[arg(long)]
    pub detailed: bool,
}

/// Options for provisioning instances
#[derive(clap::Args, Debug, Clone)]
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
    #[arg(long, value_hint = ValueHint::FilePath)]
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

    /// Disable SSH access (faster startup)
    #[arg(long)]
    pub no_ssh: bool,

    /// Create rental in detached mode (don't auto-connect via SSH)
    #[arg(short = 'd', long)]
    pub detach: bool,

    /// Show detailed executor information (full GPU names and individual executors)
    #[arg(long)]
    pub detailed: bool,
}

/// Filters for listing active rentals
#[derive(clap::Args, Debug, Clone)]
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

    /// Show detailed GPU information (full GPU names)
    #[arg(long)]
    pub detailed: bool,
}

/// Options for viewing logs
#[derive(clap::Args, Debug, Clone)]
pub struct LogsOptions {
    /// Follow logs in real-time
    #[arg(short, long)]
    pub follow: bool,

    /// Number of lines to tail
    #[arg(long)]
    pub tail: Option<u32>,
}

/// Options for SSH connections
#[derive(clap::Args, Debug, Clone)]
pub struct SshOptions {
    /// Local port forwarding (local_port:remote_host:remote_port)
    #[arg(short = 'L', long)]
    pub local_forward: Vec<String>,

    /// Remote port forwarding (remote_port:local_host:local_port)
    #[arg(short = 'R', long)]
    pub remote_forward: Vec<String>,
}
