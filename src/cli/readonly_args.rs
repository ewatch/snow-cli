use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::cli::args::{
    ApiArgs, ApiCommands, AttachmentArgs, AttachmentCommands, AuthArgs, AuthCommands,
    CodesearchArgs, CodesearchCommands, Commands, ConfigArgs, ConfigCommands, DataArgs,
    DataCommands, OutputFormat, ProfileSdkArgs, ProfileSdkCommands, ScopeArgs, ScopeCommands,
    ScopeDetailLevel, ScopeListKind, TableArgs, TableCommands,
};

const READ_ONLY_AFTER_HELP: &str = "Read-only workflows:\n  1) List recent incidents\n     snow-cli-ro table list incident --query 'active=true' --limit 20\n\n  2) Fetch a record\n     snow-cli-ro table get incident <sys_id>\n\n  3) Inspect schema or app metadata\n     snow-cli-ro table schema incident --extended\n     snow-cli-ro scope inspect x_my_app\n\n  4) Call a read-oriented custom API\n     snow-cli-ro api get /api/x_myapp/status\n\nNotes:\n  - snow-cli-ro runs with a locked read-only policy.\n  - Raw API access is limited to GET.\n  - GET is allowed by HTTP convention; use read-only ServiceNow credentials for stronger guarantees.";

/// ❄️ snow-cli-ro — read-only ServiceNow CLI for agents
#[derive(Parser, Debug)]
#[command(
    name = "snow-cli-ro",
    version,
    about = "❄️ snow-cli-ro — read-only ServiceNow CLI for humans and coding agents",
    long_about = None,
    after_help = READ_ONLY_AFTER_HELP
)]
pub struct ReadOnlyCli {
    /// ServiceNow profile to use
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Override the ServiceNow instance URL
    #[arg(long, global = true)]
    pub instance: Option<String>,

    /// Output format
    #[arg(long, alias = "format", global = true, default_value = "json")]
    pub output: OutputFormat,

    /// Override the HTTP request timeout in seconds
    #[arg(long, global = true)]
    pub timeout_secs: Option<u64>,

    /// Increase verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: ReadOnlyCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyCommands {
    /// Read ServiceNow connection profiles
    #[command(alias = "config")]
    Profile(ReadOnlyProfileArgs),

    /// Authentication status operations
    Auth(ReadOnlyAuthArgs),

    /// Read Table API records and schema
    Table(ReadOnlyTableArgs),

    /// Portable data export and validation workflows
    Data(ReadOnlyDataArgs),

    /// Analyze application scope metadata
    Scope(ReadOnlyScopeArgs),

    /// Read attachment metadata or download attachments
    Attachment(ReadOnlyAttachmentArgs),

    /// Raw REST API GET calls to read-oriented endpoints
    Api(ReadOnlyApiArgs),

    /// Search code across ServiceNow instance
    Codesearch(ReadOnlyCodesearchArgs),

    /// Generate shell completions for snow-cli-ro
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Args, Debug)]
pub struct ReadOnlyProfileArgs {
    #[command(subcommand)]
    pub command: ReadOnlyProfileCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyProfileCommands {
    /// List all configured profiles
    #[command(name = "list", alias = "list-profiles")]
    ListProfiles,

    /// Find configured profiles for a ServiceNow instance name, host, or URL
    #[command(name = "find", alias = "find-profile")]
    FindProfile {
        /// Instance name, host, or URL
        #[arg(long)]
        instance: String,
    },

    /// Read now-sdk authentication aliases
    Sdk(ReadOnlyProfileSdkArgs),

    /// List saved now-sdk authentication aliases
    #[command(name = "list-now-sdk-profiles", hide = true)]
    ListNowSdkProfiles,

    /// Show the currently selected profile
    Current,

    /// Show the current active profile configuration
    Show,
}

#[derive(Args, Debug)]
pub struct ReadOnlyProfileSdkArgs {
    #[command(subcommand)]
    pub command: ReadOnlyProfileSdkCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyProfileSdkCommands {
    /// List saved now-sdk authentication aliases
    List,
}

#[derive(Args, Debug)]
pub struct ReadOnlyAuthArgs {
    #[command(subcommand)]
    pub command: ReadOnlyAuthCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyAuthCommands {
    /// Show current authentication status
    Status,
}

#[derive(Args, Debug)]
pub struct ReadOnlyTableArgs {
    #[command(subcommand)]
    pub command: ReadOnlyTableCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyTableCommands {
    /// List records from a table (auto-paginated)
    List {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,

        /// Maximum number of records to return
        #[arg(long)]
        limit: Option<usize>,

        /// Field to order results by
        #[arg(long)]
        order_by: Option<String>,
    },

    /// Get a single record by sys_id
    Get {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,
    },

    /// Show table schema (columns, types, labels) from sys_dictionary
    Schema {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Show extended field metadata
        #[arg(long)]
        extended: bool,

        /// Include fields inherited from parent tables
        #[arg(long)]
        include_inherited: bool,
    },
}

#[derive(Args, Debug)]
pub struct ReadOnlyDataArgs {
    #[command(subcommand)]
    pub command: ReadOnlyDataCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyDataCommands {
    /// Export records from a single table
    Export {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,

        /// Maximum number of records to return
        #[arg(long)]
        limit: Option<usize>,

        /// Field to order results by
        #[arg(long)]
        order_by: Option<String>,

        /// Write the exported artifact to a file instead of stdout
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,
    },

    /// Export a multi-table dataset package from a manifest spec
    ExportPackage {
        /// Dataset export spec file
        #[arg(long, short = 'f')]
        file: String,

        /// Output directory for manifest and table files
        #[arg(long)]
        out_dir: String,
    },

    /// Validate a dataset file against the target instance
    Validate {
        /// Dataset file to validate
        #[arg(long, short = 'f')]
        file: String,
    },
}

#[derive(Args, Debug)]
pub struct ReadOnlyScopeArgs {
    #[command(subcommand)]
    pub command: ReadOnlyScopeCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyScopeCommands {
    /// List scopes and classify them by origin
    List {
        /// Optional search term for partial name matches or exact scope names
        search: Option<String>,

        /// Restrict results to one or more scope kinds
        #[arg(long, value_enum)]
        kind: Vec<ScopeListKind>,

        /// Include the source table column in text output
        #[arg(long)]
        show_source_table: bool,

        /// Include the sys_id column in text output
        #[arg(long)]
        show_sys_id: bool,
    },

    /// Inspect scope metadata and artifact counts
    Inspect {
        /// Scope name (e.g., x_my_app) or scope sys_id
        scope: String,

        /// Detail level for output payload
        #[arg(long, value_enum, default_value = "basic")]
        details: ScopeDetailLevel,
    },

    /// Export normalized scope artifacts for analysis
    Inventory {
        /// Scope name (e.g., x_my_app) or scope sys_id
        scope: String,
    },
}

#[derive(Args, Debug)]
pub struct ReadOnlyAttachmentArgs {
    #[command(subcommand)]
    pub command: ReadOnlyAttachmentCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyAttachmentCommands {
    /// List attachments for a record
    List {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,
    },

    /// Download an attachment
    Download {
        /// Attachment sys_id
        sys_id: String,

        /// Output file path (defaults to original filename)
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct ReadOnlyApiArgs {
    #[command(subcommand)]
    pub command: ReadOnlyApiCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyApiCommands {
    /// Send a GET request
    Get {
        /// API path (e.g., /api/x_myapp/my_endpoint)
        path: String,

        /// Custom headers (key:value). Method override headers are blocked by policy.
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },
}

#[derive(Args, Debug)]
pub struct ReadOnlyCodesearchArgs {
    #[command(subcommand)]
    pub command: ReadOnlyCodesearchCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyCodesearchCommands {
    /// Search code across the ServiceNow instance
    Search {
        /// Search query text
        query: String,

        /// Limit to a specific table
        #[arg(long = "source-table", alias = "table")]
        source_table: Option<String>,

        /// Restrict search to a specific application scope
        #[arg(long)]
        scope: Option<String>,

        /// Maximum number of results to return
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Restrict search to the current scope only
        #[arg(long)]
        current_scope: bool,

        /// Search group to use (advanced)
        #[arg(long, default_value = "sn_devstudio.Studio Search Group")]
        search_group: String,
    },
}

impl ReadOnlyCli {
    pub fn completion_command(shell: Shell) -> crate::cli::args::Cli {
        crate::cli::args::Cli {
            profile: None,
            instance: None,
            output: OutputFormat::Json,
            timeout_secs: None,
            read_only: true,
            verbose: 0,
            command: Commands::Completions { shell },
        }
    }

    pub fn into_full_cli(self) -> crate::cli::args::Cli {
        crate::cli::args::Cli {
            profile: self.profile,
            instance: self.instance,
            output: self.output,
            timeout_secs: self.timeout_secs,
            read_only: true,
            verbose: self.verbose,
            command: self.command.into_full_command(),
        }
    }
}

impl ReadOnlyCommands {
    fn into_full_command(self) -> Commands {
        match self {
            Self::Profile(args) => Commands::Profile(ConfigArgs {
                command: args.command.into_full_command(),
            }),
            Self::Auth(args) => Commands::Auth(AuthArgs {
                command: args.command.into_full_command(),
            }),
            Self::Table(args) => Commands::Table(TableArgs {
                command: args.command.into_full_command(),
            }),
            Self::Data(args) => Commands::Data(DataArgs {
                command: args.command.into_full_command(),
            }),
            Self::Scope(args) => Commands::Scope(ScopeArgs {
                command: args.command.into_full_command(),
            }),
            Self::Attachment(args) => Commands::Attachment(AttachmentArgs {
                command: args.command.into_full_command(),
            }),
            Self::Api(args) => Commands::Api(ApiArgs {
                command: args.command.into_full_command(),
            }),
            Self::Codesearch(args) => Commands::Codesearch(CodesearchArgs {
                command: args.command.into_full_command(),
            }),
            Self::Completions { shell } => Commands::Completions { shell },
        }
    }
}

impl ReadOnlyProfileCommands {
    fn into_full_command(self) -> ConfigCommands {
        match self {
            Self::ListProfiles => ConfigCommands::ListProfiles,
            Self::FindProfile { instance } => ConfigCommands::FindProfile { instance },
            Self::Sdk(args) => ConfigCommands::Sdk(ProfileSdkArgs {
                command: match args.command {
                    ReadOnlyProfileSdkCommands::List => ProfileSdkCommands::List,
                },
            }),
            Self::ListNowSdkProfiles => ConfigCommands::ListNowSdkProfiles,
            Self::Current => ConfigCommands::Current,
            Self::Show => ConfigCommands::Show,
        }
    }
}

impl ReadOnlyAuthCommands {
    fn into_full_command(self) -> AuthCommands {
        match self {
            Self::Status => AuthCommands::Status,
        }
    }
}

impl ReadOnlyTableCommands {
    fn into_full_command(self) -> TableCommands {
        match self {
            Self::List {
                table,
                query,
                fields,
                limit,
                order_by,
            } => TableCommands::List {
                table,
                query,
                fields,
                limit,
                order_by,
            },
            Self::Get {
                table,
                sys_id,
                fields,
            } => TableCommands::Get {
                table,
                sys_id,
                fields,
            },
            Self::Schema {
                table,
                extended,
                include_inherited,
            } => TableCommands::Schema {
                table,
                extended,
                include_inherited,
            },
        }
    }
}

impl ReadOnlyDataCommands {
    fn into_full_command(self) -> DataCommands {
        match self {
            Self::Export {
                table,
                query,
                fields,
                limit,
                order_by,
                out_path,
            } => DataCommands::Export {
                table,
                query,
                fields,
                limit,
                order_by,
                out_path,
            },
            Self::ExportPackage { file, out_dir } => DataCommands::ExportPackage { file, out_dir },
            Self::Validate { file } => DataCommands::Validate { file },
        }
    }
}

impl ReadOnlyScopeCommands {
    fn into_full_command(self) -> ScopeCommands {
        match self {
            Self::List {
                search,
                kind,
                show_source_table,
                show_sys_id,
            } => ScopeCommands::List {
                search,
                kind,
                show_source_table,
                show_sys_id,
            },
            Self::Inspect { scope, details } => ScopeCommands::Inspect { scope, details },
            Self::Inventory { scope } => ScopeCommands::Inventory { scope },
        }
    }
}

impl ReadOnlyAttachmentCommands {
    fn into_full_command(self) -> AttachmentCommands {
        match self {
            Self::List { table, sys_id } => AttachmentCommands::List { table, sys_id },
            Self::Download { sys_id, out_path } => {
                AttachmentCommands::Download { sys_id, out_path }
            }
        }
    }
}

impl ReadOnlyApiCommands {
    fn into_full_command(self) -> ApiCommands {
        match self {
            Self::Get { path, header } => ApiCommands::Get { path, header },
        }
    }
}

impl ReadOnlyCodesearchCommands {
    fn into_full_command(self) -> CodesearchCommands {
        match self {
            Self::Search {
                query,
                source_table,
                scope,
                limit,
                current_scope,
                search_group,
            } => CodesearchCommands::Search {
                query,
                source_table,
                scope,
                limit,
                current_scope,
                search_group,
            },
        }
    }
}

pub fn generate_completions(shell: Shell) {
    let mut cmd = ReadOnlyCli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_assert_read_only_cli() {
        ReadOnlyCli::command().debug_assert();
    }

    #[test]
    fn help_omits_mutating_commands_and_keeps_api_get() {
        let help = ReadOnlyCli::command().render_long_help().to_string();
        assert!(help.contains("snow-cli-ro"));
        assert!(help.contains("Raw REST API GET"));
        assert!(!help.contains("Execute background scripts"));
        assert!(!help.contains("Import set operations"));
    }
}
