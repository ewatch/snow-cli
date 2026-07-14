use clap::{Args, Subcommand};

use crate::models::identifiers::{SysId, TableName};

// --- Attachment ---

#[derive(Args, Debug)]
pub struct AttachmentArgs {
    #[command(subcommand)]
    pub command: AttachmentCommands,
}

#[derive(Subcommand, Debug)]
pub enum AttachmentCommands {
    /// List attachments for a record
    List {
        /// Table name
        table: TableName,

        /// Record sys_id
        sys_id: SysId,
    },

    /// Download an attachment
    Download {
        /// Attachment sys_id
        sys_id: SysId,

        /// Output file path (defaults to original filename)
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,
    },

    /// Upload a file as an attachment
    Upload {
        /// Table name
        table: TableName,

        /// Record sys_id
        sys_id: SysId,

        /// Path to the file to upload
        #[arg(long, short)]
        file: String,
    },
}
