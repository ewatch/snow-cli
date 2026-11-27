use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod api;
mod attachment;
mod auth;
mod codesearch;
mod data;
mod domain;
mod flow;
mod graphql;
mod import_set;
mod include;
mod profile;
mod scope;
mod script;
mod seed;
mod session;
mod skill;
mod snu;
mod table;

pub use api::*;
pub use attachment::*;
pub use auth::*;
pub use codesearch::*;
pub use data::*;
pub use domain::*;
pub use flow::*;
pub use graphql::*;
pub use import_set::*;
pub use include::*;
pub use profile::*;
pub use scope::*;
pub use script::*;
pub use seed::*;
pub use session::*;
pub use skill::*;
pub use snu::*;
pub use table::*;

const TOP_LEVEL_AFTER_HELP: &str = "\
For agents:
  Reads default to 20 rows, compact fields, and truncation metadata (total/returned/offset/next_offset).
  Use --output auto (or SNOW_CLI_OUTPUT=auto) for token-efficient lossless output (default: json).
  Add --read-only (or use snow-cli-ro) to block mutating commands and HTTP methods.

  Newest first:   snow-cli table list incident --query 'active=true' --order-by -sys_created_on --limit 20 --output auto
  Page + labels:  snow-cli table list incident --order-by -sys_created_on --limit 20 --offset 20 --display-value
  Discovery:      snow-cli table find flow
                  snow-cli table schema incident --extended
                  snow-cli table choices incident priority
  Code & flows:   snow-cli include show <name> --chain
                  snow-cli flow show '<flow name>'
  Write / API:    snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'
                  snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'
                  snow-cli api get /api/x_myapp/status

First-time setup (human):
  snow-cli profile add default --instance https://mycompany.service-now.com --auth-method basic --username admin
  snow-cli auth login
  snow-cli auth status --verify";

pub(crate) const GLOBAL_OPTIONS_FOOTER: &str = "Global options: --profile, --instance, --output, --timeout-secs, --read-only, -v (see snow-cli --help)";

pub(crate) const READ_ONLY_GLOBAL_OPTIONS_FOOTER: &str =
    "Global options: --profile, --instance, --output, --timeout-secs, -v (see snow-cli-ro --help)";

/// Hide root-level global options on every subcommand's help output while
/// keeping them parseable anywhere on the command line and fully documented
/// on the root `--help`. Prepends a concise one-line reminder to each
/// subcommand's `after_help`.
pub(crate) fn hide_global_options_on_subcommands(
    cmd: clap::Command,
    footer: &'static str,
) -> clap::Command {
    let hidden_globals: Vec<clap::Arg> = cmd
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .map(|arg| arg.clone().hide(true))
        .collect();

    cmd.mut_subcommands(|sub| apply_hidden_globals_recursive(sub, &hidden_globals, footer))
}

fn apply_hidden_globals_recursive(
    mut cmd: clap::Command,
    hidden_globals: &[clap::Arg],
    footer: &'static str,
) -> clap::Command {
    for arg in hidden_globals {
        // Preserve any subcommand-local option that shares an ID with a global
        // flag (for example `profile add --instance` or `snu --timeout-secs`).
        if cmd
            .get_arguments()
            .all(|existing| existing.get_id() != arg.get_id())
        {
            cmd = cmd.arg(arg.clone());
        }
    }

    let combined_after_help = match cmd.get_after_help() {
        Some(existing) => {
            let existing = existing.to_string();
            if existing.contains(footer) {
                existing
            } else {
                format!("{footer}\n\n{existing}")
            }
        }
        None => footer.to_string(),
    };
    cmd = cmd.after_help(combined_after_help);

    cmd.mut_subcommands(|sub| apply_hidden_globals_recursive(sub, hidden_globals, footer))
}

/// ❄️ snow-cli — ServiceNow CLI for humans and coding agents
#[derive(Args, Debug)]
pub struct Cli {
    /// ServiceNow profile to use
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Override the instance URL (must match the profile's scheme, host, and port)
    #[arg(long, global = true)]
    pub instance: Option<String>,

    /// Output format: json (default), csv, jsonl, toon, text, or auto (token-efficient lossless; resolves via SNOW_CLI_OUTPUT, then `config output`)
    #[arg(long, alias = "format", global = true, hide_possible_values = true)]
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

impl CommandFactory for Cli {
    fn command() -> clap::Command {
        let cmd = <Self as Args>::augment_args(clap::Command::new("snow-cli"))
            .version(env!("CARGO_PKG_VERSION"))
            .about(
                "❄️ snow-cli — CLI gateway for LLMs and coding agents to access ServiceNow instances",
            )
            .long_about(None)
            .after_help(TOP_LEVEL_AFTER_HELP);
        hide_global_options_on_subcommands(cmd, GLOBAL_OPTIONS_FOOTER)
    }

    fn command_for_update() -> clap::Command {
        let cmd = <Self as Args>::augment_args_for_update(clap::Command::new("snow-cli"))
            .version(env!("CARGO_PKG_VERSION"))
            .about(
                "❄️ snow-cli — CLI gateway for LLMs and coding agents to access ServiceNow instances",
            )
            .long_about(None)
            .after_help(TOP_LEVEL_AFTER_HELP);
        hide_global_options_on_subcommands(cmd, GLOBAL_OPTIONS_FOOTER)
    }
}

impl Parser for Cli {}

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

    /// Discover domain separation settings, hierarchy, and table support
    Domain(DomainArgs),

    /// Manage native ServiceNow sessions without SN-Utils
    Session(SessionArgs),

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

    /// Read script include source by name, with parsed extends chain and grep
    Include(IncludeArgs),

    /// Show a Flow Designer flow as a compact step tree
    Flow(FlowArgs),

    /// Use the SN-Utils browser extension helper tab directly (without sn-scriptsync)
    Snu(SnuArgs),

    /// Install agent skills from the embedded bundle, local paths, or URL-hosted manifests
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

    #[test]
    fn test_global_options_parse_after_subcommands() {
        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "list",
            "incident",
            "--output",
            "auto",
            "--profile",
            "prod",
            "--read-only",
            "-vv",
        ]);
        assert_eq!(cli.profile.as_deref(), Some("prod"));
        assert!(matches!(cli.output, Some(OutputFormat::Auto)));
        assert!(cli.read_only);
        assert_eq!(cli.verbose, 2);

        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "list",
            "incident",
            "--format",
            "jsonl",
            "--instance",
            "https://dev.service-now.com",
            "--timeout-secs",
            "45",
            "-vvv",
        ]);
        assert_eq!(cli.instance.as_deref(), Some("https://dev.service-now.com"));
        assert!(matches!(cli.output, Some(OutputFormat::Jsonl)));
        assert_eq!(cli.timeout_secs, Some(45));
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_root_help_is_agent_first_and_documents_globals_concisely() {
        let help = Cli::command().render_long_help().to_string();
        for expected in [
            "--profile <PROFILE>",
            "--instance <INSTANCE>",
            "--output <OUTPUT>",
            "--timeout-secs <TIMEOUT_SECS>",
            "--read-only",
            "-v, --verbose",
            "json",
            "csv",
            "jsonl",
            "toon",
            "text",
            "auto",
            "--output auto",
            "SNOW_CLI_OUTPUT=auto",
            "--order-by -sys_created_on",
            "--offset",
            "--display-value",
            "table schema",
            "table choices",
            "table find",
            "include show",
            "flow show",
        ] {
            assert!(help.contains(expected), "root help missing `{expected}`");
        }
        assert!(
            !help.contains("Possible values:"),
            "root --output should stay a single concise line:\n{help}"
        );
        assert!(
            !help.contains(GLOBAL_OPTIONS_FOOTER),
            "root help should not duplicate the subcommand global-options footer"
        );
        let agents_pos = help.find("For agents:").expect("missing For agents header");
        let setup_pos = help
            .find("First-time setup (human):")
            .expect("missing human setup header");
        assert!(
            agents_pos < setup_pos,
            "For agents section must appear before human setup"
        );
        assert!(
            help.len() < 4_000,
            "root help unexpectedly large: {} bytes",
            help.len()
        );
    }

    #[test]
    fn test_subcommand_help_hides_global_blocks_and_shows_footer() {
        let mut root = Cli::command();
        root.build();
        for path in [
            vec!["table"],
            vec!["table", "list"],
            vec!["table", "get"],
            vec!["scope", "inspect"],
            vec!["api", "get"],
        ] {
            let mut cmd = &root;
            for seg in &path {
                cmd = cmd
                    .find_subcommand(seg)
                    .unwrap_or_else(|| panic!("missing subcommand {path:?}"));
            }
            let help = cmd.clone().render_long_help().to_string();
            let (before_footer, _) = help
                .split_once(GLOBAL_OPTIONS_FOOTER)
                .unwrap_or_else(|| panic!("{path:?} help missing global options footer:\n{help}"));
            for hidden in [
                "--profile",
                "--instance",
                "--output",
                "--timeout-secs",
                "--read-only",
                "-v, --verbose",
                "Override the HTTP request timeout",
                "Block commands and HTTP methods",
            ] {
                assert!(
                    !before_footer.contains(hidden),
                    "{path:?} help should hide global option `{hidden}`:\n{before_footer}"
                );
            }
        }

        // Subcommands that define their own local `--instance` keep it visible.
        let profile_add_help = root
            .find_subcommand("profile")
            .and_then(|p| p.find_subcommand("add"))
            .expect("profile add exists")
            .clone()
            .render_long_help()
            .to_string();
        assert!(profile_add_help.contains("--instance <INSTANCE>"));
        assert!(profile_add_help.contains(GLOBAL_OPTIONS_FOOTER));
        assert!(!profile_add_help.contains("--profile <PROFILE>"));
    }
}
