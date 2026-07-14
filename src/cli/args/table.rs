use clap::{Args, Subcommand};

use crate::models::identifiers::{SysId, TableName};

const TABLE_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 10\n  snow-cli table get incident <sys_id>\n  snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'\n  snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'\n  snow-cli table schema incident --extended\n  snow-cli table stats incident --group-by state";
const TABLE_LIST_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 20\n  snow-cli table list sys_user --fields sys_id,user_name,email --order-by user_name\n  snow-cli table list incident --all --fields '*' --full   # everything, uncapped\n\nNotes:\n  - Without --limit/--all, output is bounded to 20 records; without --fields, a compact table-aware field set is returned.\n  - Without --full, field values longer than 2000 chars are cut with an inline '[truncated N of M chars]' size hint, and the metadata carries fields_truncated=true.\n  - Responses include returned/truncated metadata plus the server-reported total, so truncation is always detectable.\n  - For complete data set extraction prefer `data export`.";

const TABLE_CREATE_AFTER_HELP: &str = "Examples:\n  snow-cli table create incident --data '{\"short_description\":\"VPN down\"}'\n  echo '{\"short_description\":\"From stdin\"}' | snow-cli table create incident";

const TABLE_STATS_AFTER_HELP: &str = "Examples:\n  snow-cli table stats incident\n  snow-cli table stats incident --query 'active=true'\n  snow-cli table stats incident --group-by state,priority\n  snow-cli table stats incident --group-by state --avg priority --having 'count>5'";
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

        /// Fetch every matching record instead of the bounded default
        #[arg(long)]
        all: bool,

        /// Field to order results by
        #[arg(long)]
        order_by: Option<String>,

        /// Return complete field content instead of capping long values
        #[arg(long)]
        full: bool,
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

        /// Return complete field content instead of capping long values
        #[arg(long)]
        full: bool,
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

        /// Include fields inherited from parent tables (e.g., incident inherits from task)
        #[arg(long)]
        include_inherited: bool,
    },

    /// Count and aggregate records via the Aggregate API (Stats endpoint)
    #[command(after_help = TABLE_STATS_AFTER_HELP)]
    Stats {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: TableName,

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

        /// Aggregate filter clause (e.g., 'count>5')
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
    fn test_parse_table_list_and_get_full_flag() {
        let cli = Cli::parse_from(["snow-cli", "table", "list", "incident", "--full"]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::List { full, .. } => assert!(full),
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
        ]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::Get { full, .. } => assert!(!full),
                _ => panic!("Expected Table Get command"),
            },
            _ => panic!("Expected Table command"),
        }
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
