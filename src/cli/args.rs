use clap::{Parser, Subcommand};
use clap_complete::Shell;

mod api;
mod attachment;
mod auth;
mod codesearch;
mod data;
mod graphql;
mod import_set;
mod profile;
mod scope;
mod script;
mod seed;
mod skill;
mod snu;
mod table;

pub use api::*;
pub use attachment::*;
pub use auth::*;
pub use codesearch::*;
pub use data::*;
pub use graphql::*;
pub use import_set::*;
pub use profile::*;
pub use scope::*;
pub use script::*;
pub use seed::*;
pub use skill::*;
pub use snu::*;
pub use table::*;

const TOP_LEVEL_AFTER_HELP: &str = "Common workflows:\n  1) First-time setup\n     snow-cli profile add default --instance https://mycompany.service-now.com --auth-method basic --username admin\n\n  2) Store credentials\n     snow-cli auth login\n\n  3) List recent incidents\n     snow-cli table list incident --query 'active=true' --limit 20\n\n  4) Create and update records\n     snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'\n     snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'\n\n  5) Call a custom API\n     snow-cli api get /api/x_myapp/status";

/// ❄️ snow-cli — ServiceNow CLI for humans and coding agents
#[derive(Parser, Debug)]
#[command(
    name = "snow-cli",
    version,
    about = "❄️ snow-cli — CLI gateway for LLMs and coding agents to access ServiceNow instances",
    long_about = None,
    after_help = TOP_LEVEL_AFTER_HELP
)]
pub struct Cli {
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

    /// Block commands and HTTP methods that can mutate ServiceNow
    #[arg(long, global = true)]
    pub read_only: bool,

    /// Increase verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Csv,
    Jsonl,
    Toon,
    Text,
    /// Pick the most token-efficient lossless format per payload (TOON/JSONL/JSON).
    Auto,
}

impl OutputFormat {
    /// Canonical lowercase name, used for config storage and diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Json => "json",
            OutputFormat::Csv => "csv",
            OutputFormat::Jsonl => "jsonl",
            OutputFormat::Toon => "toon",
            OutputFormat::Text => "text",
            OutputFormat::Auto => "auto",
        }
    }

    /// Parse a format name leniently, returning `None` for unknown values
    /// instead of erroring. Used when interpreting env vars and config values
    /// so a stray value degrades gracefully rather than failing the command.
    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Some(OutputFormat::Json),
            "csv" => Some(OutputFormat::Csv),
            "jsonl" => Some(OutputFormat::Jsonl),
            "toon" => Some(OutputFormat::Toon),
            "text" => Some(OutputFormat::Text),
            "auto" => Some(OutputFormat::Auto),
            _ => None,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage ServiceNow connection profiles
    #[command(alias = "config")]
    Profile(ConfigArgs),

    /// Authentication operations
    Auth(AuthArgs),

    /// Table API operations (CRUD on any ServiceNow table)
    Table(TableArgs),

    /// Portable data export, validation, and import workflows
    Data(DataArgs),

    /// Declarative test-data planning, apply, and cleanup workflows
    Seed(SeedArgs),

    /// Analyze application scope metadata
    Scope(ScopeArgs),

    /// Attachment operations
    Attachment(AttachmentArgs),

    /// Import set operations
    ImportSet(ImportSetArgs),

    /// Raw REST API calls to any endpoint
    Api(ApiArgs),

    /// Submit a document to the optional Now GraphQL endpoint
    Graphql(GraphqlArgs),

    /// Execute background scripts on ServiceNow
    Script(ScriptArgs),

    /// Search code across ServiceNow instance (scripts, business rules, etc.)
    Codesearch(CodesearchArgs),

    /// Use the SN-Utils browser extension helper tab directly (without sn-scriptsync)
    Snu(SnuArgs),

    /// Install agent skills from local bundles or URL-hosted manifests
    // Hidden for now: the feature is not ready to be advertised yet. Still
    // parses/runs if invoked explicitly; remove `hide = true` to surface it.
    #[command(hide = true)]
    Skill(SkillArgs),

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn test_cli_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_top_level_help_includes_snowflake() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("❄️ snow-cli"));
    }

    #[test]
    fn test_parse_verbose_flags() {
        let cli = Cli::parse_from(["snow-cli", "-vvv", "config", "show"]);
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_parse_profile_flag() {
        let cli = Cli::parse_from(["snow-cli", "--profile", "prod", "config", "show"]);
        assert_eq!(cli.profile, Some("prod".to_string()));
    }

    #[test]
    fn test_parse_profile_is_optional() {
        let cli = Cli::parse_from(["snow-cli", "config", "show"]);
        assert_eq!(cli.profile, None);
    }

    #[test]
    fn test_parse_output_format() {
        let cli = Cli::parse_from(["snow-cli", "--output", "csv", "config", "show"]);
        assert!(matches!(cli.output, Some(OutputFormat::Csv)));

        let cli = Cli::parse_from(["snow-cli", "--output", "jsonl", "config", "show"]);
        assert!(matches!(cli.output, Some(OutputFormat::Jsonl)));

        let cli = Cli::parse_from(["snow-cli", "--output", "toon", "config", "show"]);
        assert!(matches!(cli.output, Some(OutputFormat::Toon)));

        let cli = Cli::parse_from(["snow-cli", "--output", "auto", "config", "show"]);
        assert!(matches!(cli.output, Some(OutputFormat::Auto)));

        let cli = Cli::parse_from(["snow-cli", "--format", "jsonl", "config", "show"]);
        assert!(matches!(cli.output, Some(OutputFormat::Jsonl)));

        // Omitting --output leaves it unset so config/env can supply a default.
        let cli = Cli::parse_from(["snow-cli", "config", "show"]);
        assert!(cli.output.is_none());
    }
}
