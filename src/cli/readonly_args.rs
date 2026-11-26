use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::cli::args::{
    AUTH_LOGIN_AFTER_HELP, ApiArgs, ApiCommands, ApiGetArgs, AttachmentArgs, AttachmentCommands,
    AttachmentDownloadArgs, AttachmentListArgs, AuthArgs, AuthCommands, AuthLoginBaseArgs,
    AuthStatusArgs, CodesearchArgs, Commands, ConfigArgs, ConfigCommands, DATA_EXPORT_AFTER_HELP,
    DomainArgs, DomainCommands, DomainInspectArgs, DomainListArgs, FlowArgs, IncludeArgs,
    OutputFormat, PROFILE_INIT_AFTER_HELP, ProfileDeleteArgs, ProfileFindArgs, ProfileInitArgs,
    ProfileOutputArgs, ProfileSdkArgs, ProfileSdkCommands, ProfileSdkImportArgs,
    ProfileStorageArgs, ProfileStorageCommands, ProfileUpsertArgs, ProfileUseArgs,
    READ_ONLY_API_AFTER_HELP, READ_ONLY_AUTH_AFTER_HELP, READ_ONLY_DATA_AFTER_HELP,
    READ_ONLY_DATA_EXPORT_PACKAGE_AFTER_HELP, READ_ONLY_DOMAIN_AFTER_HELP,
    READ_ONLY_PROFILE_AFTER_HELP, READ_ONLY_SCOPE_AFTER_HELP, READ_ONLY_SEED_AFTER_HELP,
    READ_ONLY_SNU_AFTER_HELP, READ_ONLY_TABLE_AFTER_HELP, ReadOnlyDomainCountScope,
    SESSION_AFTER_HELP, ScopeArgs, ScopeCommands, ScopeInspectArgs, ScopeInventoryArgs,
    ScopeListArgs, SeedArgs, SeedCommands, SeedPlanArgs, SessionArgs, SessionAttachArgs,
    SessionCommands, SessionOptionsArgs, SkillArgs, SnuAppMetaArgs, SnuArgs, SnuBrokerArgs,
    SnuBrokerCommands, SnuCheckConnectionArgs, SnuCommands, SnuContextArgs, SnuContextCommands,
    SnuGetRecordArgs, SnuPageArgs, SnuPageCommands, SnuPageFormStateArgs, SnuQueryArgs,
    SnuRecordsArgs, SnuRestArgs, SnuRestCommands, SnuRestReadArgs, SnuSchemaArgs,
    SnuScreenshotArgs, SnuTimeoutArgs, TABLE_CHOICES_AFTER_HELP, TABLE_FIND_AFTER_HELP,
    TABLE_GET_AFTER_HELP, TABLE_LIST_AFTER_HELP, TABLE_STATS_AFTER_HELP, TableArgs,
    TableChoicesArgs, TableCommands, TableFindArgs, TableGetArgs, TableListArgs, TableSchemaArgs,
    TableStatsArgs,
};
use crate::models::identifiers::TableName;

const READ_ONLY_AFTER_HELP: &str = "First-time setup (standalone):\n  1) Create a profile\n     snow-cli-ro profile add default --instance https://dev123.service-now.com --auth-method basic --username admin\n\n  2) Store credentials\n     snow-cli-ro auth login --password '<password>'\n\n  3) Verify credentials and connectivity\n     snow-cli-ro auth status --verify\n\nRead-only workflows:\n  1) List recent incidents\n     snow-cli-ro table list incident --query 'active=true' --limit 20\n\n  2) Fetch a record\n     snow-cli-ro table get incident <sys_id>\n\n  3) Inspect schema or app metadata\n     snow-cli-ro table schema incident --extended\n     snow-cli-ro scope inspect x_my_app\n\n  4) Call a read-oriented custom API\n     snow-cli-ro api get /api/x_myapp/status\n\nNotes:\n  - snow-cli-ro runs with a locked read-only policy for remote access.\n  - Local profile and credential management is allowed so it can be used standalone.\n  - Remote write commands and `auth token` (credential export) are blocked.\n  - Raw API access is limited to GET.\n  - GET is allowed by HTTP convention; use read-only ServiceNow credentials for stronger guarantees.";

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

    /// Output format. When omitted, resolves via SNOW_CLI_OUTPUT, then the
    /// configured default (`config output`), then falls back to json.
    #[arg(long, alias = "format", global = true)]
    pub output: Option<OutputFormat>,

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
    /// Manage ServiceNow connection profiles (local config only)
    #[command(alias = "config")]
    Profile(ReadOnlyConfigArgs),

    /// Authentication operations (login, logout, status)
    Auth(ReadOnlyAuthArgs),

    /// Read Table API records, schema, and aggregate stats
    Table(ReadOnlyTableArgs),

    /// Discover domain separation settings, hierarchy, and table support
    Domain(ReadOnlyDomainArgs),

    /// Inspect or attach a native ServiceNow session (no switching)
    Session(ReadOnlySessionArgs),

    /// Portable data export and validation workflows
    Data(ReadOnlyDataArgs),

    /// Validate declarative test-data seed plans (read-only)
    Seed(ReadOnlySeedArgs),

    /// Analyze application scope metadata
    Scope(ReadOnlyScopeArgs),

    /// Read attachment metadata or download attachments
    Attachment(ReadOnlyAttachmentArgs),

    /// Raw REST API GET calls to read-oriented endpoints
    Api(ReadOnlyApiArgs),

    /// Search code across ServiceNow instance
    Codesearch(CodesearchArgs),

    /// Read script include source by name, with parsed extends chain and grep
    Include(IncludeArgs),

    /// Show a Flow Designer flow as a compact step tree
    Flow(FlowArgs),

    /// Read-only SN-Utils browser-session operations
    Snu(ReadOnlySnuArgs),

    /// Install agent skills from the embedded bundle, local paths, or URL-hosted manifests
    Skill(SkillArgs),

    /// Generate shell completions for snow-cli-ro
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

// --- Profile (read-only: omits credential export & vault backend mutations) ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_PROFILE_AFTER_HELP)]
pub struct ReadOnlyConfigArgs {
    #[command(subcommand)]
    pub command: ReadOnlyConfigCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyConfigCommands {
    /// First-time setup (legacy; prefer `profile add <name>`)
    #[command(after_help = PROFILE_INIT_AFTER_HELP, hide = true)]
    Init(ProfileInitArgs),

    /// Create a new profile
    Add(ProfileUpsertArgs),

    /// Edit an existing profile
    Edit(ProfileUpsertArgs),

    /// Create or update a named profile (legacy upsert)
    #[command(name = "set", aliases = ["set-profile"], hide = true)]
    SetProfile(ProfileUpsertArgs),

    /// List all configured profiles
    #[command(name = "list", alias = "list-profiles")]
    ListProfiles,

    /// Find configured profiles for a ServiceNow instance name, host, or URL
    #[command(name = "find", alias = "find-profile")]
    FindProfile(ProfileFindArgs),

    /// Interoperate with now-sdk authentication aliases (list and import only)
    Sdk(ReadOnlyProfileSdkArgs),

    /// Inspect encrypted credential vault storage status
    Storage(ReadOnlyProfileStorageArgs),

    /// List saved now-sdk authentication aliases
    #[command(name = "list-now-sdk-profiles", hide = true)]
    ListNowSdkProfiles,

    /// Import saved now-sdk aliases into snow-cli profiles
    #[command(name = "import-now-sdk", hide = true)]
    ImportNowSdk(ProfileSdkImportArgs),

    /// Set the default profile used when --profile is not provided
    #[command(name = "default", aliases = ["use", "use-profile"])]
    UseProfile(ProfileUseArgs),

    /// Show the currently selected profile
    Current,

    /// Show the current active profile configuration
    Show,

    /// Remove a named profile
    #[command(name = "remove", aliases = ["delete", "delete-profile"])]
    DeleteProfile(ProfileDeleteArgs),

    /// Show or set the default output format used when --output is not passed
    Output(ProfileOutputArgs),
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

    /// Import saved now-sdk aliases into snow-cli profiles
    Import(ProfileSdkImportArgs),
}

#[derive(Args, Debug)]
pub struct ReadOnlyProfileStorageArgs {
    #[command(subcommand)]
    pub command: ReadOnlyProfileStorageCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyProfileStorageCommands {
    /// Show the active credential storage mode and vault status (never prompts keychain)
    Status,
}

// --- Auth ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_AUTH_AFTER_HELP)]
pub struct ReadOnlyAuthArgs {
    #[command(subcommand)]
    pub command: ReadOnlyAuthCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyAuthCommands {
    /// Authenticate and store credentials in the OS keychain
    #[command(after_help = AUTH_LOGIN_AFTER_HELP)]
    Login(AuthLoginBaseArgs),

    /// Clear stored credentials for the active profile
    Logout,

    /// Show current authentication status
    Status(AuthStatusArgs),
}

// --- Domain ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_DOMAIN_AFTER_HELP)]
pub struct ReadOnlyDomainArgs {
    #[command(subcommand)]
    pub command: ReadOnlyDomainCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyDomainCommands {
    /// Read domain settings and the authenticated user's home domain
    Status,
    /// List visible domain records and parent relationships (global is a special value)
    List(DomainListArgs),
    /// Read a domain's direct children, Contains relationships, and visibility grants
    Inspect(DomainInspectArgs),
    /// Inspect inherited domain fields on a table
    Table {
        table: TableName,
        /// Count records per domain visible to this session via the Aggregate API
        #[arg(
            long,
            value_enum,
            value_name = "SCOPE",
            num_args = 0..=1,
            default_missing_value = "visible"
        )]
        counts: Option<ReadOnlyDomainCountScope>,
    },
}

// --- Session ---

#[derive(Args, Debug)]
#[command(after_help = SESSION_AFTER_HELP)]
pub struct ReadOnlySessionArgs {
    #[command(subcommand)]
    pub command: ReadOnlySessionCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySessionCommands {
    /// Inspect the current native session and all three picker contexts
    Show,
    /// List selectable context values for this session
    Options(SessionOptionsArgs),
    /// Validate and securely store an existing cookie session in a separate profile
    Attach(SessionAttachArgs),
    /// Remove local session credentials; does not log out or alter the remote session
    Forget,
}

// --- Table ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_TABLE_AFTER_HELP)]
pub struct ReadOnlyTableArgs {
    #[command(subcommand)]
    pub command: ReadOnlyTableCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyTableCommands {
    /// List records from a table (auto-paginated)
    #[command(after_help = TABLE_LIST_AFTER_HELP)]
    List(TableListArgs),

    /// Get a single record by sys_id or by name
    #[command(after_help = TABLE_GET_AFTER_HELP)]
    Get(TableGetArgs),

    /// Show table schema (columns, types, labels) from sys_dictionary
    Schema(TableSchemaArgs),

    /// Find tables whose name or label contains text (searches sys_db_object)
    #[command(after_help = TABLE_FIND_AFTER_HELP)]
    Find(TableFindArgs),

    /// List active choice values and labels for a table's fields (from sys_choice)
    #[command(after_help = TABLE_CHOICES_AFTER_HELP)]
    Choices(TableChoicesArgs),

    /// Count and aggregate records via the Aggregate API (Stats endpoint)
    #[command(after_help = TABLE_STATS_AFTER_HELP)]
    Stats(TableStatsArgs),
}

// --- Data ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_DATA_AFTER_HELP)]
pub struct ReadOnlyDataArgs {
    #[command(subcommand)]
    pub command: ReadOnlyDataCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyDataCommands {
    /// Export records from a single table
    #[command(after_help = DATA_EXPORT_AFTER_HELP)]
    Export(crate::cli::args::DataExportArgs),

    /// Export a multi-table dataset package from a manifest spec
    #[command(after_help = READ_ONLY_DATA_EXPORT_PACKAGE_AFTER_HELP)]
    ExportPackage(crate::cli::args::DataExportPackageArgs),

    /// Validate a dataset file against the target instance
    Validate(crate::cli::args::DataValidateArgs),
}

// --- Seed ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_SEED_AFTER_HELP)]
pub struct ReadOnlySeedArgs {
    #[command(subcommand)]
    pub command: ReadOnlySeedCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySeedCommands {
    /// Validate a seed spec and show the execution plan
    Plan(SeedPlanArgs),
}

// --- Scope ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_SCOPE_AFTER_HELP)]
pub struct ReadOnlyScopeArgs {
    #[command(subcommand)]
    pub command: ReadOnlyScopeCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyScopeCommands {
    /// List scopes and classify them by origin
    List(ScopeListArgs),

    /// Inspect scope metadata and artifact counts
    Inspect(ScopeInspectArgs),

    /// Export normalized scope artifacts for analysis
    Inventory(ScopeInventoryArgs),
}

// --- Attachment ---

#[derive(Args, Debug)]
pub struct ReadOnlyAttachmentArgs {
    #[command(subcommand)]
    pub command: ReadOnlyAttachmentCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyAttachmentCommands {
    /// List attachments for a record
    List(AttachmentListArgs),

    /// Download an attachment
    Download(AttachmentDownloadArgs),
}

// --- API ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_API_AFTER_HELP)]
pub struct ReadOnlyApiArgs {
    #[command(subcommand)]
    pub command: ReadOnlyApiCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlyApiCommands {
    /// Send a GET request
    Get(ApiGetArgs),
}

// --- SN-Utils ---

#[derive(Args, Debug)]
#[command(after_help = READ_ONLY_SNU_AFTER_HELP)]
pub struct ReadOnlySnuArgs {
    #[command(subcommand)]
    pub command: ReadOnlySnuCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySnuCommands {
    /// Check whether the SN-Utils bridge and browser helper are connected
    CheckConnection(SnuCheckConnectionArgs),

    /// Get SN-Utils bridge instance info
    GetInstanceInfo(SnuTimeoutArgs),

    /// Wait for /token from SN-Utils and print the received browser session metadata
    WaitToken(SnuTimeoutArgs),

    /// Query ServiceNow records through the active SN-Utils browser session
    Query(SnuQueryArgs),

    /// List available tables through the active SN-Utils browser session
    ListTables(SnuTimeoutArgs),

    /// Fetch a single record through the active SN-Utils browser session
    GetRecord(SnuGetRecordArgs),

    /// Fetch table UI metadata through the active SN-Utils browser session
    Schema(SnuSchemaArgs),

    /// Fetch a scoped application's artifacts/metadata (SN-Utils requestAppMeta)
    AppMeta(SnuAppMetaArgs),

    /// Read the current ServiceNow browser-session context
    Context(ReadOnlySnuContextArgs),

    /// Send a read-only REST request through the browser session
    Rest(ReadOnlySnuRestArgs),

    /// Read state from a live ServiceNow browser page
    Page(ReadOnlySnuPageArgs),

    /// Read Agent API record metadata
    Records(SnuRecordsArgs),

    /// Capture a browser screenshot through SN-Utils
    Screenshot(SnuScreenshotArgs),

    /// Inspect the auto-started SN-Utils broker status
    Broker(ReadOnlySnuBrokerArgs),
}

#[derive(Args, Debug)]
pub struct ReadOnlySnuContextArgs {
    #[command(subcommand)]
    pub command: ReadOnlySnuContextCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySnuContextCommands {
    /// Get the current application scope and update set through the Agent API
    Get(SnuTimeoutArgs),
}

#[derive(Args, Debug)]
pub struct ReadOnlySnuRestArgs {
    #[command(subcommand)]
    pub command: ReadOnlySnuRestCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySnuRestCommands {
    /// Send a GET request
    Get(SnuRestReadArgs),
}

#[derive(Args, Debug)]
pub struct ReadOnlySnuPageArgs {
    #[command(subcommand)]
    pub command: ReadOnlySnuPageCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySnuPageCommands {
    /// Read fields and metadata from the active form
    FormState(SnuPageFormStateArgs),
}

#[derive(Args, Debug)]
pub struct ReadOnlySnuBrokerArgs {
    #[command(subcommand)]
    pub command: ReadOnlySnuBrokerCommands,
}

#[derive(Subcommand, Debug)]
pub enum ReadOnlySnuBrokerCommands {
    /// Show broker status
    Status,
}

// --- Conversions into full CLI ---

impl ReadOnlyCli {
    pub fn completion_command(shell: Shell) -> crate::cli::args::Cli {
        crate::cli::args::Cli {
            profile: None,
            instance: None,
            output: None,
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
            Self::Domain(args) => Commands::Domain(DomainArgs {
                command: args.command.into_full_command(),
            }),
            Self::Session(args) => Commands::Session(SessionArgs {
                command: match args.command {
                    ReadOnlySessionCommands::Show => SessionCommands::Show,
                    ReadOnlySessionCommands::Options(opts) => SessionCommands::Options(opts),
                    ReadOnlySessionCommands::Attach(attach) => SessionCommands::Attach(attach),
                    ReadOnlySessionCommands::Forget => SessionCommands::Forget,
                },
            }),
            Self::Table(args) => Commands::Table(TableArgs {
                command: args.command.into_full_command(),
            }),
            Self::Data(args) => Commands::Data(crate::cli::args::DataArgs {
                command: args.command.into_full_command(),
            }),
            Self::Seed(args) => Commands::Seed(SeedArgs {
                command: match args.command {
                    ReadOnlySeedCommands::Plan(plan) => SeedCommands::Plan(plan),
                },
            }),
            Self::Scope(args) => Commands::Scope(ScopeArgs {
                command: args.command.into_full_command(),
            }),
            Self::Attachment(args) => Commands::Attachment(AttachmentArgs {
                command: args.command.into_full_command(),
            }),
            Self::Api(args) => Commands::Api(ApiArgs {
                command: match args.command {
                    ReadOnlyApiCommands::Get(get) => ApiCommands::Get(get),
                },
            }),
            Self::Codesearch(args) => Commands::Codesearch(args),
            Self::Include(args) => Commands::Include(args),
            Self::Flow(args) => Commands::Flow(args),
            Self::Snu(args) => Commands::Snu(SnuArgs {
                command: args.command.into_full_command(),
            }),
            Self::Skill(args) => Commands::Skill(args),
            Self::Completions { shell } => Commands::Completions { shell },
        }
    }
}

impl ReadOnlyConfigCommands {
    fn into_full_command(self) -> ConfigCommands {
        match self {
            Self::Init(args) => ConfigCommands::Init(args),
            Self::Add(args) => ConfigCommands::Add(args),
            Self::Edit(args) => ConfigCommands::Edit(args),
            Self::SetProfile(args) => ConfigCommands::SetProfile(args),
            Self::ListProfiles => ConfigCommands::ListProfiles,
            Self::FindProfile(args) => ConfigCommands::FindProfile(args),
            Self::Sdk(args) => ConfigCommands::Sdk(ProfileSdkArgs {
                command: match args.command {
                    ReadOnlyProfileSdkCommands::List => ProfileSdkCommands::List,
                    ReadOnlyProfileSdkCommands::Import(import) => {
                        ProfileSdkCommands::Import(import)
                    }
                },
            }),
            Self::Storage(args) => ConfigCommands::Storage(ProfileStorageArgs {
                command: match args.command {
                    ReadOnlyProfileStorageCommands::Status => ProfileStorageCommands::Status,
                },
            }),
            Self::ListNowSdkProfiles => ConfigCommands::ListNowSdkProfiles,
            Self::ImportNowSdk(args) => ConfigCommands::ImportNowSdk(args),
            Self::UseProfile(args) => ConfigCommands::UseProfile(args),
            Self::Current => ConfigCommands::Current,
            Self::Show => ConfigCommands::Show,
            Self::DeleteProfile(args) => ConfigCommands::DeleteProfile(args),
            Self::Output(args) => ConfigCommands::Output(args),
        }
    }
}

impl ReadOnlyAuthCommands {
    fn into_full_command(self) -> AuthCommands {
        match self {
            Self::Login(base) => AuthCommands::Login {
                base,
                also_now_sdk: false,
                now_sdk_alias: None,
                set_now_sdk_default: false,
            },
            Self::Logout => AuthCommands::Logout,
            Self::Status(status) => AuthCommands::Status(status),
        }
    }
}

impl ReadOnlyDomainCommands {
    fn into_full_command(self) -> DomainCommands {
        match self {
            Self::Status => DomainCommands::Status,
            Self::List(args) => DomainCommands::List(args),
            Self::Inspect(args) => DomainCommands::Inspect(args),
            Self::Table { table, counts } => DomainCommands::Table {
                table,
                counts: counts.map(Into::into),
            },
        }
    }
}

impl ReadOnlyTableCommands {
    fn into_full_command(self) -> TableCommands {
        match self {
            Self::List(args) => TableCommands::List(args),
            Self::Get(args) => TableCommands::Get(args),
            Self::Schema(args) => TableCommands::Schema(args),
            Self::Find(args) => TableCommands::Find(args),
            Self::Choices(args) => TableCommands::Choices(args),
            Self::Stats(args) => TableCommands::Stats(args),
        }
    }
}

impl ReadOnlyDataCommands {
    fn into_full_command(self) -> crate::cli::args::DataCommands {
        match self {
            Self::Export(args) => crate::cli::args::DataCommands::Export(args),
            Self::ExportPackage(args) => crate::cli::args::DataCommands::ExportPackage(args),
            Self::Validate(args) => crate::cli::args::DataCommands::Validate(args),
        }
    }
}

impl ReadOnlyScopeCommands {
    fn into_full_command(self) -> ScopeCommands {
        match self {
            Self::List(args) => ScopeCommands::List(args),
            Self::Inspect(args) => ScopeCommands::Inspect(args),
            Self::Inventory(args) => ScopeCommands::Inventory(args),
        }
    }
}

impl ReadOnlyAttachmentCommands {
    fn into_full_command(self) -> AttachmentCommands {
        match self {
            Self::List(args) => AttachmentCommands::List(args),
            Self::Download(args) => AttachmentCommands::Download(args),
        }
    }
}

impl ReadOnlySnuCommands {
    fn into_full_command(self) -> SnuCommands {
        match self {
            Self::CheckConnection(args) => SnuCommands::CheckConnection(args),
            Self::GetInstanceInfo(args) => SnuCommands::GetInstanceInfo(args),
            Self::WaitToken(args) => SnuCommands::WaitToken(args),
            Self::Query(args) => SnuCommands::Query(args),
            Self::ListTables(args) => SnuCommands::ListTables(args),
            Self::GetRecord(args) => SnuCommands::GetRecord(args),
            Self::Schema(args) => SnuCommands::Schema(args),
            Self::AppMeta(args) => SnuCommands::AppMeta(args),
            Self::Context(context) => SnuCommands::Context(SnuContextArgs {
                command: match context.command {
                    ReadOnlySnuContextCommands::Get(args) => SnuContextCommands::Get(args),
                },
            }),
            Self::Rest(rest) => SnuCommands::Rest(SnuRestArgs {
                command: match rest.command {
                    ReadOnlySnuRestCommands::Get(request) => SnuRestCommands::Get(request),
                },
            }),
            Self::Page(page) => SnuCommands::Page(SnuPageArgs {
                command: match page.command {
                    ReadOnlySnuPageCommands::FormState(args) => SnuPageCommands::FormState(args),
                },
            }),
            Self::Records(records) => SnuCommands::Records(records),
            Self::Screenshot(args) => SnuCommands::Screenshot(args),
            Self::Broker(broker) => SnuCommands::Broker(SnuBrokerArgs {
                command: match broker.command {
                    ReadOnlySnuBrokerCommands::Status => SnuBrokerCommands::Status,
                },
            }),
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
    use crate::cli::args::Cli;
    use crate::policy::{ExecutionPolicy, PolicyDecision};
    use std::collections::BTreeMap;

    #[test]
    fn debug_assert_read_only_cli() {
        ReadOnlyCli::command().debug_assert();
    }

    #[test]
    fn help_omits_mutating_commands_and_keeps_api_get() {
        let help = ReadOnlyCli::command().render_long_help().to_string();
        assert!(help.contains("snow-cli-ro"));
        assert!(help.contains("Raw REST API GET"));
        assert!(help.contains("SN-Utils"));
        assert!(!help.contains("Execute background scripts"));
        assert!(!help.contains("Import set operations"));
        assert!(!help.contains("GraphQL"));
    }

    #[test]
    fn graphql_is_rejected_by_the_read_only_grammar() {
        let error =
            ReadOnlyCli::try_parse_from(["snow-cli-ro", "graphql", "{ incident { number } }"])
                .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn table_stats_maps_to_full_command() {
        let mapped = ReadOnlyTableCommands::Stats(TableStatsArgs {
            domain: None,
            table: "incident".parse().unwrap(),
            query: Some("active=true".to_string()),
            group_by: Some("state".to_string()),
            avg: None,
            min: None,
            max: None,
            sum: None,
            having: None,
            check_fields: None,
        })
        .into_full_command();
        match mapped {
            TableCommands::Stats(stats) => {
                assert_eq!(stats.table.as_str(), "incident");
                assert_eq!(stats.query, Some("active=true".to_string()));
                assert_eq!(stats.group_by, Some("state".to_string()));
            }
            _ => panic!("Expected Table Stats command"),
        }
    }

    #[test]
    fn snu_subcommand_omits_mutating_commands_and_keeps_broker_status() {
        let help = ReadOnlyCli::command()
            .find_subcommand("snu")
            .expect("snu subcommand exists")
            .clone()
            .render_long_help()
            .to_string();
        assert!(help.contains("check-connection"));
        assert!(help.contains("query"));
        assert!(help.contains("screenshot"));
        assert!(help.contains("broker"));
        assert!(!help.contains("update-record"));
        assert!(!help.contains("delete-record"));
        assert!(!help.contains("execute-bg-script"));
        assert!(!help.contains("attachment-upload"));
        assert!(!help.contains("slash"));

        let broker_help = ReadOnlyCli::command()
            .find_subcommand("snu")
            .and_then(|snu| snu.find_subcommand("broker"))
            .expect("read-only broker subcommand exists")
            .clone()
            .render_long_help()
            .to_string();
        assert!(broker_help.contains("status"));
        assert!(!broker_help.contains("stop"));
        assert!(!broker_help.contains("clear"));
    }

    #[test]
    fn auth_login_omits_now_sdk_export_flags_in_read_only_cli() {
        let login_help = ReadOnlyCli::command()
            .find_subcommand("auth")
            .and_then(|auth| auth.find_subcommand("login"))
            .expect("read-only auth login subcommand exists")
            .clone()
            .render_long_help()
            .to_string();
        assert!(!login_help.contains("--also-now-sdk"));
        assert!(!login_help.contains("--now-sdk-alias"));
        assert!(!login_help.contains("--set-now-sdk-default"));

        assert!(
            ReadOnlyCli::try_parse_from(["snow-cli-ro", "auth", "login", "--also-now-sdk"])
                .is_err()
        );
    }

    #[test]
    fn table_full_and_preview_flags_map_to_full_commands() {
        let list = ReadOnlyCli::parse_from(["snow-cli-ro", "table", "list", "incident", "--full"]);
        match list.command {
            ReadOnlyCommands::Table(args) => match args.command.into_full_command() {
                TableCommands::List(list) => {
                    assert!(list.full);
                    assert!(!list.preview);
                }
                _ => panic!("expected table list command"),
            },
            _ => panic!("expected table command"),
        }

        let get = ReadOnlyCli::parse_from([
            "snow-cli-ro",
            "table",
            "get",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--preview",
        ]);
        match get.command {
            ReadOnlyCommands::Table(args) => match args.command.into_full_command() {
                TableCommands::Get(get) => {
                    assert!(!get.full);
                    assert!(get.preview);
                }
                _ => panic!("expected table get command"),
            },
            _ => panic!("expected table command"),
        }
    }

    #[test]
    fn codesearch_preview_flag_maps_to_full_command() {
        let cli = ReadOnlyCli::parse_from([
            "snow-cli-ro",
            "codesearch",
            "search",
            "GlideRecord",
            "--preview",
        ]);
        match cli.command {
            ReadOnlyCommands::Codesearch(args) => match args.command {
                crate::cli::args::CodesearchCommands::Search { full, preview, .. } => {
                    assert!(!full);
                    assert!(preview);
                }
            },
            _ => panic!("expected codesearch command"),
        }
    }

    #[test]
    fn read_only_full_conflicts_with_preview() {
        let error = ReadOnlyCli::try_parse_from([
            "snow-cli-ro",
            "table",
            "get",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--full",
            "--preview",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    /// Collect every command node keyed by its space-separated subcommand path
    /// (excluding clap's synthetic `help` subcommand).
    fn collect_commands<'a>(
        cmd: &'a clap::Command,
        prefix: Vec<String>,
        out: &mut BTreeMap<Vec<String>, &'a clap::Command>,
    ) {
        out.insert(prefix.clone(), cmd);
        for sub in cmd.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }
            let mut next = prefix.clone();
            next.push(sub.get_name().to_string());
            collect_commands(sub, next, out);
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ArgSignature {
        long: Option<String>,
        short: Option<char>,
        aliases: Vec<String>,
        defaults: Vec<String>,
        required: bool,
        hidden: bool,
        possible_values: Vec<String>,
    }

    fn arg_signatures(
        cmd: &clap::Command,
        include_globals: bool,
    ) -> BTreeMap<String, ArgSignature> {
        cmd.get_arguments()
            .filter(|arg| {
                arg.get_id().as_str() != "help"
                    && arg.get_id().as_str() != "version"
                    && (include_globals || !arg.is_global_set())
            })
            .map(|arg| {
                let mut aliases: Vec<String> = arg
                    .get_all_aliases()
                    .into_iter()
                    .flatten()
                    .map(str::to_string)
                    .collect();
                aliases.sort();
                let defaults: Vec<String> = arg
                    .get_default_values()
                    .iter()
                    .map(|v| v.to_string_lossy().into_owned())
                    .collect();
                let possible_values: Vec<String> = arg
                    .get_possible_values()
                    .into_iter()
                    .map(|v| v.get_name().to_string())
                    .collect();
                (
                    arg.get_id().as_str().to_string(),
                    ArgSignature {
                        long: arg.get_long().map(str::to_string),
                        short: arg.get_short(),
                        aliases,
                        defaults,
                        required: arg.is_required_set(),
                        hidden: arg.is_hide_set(),
                        possible_values,
                    },
                )
            })
            .collect()
    }

    /// Structural clap-introspection test proving `ReadOnlyCli` cannot drift
    /// from `Cli` in flags, defaults, aliases, help metadata, or command
    /// coverage, and matches `policy.rs`.
    #[test]
    fn read_only_cli_matches_full_cli_and_policy_without_drift() {
        let full_root = Cli::command();
        let ro_root = ReadOnlyCli::command();

        let mut full_cmds = BTreeMap::new();
        let mut ro_cmds = BTreeMap::new();
        collect_commands(&full_root, Vec::new(), &mut full_cmds);
        collect_commands(&ro_root, Vec::new(), &mut ro_cmds);

        // Flags intentionally excluded from ReadOnlyCli because policy denies them.
        let ro_excluded_flags: &[(&[&str], &str)] = &[
            (&[], "read_only"),
            (&["auth", "login"], "also_now_sdk"),
            (&["auth", "login"], "now_sdk_alias"),
            (&["auth", "login"], "set_now_sdk_default"),
        ];

        // 1. Every ReadOnlyCli command path must exist in Cli with matching
        //    flags, defaults, aliases, help metadata, and subset possible values.
        for (path, ro_cmd) in &ro_cmds {
            let full_cmd = full_cmds
                .get(path)
                .unwrap_or_else(|| panic!("ReadOnlyCli command `{path:?}` does not exist in Cli"));

            // Subcommand aliases must match (e.g., `profile` alias `config`).
            let mut ro_aliases: Vec<&str> = ro_cmd.get_all_aliases().collect();
            let mut full_aliases: Vec<&str> = full_cmd.get_all_aliases().collect();
            ro_aliases.sort_unstable();
            full_aliases.sort_unstable();
            assert_eq!(
                ro_aliases, full_aliases,
                "alias mismatch on command `{path:?}`"
            );

            // Every ReadOnlyCli command must keep doc comments (`about`) and
            // `after_help` whenever the corresponding full command has them.
            if !path.is_empty() {
                assert!(
                    ro_cmd.get_about().is_some(),
                    "ReadOnlyCli command `{path:?}` is missing doc comment (`about`)"
                );
            }
            if full_cmd.get_after_help().is_some() {
                assert!(
                    ro_cmd.get_after_help().is_some(),
                    "ReadOnlyCli command `{path:?}` dropped `after_help` present on Cli"
                );
            }

            let is_root = path.is_empty();
            let ro_args = arg_signatures(ro_cmd, is_root);
            let full_args = arg_signatures(full_cmd, is_root);
            let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();

            for (id, ro_sig) in &ro_args {
                let full_sig = full_args.get(id).unwrap_or_else(|| {
                    panic!("ReadOnlyCli arg `{id}` on `{path:?}` does not exist in Cli")
                });
                assert_eq!(
                    ro_sig.long, full_sig.long,
                    "long flag drift on `{path:?}` arg `{id}`"
                );
                assert_eq!(
                    ro_sig.short, full_sig.short,
                    "short flag drift on `{path:?}` arg `{id}`"
                );
                assert_eq!(
                    ro_sig.aliases, full_sig.aliases,
                    "alias drift on `{path:?}` arg `{id}`"
                );
                assert_eq!(
                    ro_sig.defaults, full_sig.defaults,
                    "default value drift on `{path:?}` arg `{id}`"
                );
                assert_eq!(
                    ro_sig.required, full_sig.required,
                    "required drift on `{path:?}` arg `{id}`"
                );
                assert_eq!(
                    ro_sig.hidden, full_sig.hidden,
                    "hidden drift on `{path:?}` arg `{id}`"
                );
                // Possible values in RO must be a subset of full CLI (e.g.
                // `domain table --counts` allows only `visible`, not `all`).
                for value in &ro_sig.possible_values {
                    assert!(
                        full_sig.possible_values.contains(value),
                        "unexpected possible value `{value}` on `{path:?}` arg `{id}`"
                    );
                }
            }

            for id in full_args.keys() {
                if ro_excluded_flags
                    .iter()
                    .any(|(p, excluded)| *p == path_refs.as_slice() && *excluded == id)
                {
                    assert!(
                        !ro_args.contains_key(id),
                        "denied flag `{id}` must not be exposed on ReadOnlyCli `{path:?}`"
                    );
                } else {
                    assert!(
                        ro_args.contains_key(id),
                        "ReadOnlyCli `{path:?}` is missing arg `{id}` from Cli"
                    );
                }
            }
        }

        // 2. Every leaf command in Cli that is absent from ReadOnlyCli MUST
        //    have an explicit read-only policy denial reason in policy.rs.
        for (path, full_cmd) in &full_cmds {
            let is_leaf = full_cmd
                .get_subcommands()
                .all(|sub| sub.get_name() == "help");
            if !is_leaf || path.is_empty() {
                continue;
            }
            let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();
            if !ro_cmds.contains_key(path) {
                assert!(
                    crate::policy::read_only_denial_for_path(&path_refs).is_some(),
                    "Cli leaf `{path:?}` is omitted from ReadOnlyCli but has no policy denial reason in policy::read_only_denial_for_path"
                );
            } else {
                assert!(
                    crate::policy::read_only_denial_for_path(&path_refs).is_none(),
                    "Cli leaf `{path:?}` is exposed in ReadOnlyCli even though policy::read_only_denial_for_path marks it denied"
                );
            }
        }

        // 3. Representative parses of newly aligned read-only commands must be
        //    allowed by ExecutionPolicy::read_only().
        let policy = ExecutionPolicy::read_only();
        for argv in [
            vec!["snow-cli-ro", "seed", "plan", "--file", "spec.json"],
            vec!["snow-cli-ro", "skill", "install", "--bundled"],
            vec!["snow-cli-ro", "snu", "broker", "status"],
            vec!["snow-cli-ro", "profile", "storage", "status"],
            vec!["snow-cli-ro", "profile", "sdk", "list"],
            vec![
                "snow-cli-ro",
                "domain",
                "table",
                "incident",
                "--counts",
                "visible",
            ],
        ] {
            let parsed = ReadOnlyCli::try_parse_from(&argv)
                .unwrap_or_else(|err| panic!("expected RO parse ok for {argv:?}: {err}"))
                .into_full_cli();
            assert_eq!(
                policy.decision_for_command(&parsed.command),
                PolicyDecision::Allow,
                "RO command `{argv:?}` must be allowed by ExecutionPolicy::read_only()"
            );
        }

        // 4. Previously exposed-but-blocked commands/values must now be
        //    rejected at parse time by ReadOnlyCli.
        for argv in [
            vec!["snow-cli-ro", "profile", "export-now-sdk", "dev"],
            vec!["snow-cli-ro", "profile", "sdk", "export", "dev"],
            vec!["snow-cli-ro", "profile", "storage", "set", "file"],
            vec!["snow-cli-ro", "profile", "storage", "migrate"],
            vec![
                "snow-cli-ro",
                "domain",
                "table",
                "incident",
                "--counts",
                "all",
            ],
            vec!["snow-cli-ro", "snu", "broker", "stop"],
            vec!["snow-cli-ro", "snu", "broker", "clear"],
        ] {
            assert!(
                ReadOnlyCli::try_parse_from(&argv).is_err(),
                "denied command `{argv:?}` must be rejected by ReadOnlyCli"
            );
        }
    }
}
