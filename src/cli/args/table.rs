use clap::{Args, Subcommand};

use crate::models::display_value::DisplayValue;
use crate::models::identifiers::{EncodedQueryValue, RecordKey, SysId, TableName};
use crate::models::order_by::OrderBy;

const TABLE_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 10\n  snow-cli table get incident <sys_id>\n  snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'\n  snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'\n  snow-cli table schema incident --extended\n  snow-cli table find flow\n  snow-cli table choices incident priority\n  snow-cli table stats incident --group-by state\n\nRecipes for agents:\n  Labels, not codes:   snow-cli table list incident --fields number,priority,state --display-value\n  Newest first, paged: snow-cli table list incident --order-by -sys_created_on --limit 10 --offset 10\n  Verify a filter:     unknown --query/--fields names appear under \"warnings\" (ServiceNow ignores them);\n                       add --check-fields error to fail instead, or compare table stats counts\n  Script by name:      snow-cli include show <name> --chain --source-of canMoveTo\n  Flow steps:          snow-cli flow show '<flow name>'\n  One field verbatim:  snow-cli table get sys_script_include <name> --fields script --raw";

/// `table --help` recipes for `snow-cli-ro` (read-only commands only).
pub(crate) const READ_ONLY_TABLE_AFTER_HELP: &str = "Recipes for agents:\n  Labels, not codes:   snow-cli-ro table list incident --fields number,priority,state --display-value\n  Newest first, paged: snow-cli-ro table list incident --order-by -sys_created_on --limit 10 --offset 10\n  Verify a filter:     unknown --query/--fields names appear under \"warnings\" (ServiceNow ignores them);\n                       add --check-fields error to fail instead, or compare table stats counts\n  Script by name:      snow-cli-ro include show <name> --chain --source-of canMoveTo\n  Flow steps:          snow-cli-ro flow show '<flow name>'\n  One field verbatim:  snow-cli-ro table get sys_script_include <name> --fields script --raw";

pub(crate) const TABLE_LIST_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 20\n  snow-cli table list incident --query 'active=true' --fields number,priority,state,assignment_group --order-by -sys_created_on --limit 5 --display-value\n  snow-cli table list sys_user --fields sys_id,user_name,email --order-by user_name\n  snow-cli table list incident --order-by -sys_created_on --limit 3 --offset 3   # second page\n  snow-cli table list incident --all --fields '*' --full   # everything, uncapped\n  snow-cli table list incident --preview   # opt in to shortening oversized fields\n\nNotes:\n  - Without --limit/--all, output is bounded to 20 records; without --fields, a compact table-aware field set is returned.\n  - --display-value (bare = display) returns labels (priority \"4 - Low\"); display renders dates in your user's timezone and format. both keeps raw values: {\"value\":..,\"display\":..} only where they differ (CSV/TOON: extra <field>.display column).\n  - Fields in --fields/--query/--order-by that ServiceNow does not return (unknown or not readable) are listed under \"warnings\" and on stderr, because ServiceNow silently ignores such query terms. --check-fields error fails instead; off skips the check (env SNOW_CLI_CHECK_FIELDS).\n  - Output is lossless by default. Pass --preview to shorten long field values in JSON, JSONL, TOON, text, or auto output; preview metadata reports the field path, original size, shown size, and --full escape hatch.\n  - --preview is not supported with CSV output because CSV has no metadata channel; CSV remains lossless.\n  - Responses include offset/next_offset and returned/truncated metadata plus the server-reported total.\n  - For complete data set extraction prefer `data export`.";

pub(crate) const TABLE_GET_AFTER_HELP: &str = "Examples:\n  snow-cli table get incident <sys_id> --display-value both\n  snow-cli table get sys_script_include ChangeRequestStateHandler --fields name,api_name,script\n  snow-cli table get sys_script_include ChangeRequestStateHandler --fields script --raw | grep -n canMoveTo\n  snow-cli table get incident INC0010001 --key-field number\n\nNotes:\n  - A record that is not a 32-hex sys_id is looked up by --key-field (default: number for values like INC0010001, otherwise name); several matches fail with the candidates' sys_ids.\n  - --raw prints the single --fields value verbatim (no JSON escaping).";

const TABLE_CREATE_AFTER_HELP: &str = "Examples:\n  snow-cli table create incident --data '{\"short_description\":\"VPN down\"}'\n  echo '{\"short_description\":\"From stdin\"}' | snow-cli table create incident";

pub(crate) const TABLE_STATS_AFTER_HELP: &str = "Examples:\n  snow-cli table stats incident\n  snow-cli table stats incident --query 'active=true'\n  snow-cli table stats incident --group-by state,priority\n  snow-cli table stats incident --group-by state --avg priority --having 'count>5'";
pub(crate) const TABLE_FIND_AFTER_HELP: &str = "Examples:\n  snow-cli table find flow\n  snow-cli table find 'service catalog' --limit 50\n\nNotes:\n  - Matches name or label (case-insensitive contains), sorted by name.\n  - Each row has name, label, extends (parent table), and scope.\n  - Output is bounded to 20 tables; use --limit, --offset, or --all.";

pub(crate) const TABLE_CHOICES_AFTER_HELP: &str = "Examples:\n  snow-cli table choices incident priority\n  snow-cli table choices incident\n  snow-cli table choices change_request state --language de\n\nNotes:\n  - Choices are taken from the most specific table in the hierarchy that defines them\n    (incident.priority choices live on task); each row names that table.\n  - Rows have element, value, label, sequence, table, and dependent_value when set.";

// --- Table ---

#[derive(Args, Debug)]
#[command(after_help = TABLE_AFTER_HELP)]
pub struct TableArgs {
    #[command(subcommand)]
    pub command: TableCommands,
}

#[derive(Args, Debug)]
pub struct TableListArgs {
    /// Table name (e.g., incident, sys_user, cmdb_ci)
    pub table: TableName,

    /// Exact record domain: global or a domain sys_id (does not grant visibility)
    #[arg(long)]
    pub domain: Option<crate::models::domain::DomainId>,

    /// Encoded query string
    #[arg(long)]
    pub query: Option<String>,

    /// Comma-separated list of fields to return. Defaults to a compact
    /// table-aware projection; pass "*" for all fields.
    #[arg(long)]
    pub fields: Option<String>,

    /// Maximum number of records to return (default: 20)
    #[arg(long, conflicts_with = "all")]
    pub limit: Option<usize>,

    /// Skip this many matching records (default: 0)
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Fetch every matching record instead of the bounded default
    #[arg(long)]
    pub all: bool,

    /// Sort order: field (ascending), -field or field:desc (descending);
    /// comma-separate several keys. Sent as ORDERBY clauses in the query.
    #[arg(long, allow_hyphen_values = true)]
    pub order_by: Option<OrderBy>,

    #[command(flatten)]
    pub domain_read: crate::cli::args::DomainReadArgs,

    #[command(flatten)]
    pub read: RecordReadArgs,

    /// Explicit lossless option (the default); conflicts with --preview
    #[arg(long, conflicts_with = "preview")]
    pub full: bool,

    /// Shorten oversized string fields and emit separate preview metadata
    /// (JSON/JSONL/TOON/text/auto only; conflicts with --full)
    #[arg(long, conflicts_with = "full")]
    pub preview: bool,
}

#[derive(Args, Debug)]
pub struct TableGetArgs {
    /// Table name
    pub table: TableName,

    /// Record sys_id, or a value of --key-field (e.g. a script include name)
    #[arg(value_name = "SYS_ID|NAME")]
    pub record: RecordKey,

    /// Comma-separated list of fields to return
    #[arg(long)]
    pub fields: Option<String>,

    #[command(flatten)]
    pub lookup: RecordLookupArgs,

    #[command(flatten)]
    pub domain_read: crate::cli::args::DomainReadArgs,

    #[command(flatten)]
    pub read: RecordReadArgs,

    /// Explicit lossless option (the default); conflicts with --preview
    #[arg(long, conflicts_with = "preview")]
    pub full: bool,

    /// Shorten oversized string fields and emit separate preview metadata
    /// (JSON/JSONL/TOON/text/auto only; conflicts with --full)
    #[arg(long, conflicts_with = "full")]
    pub preview: bool,
}

#[derive(Args, Debug)]
pub struct TableSchemaArgs {
    /// Table name (e.g., incident, sys_user, cmdb_ci)
    pub table: TableName,

    /// Show extended field metadata (required, read-only, max length, default, reference table)
    #[arg(long)]
    pub extended: bool,

    /// Only show columns defined on this table, not those inherited from parent tables
    #[arg(long)]
    pub own_only: bool,

    /// Deprecated: inherited columns are now included by default
    #[arg(long, hide = true, conflicts_with = "own_only")]
    pub include_inherited: bool,
}

#[derive(Args, Debug)]
pub struct TableFindArgs {
    /// Text to look for in table names and labels (e.g., flow)
    pub text: EncodedQueryValue,

    /// Maximum number of tables to return (default: 20)
    #[arg(long, conflicts_with = "all")]
    pub limit: Option<usize>,

    /// Skip this many matching tables (default: 0)
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Return every matching table instead of the bounded default
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct TableChoicesArgs {
    /// Table name (e.g., incident); parent tables are searched too
    pub table: TableName,

    /// Only this field's choices (e.g., priority)
    pub field: Option<EncodedQueryValue>,

    /// Choice label language
    #[arg(long, default_value = "en")]
    pub language: EncodedQueryValue,
}

#[derive(Args, Debug)]
pub struct TableStatsArgs {
    /// Table name (e.g., incident, sys_user, cmdb_ci)
    pub table: TableName,

    /// Exact record domain: global or a domain sys_id (does not grant visibility)
    #[arg(long)]
    pub domain: Option<crate::models::domain::DomainId>,

    /// Encoded query string
    #[arg(long)]
    pub query: Option<String>,

    /// Comma-separated fields to group by (one result row per group)
    #[arg(long)]
    pub group_by: Option<String>,

    /// Comma-separated fields to average
    #[arg(long)]
    pub avg: Option<String>,

    /// Comma-separated fields to take the minimum of
    #[arg(long)]
    pub min: Option<String>,

    /// Comma-separated fields to take the maximum of
    #[arg(long)]
    pub max: Option<String>,

    /// Comma-separated fields to sum
    #[arg(long)]
    pub sum: Option<String>,

    /// Filter groups by aggregate: count>5, avg(priority)>=2; comma-separated clauses are ANDed
    /// (native aggregate^field^operator^value also accepted)
    #[arg(long)]
    pub having: Option<String>,

    /// Check --query/--group-by/--avg/--min/--max/--sum/--having field names:
    /// warn (default), error, or off (env SNOW_CLI_CHECK_FIELDS). Costs one
    /// row read when any of those flags is set.
    #[arg(long, value_enum, value_name = "MODE")]
    pub check_fields: Option<CheckFields>,
}

#[derive(Subcommand, Debug)]
pub enum TableCommands {
    /// List records from a table (auto-paginated)
    #[command(after_help = TABLE_LIST_AFTER_HELP)]
    List(TableListArgs),

    /// Get a single record by sys_id or by name
    #[command(after_help = TABLE_GET_AFTER_HELP)]
    Get(TableGetArgs),

    /// Create a new record
    #[command(after_help = TABLE_CREATE_AFTER_HELP)]
    Create {
        /// Table name
        table: TableName,

        /// JSON data for the record
        #[arg(long)]
        data: Option<String>,
    },

    /// Update an existing record
    Update {
        /// Table name
        table: TableName,

        /// Record sys_id
        sys_id: SysId,

        /// JSON data for the update
        #[arg(long)]
        data: Option<String>,
    },

    /// Delete a record
    Delete {
        /// Table name
        table: TableName,

        /// Record sys_id
        sys_id: SysId,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

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

/// How to treat field names ServiceNow does not return (unknown or not
/// readable): ServiceNow silently ignores them in queries and projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum CheckFields {
    /// Report them under "warnings" and on stderr (default)
    #[default]
    Warn,
    /// Fail with a validation error before printing results
    Error,
    /// Skip the check
    Off,
}

impl CheckFields {
    /// Environment variable consulted when `--check-fields` is omitted.
    pub const ENV: &'static str = "SNOW_CLI_CHECK_FIELDS";

    /// Resolve the effective mode: flag > `SNOW_CLI_CHECK_FIELDS` > warn.
    /// An unknown env value is ignored with a warning, never fatal.
    pub fn resolve(flag: Option<Self>) -> Self {
        Self::resolve_from(flag, std::env::var(Self::ENV).ok().as_deref())
    }

    fn resolve_from(flag: Option<Self>, env: Option<&str>) -> Self {
        if let Some(mode) = flag {
            return mode;
        }
        match env
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            None | Some("") => Self::default(),
            Some("warn") => Self::Warn,
            Some("error") => Self::Error,
            Some("off") => Self::Off,
            Some(other) => {
                tracing::warn!(
                    value = other,
                    "ignoring unknown SNOW_CLI_CHECK_FIELDS value"
                );
                Self::default()
            }
        }
    }
}

/// Read modifiers shared by `table list` and `table get`.
#[derive(Args, Debug, Clone, Copy, Default)]
pub struct RecordReadArgs {
    /// Field values: raw (stored codes), display (labels; dates in your
    /// timezone/format), or both. Bare --display-value means display
    #[arg(long, value_name = "MODE", num_args = 0..=1, default_missing_value = "display")]
    pub display_value: Option<DisplayValue>,

    /// Check field names in --fields/--query/--order-by: warn (default),
    /// error, or off (env SNOW_CLI_CHECK_FIELDS)
    #[arg(long, value_enum, value_name = "MODE")]
    pub check_fields: Option<CheckFields>,
}

/// Name lookup and raw output options for `table get`.
#[derive(Args, Debug, Clone, Default)]
pub struct RecordLookupArgs {
    /// Field matched when the record is not a sys_id (default: number for
    /// values like INC0010001, otherwise name)
    #[arg(long, value_name = "FIELD")]
    pub key_field: Option<EncodedQueryValue>,

    /// Print the single --fields value verbatim (unescaped), e.g. script source
    #[arg(long, requires = "fields", conflicts_with = "preview")]
    pub raw: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

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
                TableCommands::List(TableListArgs {
                    table,
                    query,
                    limit,
                    ..
                }) => {
                    assert_eq!(table.as_str(), "incident");
                    assert_eq!(query, Some("active=true".to_string()));
                    assert_eq!(limit, Some(10));
                }
                _ => panic!("Expected Table List command"),
            },
            _ => panic!("Expected Table command"),
        }
    }

    #[test]
    fn test_parse_table_list_and_get_full_and_preview_flags() {
        let cli = Cli::parse_from(["snow-cli", "table", "list", "incident", "--full"]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::List(TableListArgs { full, preview, .. }) => {
                    assert!(full);
                    assert!(!preview);
                }
                _ => panic!("Expected Table List command"),
            },
            _ => panic!("Expected Table command"),
        }

        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "get",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--preview",
        ]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::Get(TableGetArgs {
                    full,
                    preview,
                    record,
                    ..
                }) => {
                    assert!(!full);
                    assert!(preview);
                    assert!(matches!(record, RecordKey::SysId(_)));
                }
                _ => panic!("Expected Table Get command"),
            },
            _ => panic!("Expected Table command"),
        }
    }

    #[test]
    fn test_table_full_conflicts_with_preview() {
        let error = Cli::try_parse_from([
            "snow-cli",
            "table",
            "list",
            "incident",
            "--full",
            "--preview",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn test_parse_display_value_bare_and_explicit() {
        let parse = |extra: &[&str]| {
            let mut argv = vec!["snow-cli", "table", "list", "incident"];
            argv.extend_from_slice(extra);
            match Cli::parse_from(argv).command {
                Commands::Table(args) => match args.command {
                    TableCommands::List(TableListArgs { read, .. }) => read,
                    _ => panic!("Expected Table List command"),
                },
                _ => panic!("Expected Table command"),
            }
        };
        assert_eq!(parse(&[]).display_value, None);
        assert_eq!(
            parse(&["--display-value"]).display_value,
            Some(DisplayValue::Display)
        );
        assert_eq!(
            parse(&["--display-value", "both", "--limit", "5"]).display_value,
            Some(DisplayValue::Both)
        );
        assert_eq!(
            parse(&["--display-value=all"]).display_value,
            Some(DisplayValue::Both)
        );
        assert_eq!(
            parse(&["--check-fields", "error"]).check_fields,
            Some(CheckFields::Error)
        );
    }

    #[test]
    fn test_parse_table_get_by_name_with_raw() {
        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "get",
            "sys_script_include",
            "ChangeRequestStateHandler",
            "--fields",
            "script",
            "--raw",
        ]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::Get(TableGetArgs { record, lookup, .. }) => {
                    assert!(matches!(record, RecordKey::Name(_)));
                    assert!(lookup.raw);
                }
                _ => panic!("Expected Table Get command"),
            },
            _ => panic!("Expected Table command"),
        }
        // --raw needs --fields.
        assert!(
            Cli::try_parse_from([
                "snow-cli",
                "table",
                "get",
                "sys_script_include",
                "X",
                "--raw"
            ])
            .is_err()
        );
    }

    #[test]
    fn test_check_fields_resolution_order() {
        assert_eq!(
            CheckFields::resolve_from(Some(CheckFields::Off), Some("error")),
            CheckFields::Off
        );
        assert_eq!(
            CheckFields::resolve_from(None, Some(" ERROR ")),
            CheckFields::Error
        );
        assert_eq!(
            CheckFields::resolve_from(None, Some("bogus")),
            CheckFields::Warn
        );
        assert_eq!(CheckFields::resolve_from(None, None), CheckFields::Warn);
    }

    #[test]
    fn test_parse_table_stats() {
        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "stats",
            "incident",
            "--query",
            "active=true",
            "--group-by",
            "state,priority",
            "--avg",
            "priority",
            "--having",
            "count>5",
        ]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::Stats(TableStatsArgs {
                    table,
                    query,
                    group_by,
                    avg,
                    min,
                    max,
                    sum,
                    having,
                    ..
                }) => {
                    assert_eq!(table.as_str(), "incident");
                    assert_eq!(query, Some("active=true".to_string()));
                    assert_eq!(group_by, Some("state,priority".to_string()));
                    assert_eq!(avg, Some("priority".to_string()));
                    assert_eq!(min, None);
                    assert_eq!(max, None);
                    assert_eq!(sum, None);
                    assert_eq!(having, Some("count>5".to_string()));
                }
                _ => panic!("Expected Table Stats command"),
            },
            _ => panic!("Expected Table command"),
        }
    }
}
