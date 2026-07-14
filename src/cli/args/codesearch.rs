use clap::{Args, Subcommand};

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
        /// Search query text
        query: String,

        /// Limit to a specific table (e.g., sys_script_include, sys_script, sysevent_script_action)
        #[arg(long = "source-table", alias = "table")]
        source_table: Option<String>,

        /// Restrict search to a specific application scope (e.g., x_my_app, global)
        #[arg(long)]
        scope: Option<String>,

        /// Maximum number of results to return (default: 100)
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
