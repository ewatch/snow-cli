use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

/// snow-cli — ServiceNow CLI for humans and coding agents
#[derive(Parser, Debug)]
#[command(name = "snow-cli", version, about, long_about = None)]
pub struct Cli {
    /// ServiceNow profile to use
    #[arg(long, global = true, default_value = "default")]
    pub profile: String,

    /// Override the ServiceNow instance URL
    #[arg(long, global = true)]
    pub instance: Option<String>,

    /// Output format
    #[arg(long, global = true, default_value = "json")]
    pub output: OutputFormat,

    /// Increase verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Csv,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage configuration and profiles
    Config(ConfigArgs),

    /// Authentication operations
    Auth(AuthArgs),

    /// Table API operations (CRUD on any ServiceNow table)
    Table(TableArgs),

    /// Incident management shortcuts
    Incident(IncidentArgs),

    /// Attachment operations
    Attachment(AttachmentArgs),

    /// Import set operations
    ImportSet(ImportSetArgs),

    /// Raw REST API calls to any endpoint
    Api(ApiArgs),

    /// Execute background scripts on ServiceNow
    Script(ScriptArgs),

    /// Search code across ServiceNow instance (scripts, business rules, etc.)
    Codesearch(CodesearchArgs),

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

// --- Config ---

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Interactive first-time setup
    Init {
        /// Instance URL (e.g., https://mycompany.service-now.com)
        #[arg(long)]
        instance: Option<String>,

        /// Authentication method
        #[arg(long, value_enum)]
        auth_method: Option<CliAuthMethod>,

        /// Username (for basic auth or OAuth2 password grant)
        #[arg(long)]
        username: Option<String>,

        /// OAuth grant type (for oauth2 auth method)
        #[arg(long, value_enum)]
        oauth_grant_type: Option<CliOAuthGrantType>,

        /// Profile name to create (defaults to "default")
        #[arg(long, default_value = "default")]
        name: String,
    },

    /// Create or update a named profile
    SetProfile {
        /// Profile name
        name: String,

        /// Instance URL (e.g., https://mycompany.service-now.com)
        #[arg(long)]
        instance: Option<String>,

        /// Authentication method
        #[arg(long, value_enum)]
        auth_method: Option<CliAuthMethod>,

        /// Username (for basic auth or OAuth2 password grant)
        #[arg(long)]
        username: Option<String>,

        /// OAuth client ID (for oauth2)
        #[arg(long)]
        client_id: Option<String>,

        /// OAuth grant type (for oauth2 auth method)
        #[arg(long, value_enum)]
        oauth_grant_type: Option<CliOAuthGrantType>,

        /// Path to client certificate (for mTLS)
        #[arg(long)]
        cert_path: Option<String>,

        /// Path to client key (for mTLS)
        #[arg(long)]
        key_path: Option<String>,
    },

    /// List all configured profiles
    ListProfiles,

    /// Set the active default profile
    UseProfile {
        /// Profile name to activate
        name: String,
    },

    /// Show the current active configuration
    Show,
}

/// Authentication method for CLI argument parsing.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum CliAuthMethod {
    Basic,
    Oauth2,
    ApiKey,
    Mtls,
    Saml,
}

/// OAuth 2.0 grant type for CLI argument parsing.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum CliOAuthGrantType {
    ClientCredentials,
    Password,
}

// --- Auth ---

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommands,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// Authenticate and store credentials in the OS keychain
    Login {
        /// Password for basic auth (reads from stdin if not provided)
        #[arg(long)]
        password: Option<String>,

        /// API token (for api_key auth)
        #[arg(long)]
        token: Option<String>,

        /// OAuth client secret (for oauth2 auth)
        #[arg(long)]
        client_secret: Option<String>,
    },

    /// Clear stored credentials for the active profile
    Logout,

    /// Show current authentication status
    Status,

    /// Print the current access token to stdout (for piping to other tools)
    Token,
}

// --- Table ---

#[derive(Args, Debug)]
pub struct TableArgs {
    #[command(subcommand)]
    pub command: TableCommands,
}

#[derive(Subcommand, Debug)]
pub enum TableCommands {
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

    /// Create a new record
    Create {
        /// Table name
        table: String,

        /// JSON data for the record
        #[arg(long)]
        data: Option<String>,
    },

    /// Update an existing record
    Update {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// JSON data for the update
        #[arg(long)]
        data: Option<String>,
    },

    /// Delete a record
    Delete {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Show table schema (columns, types, labels) from sys_dictionary
    Schema {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Show extended field metadata (required, read-only, max length, default, reference table)
        #[arg(long)]
        extended: bool,

        /// Include fields inherited from parent tables (e.g., incident inherits from task)
        #[arg(long)]
        include_inherited: bool,
    },
}

// --- Incident ---

#[derive(Args, Debug)]
pub struct IncidentArgs {
    #[command(subcommand)]
    pub command: IncidentCommands,
}

#[derive(Subcommand, Debug)]
pub enum IncidentCommands {
    /// List incidents
    List {
        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Maximum number of records to return
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Get an incident by number
    Get {
        /// Incident number (e.g., INC0010001)
        number: String,
    },

    /// Create a new incident
    Create {
        /// JSON data for the incident
        #[arg(long)]
        data: Option<String>,
    },

    /// Update an incident by number
    Update {
        /// Incident number
        number: String,

        /// JSON data for the update
        #[arg(long)]
        data: Option<String>,
    },

    /// Resolve an incident
    Resolve {
        /// Incident number
        number: String,

        /// Resolution notes
        #[arg(long)]
        notes: Option<String>,
    },
}

// --- Attachment ---

#[derive(Args, Debug)]
pub struct AttachmentArgs {
    #[command(subcommand)]
    pub command: AttachmentCommands,
}

#[derive(Subcommand, Debug)]
pub enum AttachmentCommands {
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
        #[arg(long, short)]
        output: Option<String>,
    },

    /// Upload a file as an attachment
    Upload {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Path to the file to upload
        #[arg(long, short)]
        file: String,
    },
}

// --- Import Set ---

#[derive(Args, Debug)]
pub struct ImportSetArgs {
    #[command(subcommand)]
    pub command: ImportSetCommands,
}

#[derive(Subcommand, Debug)]
pub enum ImportSetCommands {
    /// Load data into a staging table
    Load {
        /// Staging table name
        table: String,

        /// JSON data to load
        #[arg(long)]
        data: Option<String>,
    },

    /// Transform staged data
    Transform {
        /// Import set sys_id
        sys_id: String,
    },
}

// --- API (raw) ---

#[derive(Args, Debug)]
pub struct ApiArgs {
    #[command(subcommand)]
    pub command: ApiCommands,
}

#[derive(Subcommand, Debug)]
pub enum ApiCommands {
    /// Send a GET request
    Get {
        /// API path (e.g., /api/x_myapp/my_endpoint)
        path: String,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },

    /// Send a POST request
    Post {
        /// API path
        path: String,

        /// JSON request body
        #[arg(long)]
        data: Option<String>,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },

    /// Send a PUT request
    Put {
        /// API path
        path: String,

        /// JSON request body
        #[arg(long)]
        data: Option<String>,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },

    /// Send a DELETE request
    Delete {
        /// API path
        path: String,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },
}

// --- Script ---

#[derive(Args, Debug)]
pub struct ScriptArgs {
    #[command(subcommand)]
    pub command: ScriptCommands,
}

#[derive(Subcommand, Debug)]
pub enum ScriptCommands {
    /// Execute a background script on the ServiceNow instance [WIP: requires a Scripted REST endpoint on the target instance]
    Run {
        /// Path to a script file to execute
        #[arg(long, short = 'f', group = "script_source")]
        file: Option<String>,

        /// Inline script code to execute
        #[arg(long, short = 'c', group = "script_source")]
        code: Option<String>,

        /// Scope in which to run the script (e.g., global, x_myapp)
        #[arg(long, default_value = "global")]
        scope: String,
    },
}

// --- Codesearch ---

#[derive(Args, Debug)]
pub struct CodesearchArgs {
    #[command(subcommand)]
    pub command: CodesearchCommands,
}

#[derive(Subcommand, Debug)]
pub enum CodesearchCommands {
    /// Search code across the ServiceNow instance
    Search {
        /// Search term
        #[arg(long, short)]
        term: String,

        /// Limit to a specific table (e.g., sys_script_include, sys_script, sysevent_script_action)
        #[arg(long)]
        table: Option<String>,

        /// Maximum number of results to return (default: 100)
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Search across all application scopes
        #[arg(long, default_value = "true")]
        search_all_scopes: bool,

        /// Search group to use (default: sn_devstudio.Studio Search Group)
        #[arg(long, default_value = "sn_devstudio.Studio Search Group")]
        search_group: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_parse_verbose_flags() {
        let cli = Cli::parse_from(["snow-cli", "-vvv", "config", "show"]);
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_parse_profile_flag() {
        let cli = Cli::parse_from(["snow-cli", "--profile", "prod", "config", "show"]);
        assert_eq!(cli.profile, "prod");
    }

    #[test]
    fn test_parse_default_profile() {
        let cli = Cli::parse_from(["snow-cli", "config", "show"]);
        assert_eq!(cli.profile, "default");
    }

    #[test]
    fn test_parse_output_format() {
        let cli = Cli::parse_from(["snow-cli", "--output", "csv", "config", "show"]);
        assert!(matches!(cli.output, OutputFormat::Csv));
    }

    #[test]
    fn test_parse_table_list() {
        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "list",
            "incident",
            "--query",
            "active=true",
            "--limit",
            "10",
        ]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::List {
                    table,
                    query,
                    limit,
                    ..
                } => {
                    assert_eq!(table, "incident");
                    assert_eq!(query, Some("active=true".to_string()));
                    assert_eq!(limit, Some(10));
                }
                _ => panic!("Expected Table List command"),
            },
            _ => panic!("Expected Table command"),
        }
    }
}
