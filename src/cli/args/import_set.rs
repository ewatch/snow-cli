use clap::{Args, Subcommand};

use crate::models::identifiers::{SysId, TableName};

const IMPORT_SET_AFTER_HELP: &str = "Examples:\n  snow-cli import-set load imp_user --data '{\"user_name\":\"snow-cli-user\",\"email\":\"snow-cli-user@example.com\"}'\n  echo '{\"user_name\":\"stdin-user\",\"email\":\"stdin-user@example.com\"}' | snow-cli import-set load imp_user\n  snow-cli import-set load imp_user --fail-on-error --data '{\"user_name\":\"ci-user\",\"email\":\"ci-user@example.com\"}'\n\nNotes:\n  - `import-set load` posts to /api/now/import/{table}.\n  - On the validated `sprint` instance, this endpoint also ran the transform map automatically for `imp_user`.\n  - Use `--fail-on-error` when row-level transform errors should make the command exit non-zero.\n  - `import-set transform` remains a placeholder until a supported separate transform trigger is implemented.";
// --- Import Set ---

#[derive(Args, Debug)]
#[command(after_help = IMPORT_SET_AFTER_HELP)]
pub struct ImportSetArgs {
    #[command(subcommand)]
    pub command: ImportSetCommands,
}

#[derive(Subcommand, Debug)]
pub enum ImportSetCommands {
    /// Load data into a staging table
    Load {
        /// Staging table name
        table: TableName,

        /// JSON data to load
        #[arg(long)]
        data: Option<String>,

        /// Exit non-zero when the import response contains row-level errors
        #[arg(long)]
        fail_on_error: bool,
    },

    /// Transform staged data
    Transform {
        /// Import set sys_id
        sys_id: SysId,
    },
}
