use crate::cli::args::AttachmentArgs;

pub async fn handle(args: AttachmentArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::AttachmentCommands::List { table, sys_id } => {
            tracing::info!("Listing attachments for {}/{}", table, sys_id);
            anyhow::bail!(
                "`attachment list` is not implemented yet. For now, use the raw API: \
                 snow-cli api get '/api/now/attachment?sysparm_query=table_name={table}^table_sys_id={sys_id}'"
            )
        }
        crate::cli::args::AttachmentCommands::Download { sys_id, .. } => {
            tracing::info!("Downloading attachment: {}", sys_id);
            anyhow::bail!(
                "`attachment download` is not implemented yet. For now, fetch metadata via: \
                 snow-cli api get '/api/now/attachment/{sys_id}'"
            )
        }
        crate::cli::args::AttachmentCommands::Upload {
            table,
            sys_id,
            file,
        } => {
            tracing::info!("Uploading {} to {}/{}", file, table, sys_id);
            anyhow::bail!(
                "`attachment upload` is not implemented yet. Use `snow-cli api post` with \
                 multipart data as a temporary workaround."
            )
        }
    }
}
