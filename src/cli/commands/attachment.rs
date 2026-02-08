use crate::cli::args::AttachmentArgs;

pub async fn handle(args: AttachmentArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::AttachmentCommands::List { table, sys_id } => {
            tracing::info!("Listing attachments for {}/{}", table, sys_id);
            todo!("attachment list not yet implemented")
        }
        crate::cli::args::AttachmentCommands::Download { sys_id, .. } => {
            tracing::info!("Downloading attachment: {}", sys_id);
            todo!("attachment download not yet implemented")
        }
        crate::cli::args::AttachmentCommands::Upload {
            table,
            sys_id,
            file,
        } => {
            tracing::info!("Uploading {} to {}/{}", file, table, sys_id);
            todo!("attachment upload not yet implemented")
        }
    }
}
