use clap::{Args, Subcommand};

use crate::models::identifiers::{SysId, TableName};

const IMPORT_SET_AFTER_HELP: &str = "Examples:\n  snow-cli import-set load imp_user --data '{\"user_id\":\"snow-cli-user\",\"email\":\"snow-cli-user@example.com\"}'\n  echo '{\"user_id\":\"stdin-user\",\"email\":\"stdin-user@example.com\"}' | snow-cli import-set load imp_user\n  snow-cli import-set load imp_user --fail-on-error --data '{\"user_id\":\"ci-user\",\"email\":\"ci-user@example.com\"}'\n\nNotes:\n  - `import-set load` posts to /api/now/import/{table}.\n  - This endpoint also runs the staging table's transform map automatically, so a\n    successful load already transforms the row into the target table (verified\n    against a PDI: a load into a mapped staging table inserts the target record).\n  - Output formats: json, jsonl, toon, text, auto. CSV is rejected before reading\n    input or sending an import request; use --output json to override CSV defaults.\n  - Row-level transform errors exit non-zero by default; results are still printed.\n  - `--allow-row-errors` explicitly accepts partial failures (exit 0 with a warning).\n  - A failed load may already have inserted/updated records; do not blindly retry.";
// --- Import Set ---

#[derive(Args, Debug)]
#[command(after_help = IMPORT_SET_AFTER_HELP)]
pub struct ImportSetArgs {
    #[command(subcommand)]
    pub command: ImportSetCommands,
}

#[derive(Subcommand, Debug)]
pub enum ImportSetCommands {
    /// Load data into a staging table (row-level errors fail by default)
    #[command(after_help = IMPORT_SET_AFTER_HELP)]
    Load {
        /// Staging table name
        table: TableName,

        /// JSON data to load
        #[arg(long)]
        data: Option<String>,

        /// Exit non-zero on row-level errors (compatibility flag; now the default)
        #[arg(long, conflicts_with = "allow_row_errors")]
        fail_on_error: bool,

        /// Accept row-level errors and exit 0 with a warning; results are still printed
        #[arg(long, conflicts_with = "fail_on_error")]
        allow_row_errors: bool,
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
