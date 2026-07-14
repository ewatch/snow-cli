use clap::{Args, Subcommand};

const SNU_AFTER_HELP: &str = "Examples:\n  snow-cli snu check-connection --verify\n  snow-cli snu query incident --query 'active=true' --fields sys_id,number --limit 10\n  snow-cli snu get-record incident <sys_id> --fields sys_id,number,short_description\n  snow-cli snu update-record sys_script_include <sys_id> --field script --content 'gs.info(\"hello\")'\n  snow-cli snu execute-bg-script --code 'gs.info(\"hello from SN-Utils\")'\n  snow-cli --instance https://dev12345.service-now.com snu query incident --query 'active=true'\n  snow-cli snu broker status\n\nNotes:\n  - Requires the SN-Utils browser extension with its ScriptSync helper tab open. Commands auto-start a local broker on 127.0.0.1:1978; run /token in a ServiceNow tab when prompted. Stop sn-scriptsync first if it owns that port.\n  - When the browser holds sessions for several instances, pass the global --instance <url-or-host> to pick one; `snu broker status` lists live sessions and `snu broker clear` drops cached ones.\n  - `snu update-record` and `snu delete-record` run as server-side background scripts over the bridge; `snu delete-record --dry-run` only previews matches and never deletes.\n  - Full command list, token/session lifecycle, and troubleshooting: https://ewatch.github.io/snow-cli/commands/snu.html";
pub const DEFAULT_SNU_TIMEOUT_SECS: u64 = 180;
/// Default sysparm_fields used by snu record reads (query, get-record).
pub const DEFAULT_SNU_FIELDS: &str = "sys_id,number,short_description";
// --- SN-Utils bridge ---

#[derive(Args, Debug)]
#[command(after_help = SNU_AFTER_HELP)]
pub struct SnuArgs {
    #[command(subcommand)]
    pub command: SnuCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuCommands {
    /// Check whether the SN-Utils bridge and browser helper are connected
    CheckConnection {
        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
        /// Also probe ServiceNow with the cached session to prove the g_ck
        /// token is still valid (adds `token_valid` to the output)
        #[arg(long)]
        verify: bool,
    },

    /// Get SN-Utils bridge instance info
    GetInstanceInfo {
        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Wait for /token from SN-Utils and print the received browser session metadata
    WaitToken {
        /// Seconds to wait for the helper tab and /token message
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Query ServiceNow records through the active SN-Utils browser session
    Query {
        /// Table name
        table: String,

        /// Encoded query
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated sysparm_fields
        #[arg(long, default_value = DEFAULT_SNU_FIELDS)]
        fields: String,

        /// Maximum records to return
        #[arg(long, default_value = "10")]
        limit: u32,

        /// ORDERBY clause or field expression appended to sysparm_query
        #[arg(long)]
        order_by: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Execute a server-side background script through the active SN-Utils browser session
    ExecuteBgScript {
        /// Path to a script file to execute
        #[arg(long, short = 'f', group = "script_source")]
        file: Option<String>,

        /// Inline script code to execute
        #[arg(long, short = 'c', group = "script_source")]
        code: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Create a record through the active SN-Utils browser session
    CreateRecord {
        /// Table name
        table: String,

        /// JSON object of field/value pairs for the new record
        #[arg(long)]
        data: Option<String>,

        /// Application scope for the ACL/transaction context (sysparm_transaction_scope)
        #[arg(long)]
        scope: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Fetch a scoped application's artifacts/metadata (SN-Utils requestAppMeta)
    AppMeta {
        /// Application scope or sys_id (sysparm_transaction_scope)
        app_id: String,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// List available tables through the active SN-Utils browser session
    ListTables {
        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Fetch a single record through the active SN-Utils browser session
    GetRecord {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Optional comma-separated fields list
        #[arg(long)]
        fields: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Update one or more fields through the active SN-Utils browser session
    UpdateRecord {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// JSON object of field/value pairs to update (use for multiple fields)
        #[arg(long, group = "update_source")]
        data: Option<String>,

        /// Single field name to update (use with --content; convenient for large values)
        #[arg(long, group = "update_source", requires = "content")]
        field: Option<String>,

        /// New content for the field named by --field
        #[arg(long, requires = "field")]
        content: Option<String>,

        /// Confirm the updated values by reading them back
        #[arg(long = "await")]
        await_confirmation: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Delete a record through the active SN-Utils browser session
    DeleteRecord {
        /// Table name
        table: String,

        /// Record sys_id (single delete)
        #[arg(long, group = "delete_target")]
        sys_id: Option<String>,

        /// Encoded query (bulk delete)
        #[arg(long, group = "delete_target")]
        query: Option<String>,

        /// Required for bulk delete to confirm destructive intent
        #[arg(long)]
        confirm: bool,

        /// Maximum records to delete in bulk mode
        #[arg(long)]
        limit: Option<u32>,

        /// Preview without deleting
        #[arg(long)]
        dry_run: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Fetch table UI metadata through the active SN-Utils browser session
    Schema {
        /// Table name
        table: String,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Run an SN-Utils slash command in a ServiceNow browser tab
    Slash {
        /// Slash command, e.g. /tn or tn
        command: String,

        /// Browser tab URL pattern to target
        #[arg(long, default_value = "https://*.service-now.com/*")]
        url: String,

        /// Specific browser tab id to target
        #[arg(long)]
        tab_id: Option<u64>,

        /// Insert/show but do not auto-run the slash command
        #[arg(long)]
        no_auto_run: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Browser tab operations through SN-Utils
    Tab(SnuTabArgs),

    /// Switch browser session context through SN-Utils
    Context(SnuContextArgs),

    /// Capture a browser screenshot through SN-Utils
    Screenshot {
        /// Browser tab URL/pattern to capture
        #[arg(long)]
        url: Option<String>,

        /// Specific browser tab id to capture
        #[arg(long)]
        tab_id: Option<u64>,

        /// Output PNG path; defaults to the helper-provided filename
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Upload an attachment through the active SN-Utils browser session
    AttachmentUpload {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Path to file to upload
        #[arg(long, short)]
        file: String,

        /// Content type override
        #[arg(long)]
        content_type: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Inspect or stop the auto-started SN-Utils broker
    Broker(SnuBrokerArgs),
}

#[derive(Args, Debug)]
pub struct SnuBrokerArgs {
    #[command(subcommand)]
    pub command: SnuBrokerCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuBrokerCommands {
    /// Show broker status
    Status,

    /// Stop the running broker
    Stop,

    /// Clear cached browser sessions from broker memory
    Clear {
        /// Clear only this instance (URL or host); omit to clear all instances
        #[arg(long)]
        instance: Option<String>,
    },

    /// Run the broker server process
    #[command(hide = true)]
    Serve,
}

#[derive(Args, Debug)]
pub struct SnuTabArgs {
    #[command(subcommand)]
    pub command: SnuTabCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuTabCommands {
    /// Activate a browser tab matching a URL pattern
    Activate {
        /// URL or browser extension match pattern
        url: String,

        /// Reload after activation
        #[arg(long)]
        reload: bool,

        /// Wait for page load completion
        #[arg(long)]
        wait_for_load: bool,

        /// Open the URL if no tab matches
        #[arg(long)]
        open_if_not_found: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },
}

#[derive(Args, Debug)]
pub struct SnuContextArgs {
    #[command(subcommand)]
    pub command: SnuContextCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuContextCommands {
    /// Switch update set, application, or domain in the browser session
    Switch {
        /// Context type to switch
        #[arg(value_enum)]
        switch_type: SnuSwitchType,

        /// sys_id/app_id/domain value
        value: String,

        /// Do not reload a ServiceNow tab after switching
        #[arg(long)]
        no_reload_tab: bool,

        /// Browser tab URL pattern to reload
        #[arg(long, default_value = "https://*.service-now.com/*")]
        tab_url: String,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SnuSwitchType {
    Updateset,
    Application,
    Domain,
}

impl SnuSwitchType {
    pub fn as_action_value(&self) -> &'static str {
        match self {
            Self::Updateset => "updateset",
            Self::Application => "application",
            Self::Domain => "domain",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::{CommandFactory, Parser};

    #[test]
    fn test_snu_help_mentions_browser_helpers_and_background_scripts() {
        let help = Cli::command()
            .find_subcommand_mut("snu")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("snow-cli snu check-connection"));
        assert!(help.contains("snu execute-bg-script"));
        // The after-help stays condensed; deep operational detail lives in the
        // book, which the help must point at.
        assert!(help.contains("https://ewatch.github.io/snow-cli/commands/snu.html"));
    }

    #[test]
    fn test_parse_snu_execute_bg_script() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "execute-bg-script",
            "--code",
            "gs.info('hi')",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::ExecuteBgScript { code, file, .. } => {
                    assert_eq!(code, Some("gs.info('hi')".to_string()));
                    assert_eq!(file, None);
                }
                _ => panic!("Expected Snu ExecuteBgScript command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_check_connection() {
        let cli = Cli::parse_from(["snow-cli", "snu", "check-connection"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::CheckConnection {
                    timeout_secs,
                    verify,
                } => {
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                    assert!(!verify);
                }
                _ => panic!("Expected Snu CheckConnection command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_get_instance_info() {
        let cli = Cli::parse_from(["snow-cli", "snu", "get-instance-info"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::GetInstanceInfo { timeout_secs } => {
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu GetInstanceInfo command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_list_tables() {
        let cli = Cli::parse_from(["snow-cli", "snu", "list-tables"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::ListTables { timeout_secs } => {
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu ListTables command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_create_record() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "create-record",
            "incident",
            "--data",
            "{\"short_description\":\"hi\"}",
            "--scope",
            "x_app",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::CreateRecord {
                    table, data, scope, ..
                } => {
                    assert_eq!(table.as_str(), "incident");
                    assert_eq!(data.as_deref(), Some("{\"short_description\":\"hi\"}"));
                    assert_eq!(scope.as_deref(), Some("x_app"));
                }
                _ => panic!("Expected Snu CreateRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_app_meta() {
        let cli = Cli::parse_from(["snow-cli", "snu", "app-meta", "x_my_app"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::AppMeta {
                    app_id,
                    timeout_secs,
                } => {
                    assert_eq!(app_id, "x_my_app");
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu AppMeta command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_get_record() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "get-record",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--fields",
            "sys_id,number",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::GetRecord {
                    table,
                    sys_id,
                    fields,
                    ..
                } => {
                    assert_eq!(table.as_str(), "incident");
                    assert_eq!(sys_id.as_str(), "6816f79cc0a8016401c5a33be04be441");
                    assert_eq!(fields, Some("sys_id,number".to_string()));
                }
                _ => panic!("Expected Snu GetRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_update_record_with_data() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "update-record",
            "sp_widget",
            "6816f79cc0a8016401c5a33be04be441",
            "--data",
            "{\"script\":\"gs.info('x')\",\"css\":\".a{}\"}",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::UpdateRecord {
                    table,
                    sys_id,
                    data,
                    field,
                    content,
                    await_confirmation,
                    ..
                } => {
                    assert_eq!(table, "sp_widget");
                    assert_eq!(sys_id.as_str(), "6816f79cc0a8016401c5a33be04be441");
                    assert_eq!(
                        data.as_deref(),
                        Some("{\"script\":\"gs.info('x')\",\"css\":\".a{}\"}")
                    );
                    assert_eq!(field, None);
                    assert_eq!(content, None);
                    assert!(!await_confirmation);
                }
                _ => panic!("Expected Snu UpdateRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_update_record_single_field() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "update-record",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--field",
            "state",
            "--content",
            "2",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::UpdateRecord {
                    field,
                    content,
                    data,
                    ..
                } => {
                    assert_eq!(field.as_deref(), Some("state"));
                    assert_eq!(content.as_deref(), Some("2"));
                    assert_eq!(data, None);
                }
                _ => panic!("Expected Snu UpdateRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_update_record_data_and_field_conflict() {
        let result = Cli::try_parse_from([
            "snow-cli",
            "snu",
            "update-record",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--data",
            "{\"state\":\"2\"}",
            "--field",
            "state",
            "--content",
            "2",
        ]);
        assert!(
            result.is_err(),
            "--data and --field must be mutually exclusive"
        );
    }
}
