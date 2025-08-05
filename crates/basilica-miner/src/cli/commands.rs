use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Executor management commands
    Executor {
        #[command(subcommand)]
        executor_cmd: ExecutorCommand,
    },
    /// Validator management commands
    Validator {
        #[command(subcommand)]
        validator_cmd: ValidatorCommand,
    },
    /// Manual executor assignment commands
    Assignment {
        #[command(subcommand)]
        assignment_cmd: AssignmentCommand,
    },
    /// Service management commands
    Service {
        #[command(subcommand)]
        service_cmd: ServiceCommand,
    },
    /// Database management commands
    Database {
        #[command(subcommand)]
        database_cmd: DatabaseCommand,
    },
    /// Configuration management commands
    Config {
        #[command(subcommand)]
        config_cmd: ConfigCommand,
    },
    /// Show miner status and statistics
    Status,
    /// Run database migrations
    Migrate,
    /// Deploy executors to remote machines
    DeployExecutors {
        /// Only show what would be deployed without actually deploying
        #[arg(long)]
        dry_run: bool,
        /// Deploy to specific machine IDs only (comma-separated)
        #[arg(long)]
        only_machines: Option<String>,
        /// Skip deployment and only check status
        #[arg(long)]
        status_only: bool,
    },
}

/// Validator management subcommands
#[derive(Subcommand, Debug)]
pub enum ValidatorCommand {
    /// List recent validator interactions
    List {
        /// Number of recent interactions to show
        #[arg(short, long, default_value = "100")]
        limit: i64,
    },

    /// Show SSH access grants for a validator
    ShowAccess {
        /// Validator hotkey
        hotkey: String,
    },
}

/// Service management subcommands
#[derive(Subcommand, Debug)]
pub enum ServiceCommand {
    /// Start the miner service
    Start,

    /// Stop the miner service
    Stop,

    /// Restart the miner service
    Restart,

    /// Show service status
    Status,

    /// Reload service configuration
    Reload,
}

/// Database management subcommands
#[derive(Subcommand, Debug)]
pub enum DatabaseCommand {
    /// Backup the database
    Backup {
        /// Backup file path
        path: String,
    },

    /// Restore database from backup
    Restore {
        /// Backup file path to restore from
        path: String,
    },

    /// Show database statistics
    Stats,

    /// Clean up old database records
    Cleanup {
        /// Number of days to keep records (default: 30)
        #[arg(short, long, default_value = "30")]
        days: u32,
    },

    /// Vacuum database to reclaim space
    Vacuum,

    /// Check database integrity
    Integrity,
}

/// Manual executor assignment subcommands
#[derive(Subcommand, Debug)]
#[command(about = "Manage executor-validator assignments")]
pub enum AssignmentCommand {
    /// Assign an executor to a validator
    #[command(long_about = r#"Assign an executor to a validator

USAGE:
    basilica assignment assign <EXECUTOR_ID> <VALIDATOR_HOTKEY> [OPTIONS]

ARGS:
    <EXECUTOR_ID>         UUID or HUID (full or prefix with min 3 chars)
    <VALIDATOR_HOTKEY>    Validator address

OPTIONS:
    -n, --notes <NOTES>   Optional notes for the assignment

EXAMPLES:
    Assign by HUID:
        $ basilica assignment assign swift-falcon-a3f2 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY

    Assign by UUID:
        $ basilica assignment assign 550e8400-e29b-41d4-a716-446655440000 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY

    With notes:
        $ basilica assignment assign swift-falcon-a3f2 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY --notes "Primary GPU node""#)]
    Assign {
        /// Executor ID to assign
        executor_id: String,
        /// Validator hotkey to assign to
        validator_hotkey: String,
        /// Optional notes for the assignment
        #[arg(short, long)]
        notes: Option<String>,
    },

    /// Remove executor assignment
    Unassign {
        /// Executor ID to unassign
        executor_id: String,
    },

    /// List current assignments
    List {
        /// Filter by validator hotkey
        #[arg(short, long)]
        validator: Option<String>,
    },

    /// Show assignment coverage statistics
    Coverage,

    /// Show validator stakes
    Stakes {
        /// Minimum stake threshold in TAO
        #[arg(long)]
        min_stake: Option<f64>,
    },

    /// Get assignment suggestions
    Suggest {
        /// Minimum coverage target (0.0 - 1.0)
        #[arg(long, default_value = "0.5")]
        min_coverage: f64,
    },

    /// Export assignments to JSON file
    Export {
        /// Output file path
        path: String,
    },

    /// Import assignments from JSON file
    Import {
        /// Input file path
        path: String,
        /// Perform dry run without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Show assignment history
    History {
        /// Filter by executor ID
        #[arg(short, long)]
        executor_id: Option<String>,
        /// Maximum number of records to show
        #[arg(short, long, default_value = "20")]
        limit: i64,
    },
}

/// Configuration management subcommands
#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Validate configuration file
    Validate {
        /// Configuration file path to validate (default: current config)
        #[arg(short, long)]
        path: Option<String>,
    },

    /// Show current configuration
    Show {
        /// Show sensitive fields (default: masked)
        #[arg(long)]
        show_sensitive: bool,
    },

    /// Reload configuration (test only)
    Reload,

    /// Compare configurations
    Diff {
        /// Path to configuration file to compare with
        other_path: String,
    },

    /// Export configuration in different formats
    Export {
        /// Export format (toml, json, yaml)
        #[arg(short, long, default_value = "toml")]
        format: String,
        /// Output file path
        path: String,
    },
}

/// Executor management subcommands
#[derive(Subcommand, Debug)]
#[command(about = "Manage executors with UUID/HUID dual identifier support")]
#[command(long_about = r#"Executor Management with Dual Identifier System

Basilica uses a dual identifier system for executors:

1. UUID (Universally Unique Identifier)
   - Format: 550e8400-e29b-41d4-a716-446655440000
   - Used internally for data integrity
   - Guaranteed globally unique
   - Ideal for scripts and automation

2. HUID (Human-Unique Identifier)
   - Format: adjective-noun-hex (e.g., swift-falcon-a3f2)
   - User-friendly and memorable
   - Used in CLI output by default
   - Easy to communicate verbally

USAGE:
You can use either identifier in commands:
- Full UUID: basilica executor show 550e8400-e29b-41d4-a716-446655440000
- Full HUID: basilica executor show swift-falcon-a3f2
- UUID prefix: basilica executor show 550e8400 (min 3 chars)
- HUID prefix: basilica executor show swift-fal (min 3 chars)

PREFIX MATCHING:
Minimum 3 characters required for prefix matching.
If multiple executors match, you'll see all matches with suggestions."#)]
pub enum ExecutorCommand {
    /// List all configured executors and their health status
    #[command(long_about = r#"List all executors with optional filtering

USAGE:
    basilica executor list [OPTIONS]

OPTIONS:
    -f, --filter <FILTER>    Filter by UUID or HUID prefix (min 3 chars)
    -v, --verbose            Show UUID + HUID (default: HUID only)
    -o, --output <FORMAT>    Output format [default: table]
                            Possible values: table, json, compact, verbose

EXAMPLES:
    List all executors:
        $ basilica executor list

    Filter by prefix:
        $ basilica executor list --filter swift

    Show full details:
        $ basilica executor list --verbose

    JSON output:
        $ basilica executor list --output json"#)]
    List,

    /// Show detailed information about an executor
    #[command(long_about = r#"Show detailed information about a specific executor

USAGE:
    basilica executor show <EXECUTOR_ID> [OPTIONS]

ARGS:
    <EXECUTOR_ID>    UUID or HUID (full or prefix with min 3 chars)

OPTIONS:
    -o, --output <FORMAT>    Output format [default: text]
                            Possible values: text, json

EXAMPLES:
    Show by full HUID:
        $ basilica executor show swift-falcon-a3f2

    Show by UUID prefix:
        $ basilica executor show 550e8400

    JSON output:
        $ basilica executor show swift-falcon-a3f2 --output json"#)]
    Show {
        /// Executor ID (UUID or HUID, supports prefix matching with min 3 chars)
        #[arg(value_name = "EXECUTOR_ID")]
        executor_id: String,
    },

    /// Show health status for all executors
    Health,

    /// Restart a specific executor
    Restart {
        /// Executor ID (UUID or HUID, supports prefix matching with min 3 chars)
        #[arg(value_name = "EXECUTOR_ID")]
        executor_id: String,
    },

    /// View executor logs
    Logs {
        /// Executor ID (UUID or HUID, supports prefix matching with min 3 chars)
        #[arg(value_name = "EXECUTOR_ID")]
        executor_id: String,
        /// Follow logs in real-time
        #[arg(short, long)]
        follow: bool,
        /// Number of recent lines to show
        #[arg(short, long)]
        lines: Option<usize>,
    },

    /// Connect directly to an executor
    Connect {
        /// Executor ID (UUID or HUID, supports prefix matching with min 3 chars)
        #[arg(value_name = "EXECUTOR_ID")]
        executor_id: String,
    },

    /// Run diagnostics on an executor
    Diagnostics {
        /// Executor ID (UUID or HUID, supports prefix matching with min 3 chars)
        #[arg(value_name = "EXECUTOR_ID")]
        executor_id: String,
    },

    /// Ping an executor to test connectivity
    Ping {
        /// Executor ID (UUID or HUID, supports prefix matching with min 3 chars)
        #[arg(value_name = "EXECUTOR_ID")]
        executor_id: String,
    },
}
