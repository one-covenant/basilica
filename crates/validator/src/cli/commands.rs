use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    Start {
        #[arg(long)]
        config: Option<PathBuf>,
    },

    Stop,

    Status,

    GenConfig {
        #[arg(short, long, default_value = "validator.toml")]
        output: PathBuf,
    },

    /// Test SSH connection to executor machines
    Connect {
        /// SSH hostname or IP address
        #[arg(long)]
        host: Option<String>,

        /// SSH username
        #[arg(long)]
        username: Option<String>,

        /// SSH port (default: 22)
        #[arg(long)]
        port: Option<u16>,

        /// Path to SSH private key
        #[arg(long)]
        private_key: Option<PathBuf>,

        /// Connection timeout in seconds (default: 30)
        #[arg(long)]
        timeout: Option<u64>,

        /// Executor ID to connect to (alternative to host/username)
        #[arg(long)]
        executor_id: Option<String>,
    },

    /// Verify executor hardware via SSH validation protocol
    Verify {
        /// SSH hostname or IP address
        #[arg(long)]
        host: Option<String>,

        /// SSH username
        #[arg(long)]
        username: Option<String>,

        /// SSH port (default: 22)
        #[arg(long)]
        port: Option<u16>,

        /// Path to SSH private key
        #[arg(long)]
        private_key: Option<PathBuf>,

        /// Connection timeout in seconds (default: 30)
        #[arg(long)]
        timeout: Option<u64>,

        /// Executor ID to verify
        #[arg(short, long)]
        executor_id: Option<String>,

        /// Miner UID to verify all executors
        #[arg(short, long)]
        miner_uid: Option<u16>,

        /// Path to gpu-attestor binary
        #[arg(long)]
        gpu_attestor_path: Option<PathBuf>,

        /// Remote working directory (default: /tmp/basilica_validation)
        #[arg(long)]
        remote_work_dir: Option<String>,

        /// Execution timeout in seconds (default: 300)
        #[arg(long)]
        execution_timeout: Option<u64>,

        /// Skip cleanup after verification
        #[arg(long)]
        skip_cleanup: bool,

        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Legacy verification command (deprecated)
    #[deprecated(note = "Use 'verify' command instead")]
    VerifyLegacy {
        #[arg(short, long)]
        miner_uid: Option<u16>,

        #[arg(short, long)]
        executor_id: Option<String>,

        #[arg(long)]
        all: bool,
    },

    Database {
        #[command(subcommand)]
        action: DatabaseAction,
    },

    /// Container rental commands
    Rental {
        #[command(subcommand)]
        action: RentalAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DatabaseAction {
    Migrate,

    Reset {
        #[arg(long)]
        confirm: bool,
    },

    Status,

    Cleanup {
        #[arg(long, default_value = "30")]
        days: u32,
    },
}

#[derive(Subcommand, Debug, Clone)]
#[allow(dead_code, unused_imports)]
pub enum RentalAction {
    /// Start a new container rental
    Start {
        /// Miner ID or endpoint
        #[arg(long)]
        miner: String,

        /// Executor ID
        #[arg(long)]
        executor: String,

        /// Container image
        #[arg(long)]
        container: String,

        /// Port mappings (format: host:container:protocol)
        #[arg(long)]
        ports: Vec<String>,

        /// Environment variables (format: KEY=VALUE)
        #[arg(long)]
        env: Vec<String>,

        /// SSH public key path
        #[arg(long)]
        ssh_key: PathBuf,

        /// Command to run in container
        #[arg(long)]
        command: Option<String>,

        /// Rental duration in hours
        #[arg(long, default_value = "1")]
        duration_hours: u32,

        /// CPU cores
        #[arg(long)]
        cpu_cores: Option<f64>,

        /// Memory in MB
        #[arg(long)]
        memory_mb: Option<i64>,

        /// GPU count
        #[arg(long)]
        gpu_count: Option<u32>,
    },

    /// Get rental status
    Status {
        /// Rental ID
        #[arg(long)]
        id: String,
    },

    /// Stream rental logs
    Logs {
        /// Rental ID
        #[arg(long)]
        id: String,

        /// Follow logs
        #[arg(long)]
        follow: bool,

        /// Number of lines to tail
        #[arg(long)]
        tail: Option<u32>,
    },

    /// Stop a rental
    Stop {
        /// Rental ID
        #[arg(long)]
        id: String,

        /// Force stop
        #[arg(long)]
        force: bool,
    },
}
