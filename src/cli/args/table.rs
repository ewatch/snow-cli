use clap::{Args, Subcommand};

use crate::models::identifiers::{EncodedQueryValue, SysId, TableName};
use crate::models::order_by::OrderBy;

const TABLE_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 10\n  snow-cli table get incident <sys_id>\n  snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'\n  snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'\n  snow-cli table schema incident --extended\n  snow-cli table find flow\n  snow-cli table choices incident priority\n  snow-cli table stats incident --group-by state";
const TABLE_LIST_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 20\n  snow-cli table list sys_user --fields sys_id,user_name,email --order-by user_name\n  snow-cli table list incident --order-by -sys_created_on --limit 3 --offset 3   # second page\n  snow-cli table list incident --all --fields '*' --full   # everything, uncapped\n  snow-cli table list incident --preview   # opt in to shortening oversized fields\n\nNotes:\n  - Without --limit/--all, output is bounded to 20 records; without --fields, a compact table-aware field set is returned.\n  - Output is lossless by default. Pass --preview to shorten long field values in JSON, JSONL, TOON, text, or auto output; preview metadata reports the field path, original size, shown size, and --full escape hatch.\n  - --preview is not supported with CSV output because CSV has no metadata channel; CSV remains lossless.\n  - Responses include offset/next_offset and returned/truncated metadata plus the server-reported total.\n  - For complete data set extraction prefer `data export`.";

const TABLE_CREATE_AFTER_HELP: &str = "Examples:\n  snow-cli table create incident --data '{\"short_description\":\"VPN down\"}'\n  echo '{\"short_description\":\"From stdin\"}' | snow-cli table create incident";

const TABLE_STATS_AFTER_HELP: &str = "Examples:\n  snow-cli table stats incident\n  snow-cli table stats incident --query 'active=true'\n  snow-cli table stats incident --group-by state,priority\n  snow-cli table stats incident --group-by state --avg priority --having 'count>5'";
pub(crate) const TABLE_FIND_AFTER_HELP: &str = "Examples:\n  snow-cli table find flow\n  snow-cli table find 'service catalog' --limit 50\n\nNotes:\n  - Matches name or label (case-insensitive contains), sorted by name.\n  - Each row has name, label, extends (parent table), and scope.\n  - Output is bounded to 20 tables; use --limit, --offset, or --all.";

pub(crate) const TABLE_CHOICES_AFTER_HELP: &str = "Examples:\n  snow-cli table choices incident priority\n  snow-cli table choices incident\n  snow-cli table choices change_request state --language de\n\nNotes:\n  - Choices are taken from the most specific table in the hierarchy that defines them\n    (incident.priority choices live on task); each row names that table.\n  - Rows have element, value, label, sequence, table, and dependent_value when set.";

// --- Table ---

#[derive(Args, Debug)]
#[command(after_help = TABLE_AFTER_HELP)]
pub struct TableArgs {
    #[command(subcommand)]
    pub command: TableCommands,
}

#[derive(Subcommand, Debug)]
pub enum TableCommands {
    /// List records from a table (auto-paginated)
    #[command(after_help = TABLE_LIST_AFTER_HELP)]
    List {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: TableName,

        /// Exact record domain: global or a domain sys_id (does not grant visibility)
        #[arg(long)]
        domain: Option<crate::models::domain::DomainId>,

        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated list of fields to return. Defaults to a compact
        /// table-aware projection; pass "*" for all fields.
        #[arg(long)]
        fields: Option<String>,

        /// Maximum number of records to return (default: 20)
        #[arg(long, conflicts_with = "all")]
        limit: Option<usize>,

        /// Skip this many matching records (default: 0)
        #[arg(long, default_value_t = 0)]
        offset: usize,

        /// Fetch every matching record instead of the bounded default
        #[arg(long)]
        all: bool,

        /// Sort order: field (ascending), -field or field:desc (descending);
        /// comma-separate several keys. Sent as ORDERBY clauses in the query.
        #[arg(long, allow_hyphen_values = true)]
        order_by: Option<OrderBy>,

        #[command(flatten)]
        domain_read: crate::cli::args::DomainReadArgs,

        /// Explicit lossless option (the default); conflicts with --preview
        #[arg(long, conflicts_with = "preview")]
        full: bool,

        /// Shorten oversized string fields and emit separate preview metadata
        /// (JSON/JSONL/TOON/text/auto only; conflicts with --full)
        #[arg(long, conflicts_with = "full")]
        preview: bool,
    },

    /// Get a single record by sys_id
    Get {
        /// Table name
        table: TableName,

        /// Record sys_id
        sys_id: SysId,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,

        #[command(flatten)]
        domain_read: crate::cli::args::DomainReadArgs,

        /// Explicit lossless option (the default); conflicts with --preview
        #[arg(long, conflicts_with = "preview")]
        full: bool,

        /// Shorten oversized string fields and emit separate preview metadata
        /// (JSON/JSONL/TOON/text/auto only; conflicts with --full)
        #[arg(long, conflicts_with = "full")]
        preview: bool,
    },

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
    Schema {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: TableName,

        /// Show extended field metadata (required, read-only, max length, default, reference table)
        #[arg(long)]
        extended: bool,

        /// Only show columns defined on this table, not those inherited from parent tables
        #[arg(long)]
        own_only: bool,

        /// Deprecated: inherited columns are now included by default
        #[arg(long, hide = true, conflicts_with = "own_only")]
        include_inherited: bool,
    },

    /// Find tables whose name or label contains text (searches sys_db_object)
    #[command(after_help = TABLE_FIND_AFTER_HELP)]
    Find {
        /// Text to look for in table names and labels (e.g., flow)
        text: EncodedQueryValue,

        /// Maximum number of tables to return (default: 20)
        #[arg(long, conflicts_with = "all")]
        limit: Option<usize>,

        /// Skip this many matching tables (default: 0)
        #[arg(long, default_value_t = 0)]
        offset: usize,

        /// Return every matching table instead of the bounded default
        #[arg(long)]
        all: bool,
    },

    /// List active choice values and labels for a table's fields (from sys_choice)
    #[command(after_help = TABLE_CHOICES_AFTER_HELP)]
    Choices {
        /// Table name (e.g., incident); parent tables are searched too
        table: TableName,

        /// Only this field's choices (e.g., priority)
        field: Option<EncodedQueryValue>,

        /// Choice label language
        #[arg(long, default_value = "en")]
        language: EncodedQueryValue,
    },

    /// Count and aggregate records via the Aggregate API (Stats endpoint)
    #[command(after_help = TABLE_STATS_AFTER_HELP)]
    Stats {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: TableName,

        /// Exact record domain: global or a domain sys_id (does not grant visibility)
        #[arg(long)]
        domain: Option<crate::models::domain::DomainId>,

        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated fields to group by (one result row per group)
        #[arg(long)]
        group_by: Option<String>,

        /// Comma-separated fields to average
        #[arg(long)]
        avg: Option<String>,

        /// Comma-separated fields to take the minimum of
        #[arg(long)]
        min: Option<String>,

        /// Comma-separated fields to take the maximum of
        #[arg(long)]
        max: Option<String>,

        /// Comma-separated fields to sum
        #[arg(long)]
        sum: Option<String>,

        /// Filter groups by aggregate: count>5, avg(priority)>=2; comma-separated clauses are ANDed
        /// (native aggregate^field^operator^value also accepted)
        #[arg(long)]
        having: Option<String>,
    },
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
                TableCommands::List {
                    table,
                    query,
                    limit,
                    ..
                } => {
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
                TableCommands::List { full, preview, .. } => {
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
                TableCommands::Get { full, preview, .. } => {
                    assert!(!full);
                    assert!(preview);
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
                TableCommands::Stats {
                    table,
                    query,
                    group_by,
                    avg,
                    min,
                    max,
                    sum,
                    having,
                    ..
                } => {
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
