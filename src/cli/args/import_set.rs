use clap::{Args, Subcommand};

use crate::models::identifiers::{SysId, TableName};

const IMPORT_SET_AFTER_HELP: &str = "Examples:\n  snow-cli import-set load imp_user --data '{\"user_name\":\"snow-cli-user\",\"email\":\"snow-cli-user@example.com\"}'\n  echo '{\"user_name\":\"stdin-user\",\"email\":\"stdin-user@example.com\"}' | snow-cli import-set load imp_user\n  snow-cli import-set load imp_user --fail-on-error --data '{\"user_name\":\"ci-user\",\"email\":\"ci-user@example.com\"}'\n\nNotes:\n  - `import-set load` posts to /api/now/import/{table}.\n  - This endpoint also runs the staging table's transform map automatically, so a\n    successful load already transforms the row into the target table (verified\n    against a PDI: a load into a mapped staging table inserts the target record).\n  - Use `--fail-on-error` when row-level transform errors should make the command exit non-zero.";
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

    /// Transform staged data.
    ///
    /// Hidden: not implemented. `import-set load` already runs the staging
    /// table's transform map automatically (POST /api/now/import/{table}), so a
    /// separate transform trigger is not wired into the CLI. Kept as a hidden
    /// placeholder so the surface is reserved if a supported standalone REST
    /// transform trigger is added later.
    #[command(hide = true)]
    Transform {
        /// Import set sys_id
        sys_id: SysId,
    },
}
