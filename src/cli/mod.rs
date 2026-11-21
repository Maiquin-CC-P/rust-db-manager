use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rustdb",
    version = "0.1.0",
    about = "RustDB Manager — Multi-client database baseline, cloning, migration DAG, and schema drift comparator"
)]
pub struct Cli {
    /// Path to connections TOML file
    #[arg(
        long,
        global = true,
        default_value = "config/connections.example.toml",
        env = "RUSTDB_CONFIG"
    )]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Test database connectivity and inspect server version
    Connection {
        #[command(subcommand)]
        action: ConnectionCommands,
    },
    /// Inspect and export live database schema inventory
    Schema {
        #[command(subcommand)]
        action: SchemaCommands,
    },
    /// Validate or build clean database baselines from scripts
    Baseline {
        #[command(subcommand)]
        action: BaselineCommands,
    },
    /// Copy or clone a database (Rebuild from scripts, Physical Backup/Restore, or DACPAC Schema Clone)
    Db {
        #[command(subcommand)]
        action: DbCommands,
    },
    /// Compare two databases and classify differences (Explained vs. Cross-Client Contamination vs. Manual Drift)
    Compare {
        /// Connection profile name
        #[arg(long, default_value = "local")]
        profile: String,

        /// Source reference database name (e.g. DEMO_PROD_BASE)
        #[arg(long)]
        source: String,

        /// Target database name to evaluate (e.g. DEMO_CLIENTE_A)
        #[arg(long)]
        target: String,

        /// Optional path to catalog directory for migration & contamination attribution
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,

        /// Product ID in catalog (used to load migration manifests for drift attribution)
        #[arg(long, default_value = "sistema-erp")]
        product: String,

        /// Target client ID (e.g. cliente-a) to detect cross-client contamination
        #[arg(long)]
        client: Option<String>,

        /// Comparison engine layer (`native` sys.* catalog or `hybrid` with SqlPackage DACPAC)
        #[arg(long, value_enum, default_value_t = CompareEngineArg::Native)]
        engine: CompareEngineArg,

        /// Output report format (`markdown` or `json`)
        #[arg(long, value_enum, default_value_t = ReportFormatArg::Markdown)]
        format: ReportFormatArg,

        /// Optional file path to save the generated report
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConnectionCommands {
    /// Test connection to a configured profile
    Test {
        #[arg(long, default_value = "local")]
        profile: String,
    },
    /// List online user databases on the profile instance
    ListDbs {
        #[arg(long, default_value = "local")]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SchemaCommands {
    /// Inspect tables, columns, indexes, FKs, constraints, views, SPs, functions, triggers, and migration history
    Inspect {
        #[arg(long, default_value = "local")]
        profile: String,

        #[arg(long)]
        database: String,

        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum BaselineCommands {
    /// Validate baseline scripts, SHA-256 hashes, and client migration DAGs offline (no DB modifications)
    Validate {
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,

        #[arg(long)]
        product: String,

        #[arg(long)]
        version: String,

        /// Optional client ID to also validate its migration dependency graph and scope isolation
        #[arg(long)]
        client: Option<String>,
    },
    /// Build a clean database instance from baseline scripts and optional client migrations
    Build {
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,

        #[arg(long)]
        product: String,

        #[arg(long)]
        version: String,

        /// Optional client ID to apply client-approved migrations on top of the baseline
        #[arg(long)]
        client: Option<String>,

        #[arg(long, default_value = "local")]
        profile: String,

        #[arg(long)]
        database: String,

        /// Drop and recreate the target database if it already exists
        #[arg(long, default_value_t = false)]
        drop_if_exists: bool,

        /// Explicitly allow migrations marked with `"destructive": true`
        #[arg(long, default_value_t = false)]
        allow_destructive: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum DbCommands {
    /// Create a copy of a database using Rebuild, Physical Backup/Restore, or Schema-Clone
    Copy {
        #[arg(long, default_value = "local")]
        profile: String,

        /// Copy modality: `rebuild`, `backup-restore`, or `schema-clone`
        #[arg(long, value_enum, default_value_t = CopyModeArg::BackupRestore)]
        mode: CopyModeArg,

        /// Source database name (required for `backup-restore` and `schema-clone`)
        #[arg(long)]
        source: Option<String>,

        /// Target new database name
        #[arg(long)]
        target: String,

        /// Catalog directory (used for `rebuild` mode)
        #[arg(long, default_value = "catalog")]
        catalog: PathBuf,

        /// Product ID (used for `rebuild` mode)
        #[arg(long)]
        product: Option<String>,

        /// Baseline version (used for `rebuild` mode)
        #[arg(long)]
        version: Option<String>,

        /// Optional client ID (used for `rebuild` mode or stamping metadata)
        #[arg(long)]
        client: Option<String>,

        /// Optional directory for `.bak` files during `backup-restore`
        #[arg(long)]
        backup_dir: Option<PathBuf>,

        /// Drop and recreate target database if it already exists
        #[arg(long, default_value_t = false)]
        drop_if_exists: bool,

        /// Allow destructive migrations during `rebuild`
        #[arg(long, default_value_t = false)]
        allow_destructive: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CopyModeArg {
    Rebuild,
    BackupRestore,
    SchemaClone,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompareEngineArg {
    Native,
    Hybrid,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ReportFormatArg {
    Markdown,
    Json,
}
