use clap::{Args, Subcommand};

use crate::models::identifiers::{SysId, TableName};

// --- Attachment ---

#[derive(Args, Debug)]
pub struct AttachmentArgs {
    #[command(subcommand)]
    pub command: AttachmentCommands,
}

#[derive(Args, Debug)]
pub struct AttachmentListArgs {
    /// Table name
    pub table: TableName,

    /// Record sys_id
    pub sys_id: SysId,

    /// Maximum number of attachments to return (default: 20)
    #[arg(long, conflicts_with = "all")]
    pub limit: Option<usize>,

    /// Skip this many attachments (default: 0)
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Fetch every attachment instead of the bounded default
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct AttachmentDownloadArgs {
    /// Attachment sys_id
    pub sys_id: SysId,

    /// Output file path (defaults to original filename)
    #[arg(long = "out", short = 'o')]
    pub out_path: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum AttachmentCommands {
    /// List attachments for a record
    List(AttachmentListArgs),

    /// Download an attachment
    Download(AttachmentDownloadArgs),

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
