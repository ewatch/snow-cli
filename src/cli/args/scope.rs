use clap::{Args, Subcommand};

use crate::models::identifiers::{EncodedQueryValue, SysId, TableName};

const SCOPE_AFTER_HELP: &str = "Examples:\n  snow-cli scope list\n  snow-cli scope list incident\n  snow-cli scope list sn_ot_incident_mgmt\n  snow-cli scope inspect x_my_app\n  snow-cli scope inspect 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --details full\n  snow-cli scope move-file sys_script_include 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --target-scope x_target_app --dry-run\n  snow-cli scope move-file sys_script_include 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --target-scope x_target_app --yes";
// --- Scope ---

#[derive(Args, Debug)]
#[command(after_help = SCOPE_AFTER_HELP)]
pub struct ScopeArgs {
    #[command(subcommand)]
    pub command: ScopeCommands,
}

#[derive(Subcommand, Debug)]
pub enum ScopeCommands {
    /// List scopes and classify them by origin
    List {
        /// Optional search term for partial name matches or exact scope names
        search: Option<EncodedQueryValue>,

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
        scope: EncodedQueryValue,

        /// Detail level for output payload
        #[arg(long, value_enum, default_value = "basic")]
        details: ScopeDetailLevel,
    },

    /// Export normalized scope artifacts for analysis
    Inventory {
        /// Scope name (e.g., x_my_app) or scope sys_id
        scope: EncodedQueryValue,
    },

    /// Move one application file to a different custom scope without changing sys_id
    MoveFile {
        /// Source table name for the application file
        table: TableName,

        /// Source record sys_id
        sys_id: SysId,

        /// Target scope name (e.g., x_my_app) or scope sys_id
        #[arg(long = "target-scope")]
        target_scope: String,

        /// Validate and preview the move without persisting changes
        #[arg(long)]
        dry_run: bool,

        /// Confirm execution when warnings are reported
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ScopeDetailLevel {
    Basic,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum ScopeListKind {
    StoreApp,
    Plugin,
    CustomApp,
    Platform,
    PlatformApp,
}

impl ScopeListKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StoreApp => "store_app",
            Self::Plugin => "plugin",
            Self::CustomApp => "custom_app",
            Self::Platform => "platform",
            Self::PlatformApp => "platform_app",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_parse_scope_inspect_defaults_to_basic_details() {
        let cli = Cli::parse_from(["snow-cli", "scope", "inspect", "x_my_app"]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::Inspect { scope, details } => {
                    assert_eq!(scope.as_str(), "x_my_app");
                    assert!(matches!(details, ScopeDetailLevel::Basic));
                }
                _ => panic!("Expected Scope Inspect command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_list_with_search() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "list",
            "global",
            "--kind",
            "plugin",
            "--kind",
            "store-app",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::List {
                    search,
                    kind,
                    show_source_table,
                    show_sys_id,
                } => {
                    assert_eq!(search.map(|v| v.to_string()), Some("global".to_string()));
                    assert_eq!(kind, vec![ScopeListKind::Plugin, ScopeListKind::StoreApp]);
                    assert!(!show_source_table);
                    assert!(!show_sys_id);
                }
                _ => panic!("Expected Scope List command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_list_text_column_flags() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "list",
            "incident",
            "--show-source-table",
            "--show-sys-id",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::List {
                    search,
                    show_source_table,
                    show_sys_id,
                    ..
                } => {
                    assert_eq!(search.map(|v| v.to_string()), Some("incident".to_string()));
                    assert!(show_source_table);
                    assert!(show_sys_id);
                }
                _ => panic!("Expected Scope List command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_inspect_full_details() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "inspect",
            "x_my_app",
            "--details",
            "full",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::Inspect { details, .. } => {
                    assert!(matches!(details, ScopeDetailLevel::Full));
                }
                _ => panic!("Expected Scope Inspect command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_inventory() {
        let cli = Cli::parse_from(["snow-cli", "scope", "inventory", "x_my_app"]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::Inventory { scope } => {
                    assert_eq!(scope.as_str(), "x_my_app");
                }
                _ => panic!("Expected Scope Inventory command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_move_file() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "move-file",
            "sys_script_include",
            "6816f79cc0a8016401c5a33be04be441",
            "--target-scope",
            "x_target_app",
            "--dry-run",
            "--yes",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::MoveFile {
                    table,
                    sys_id,
                    target_scope,
                    dry_run,
                    yes,
                } => {
                    assert_eq!(table.as_str(), "sys_script_include");
                    assert_eq!(sys_id.as_str(), "6816f79cc0a8016401c5a33be04be441");
                    assert_eq!(target_scope, "x_target_app");
                    assert!(dry_run);
                    assert!(yes);
                }
                _ => panic!("Expected Scope MoveFile command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }
}
