use crate::cli::args::ImportSetArgs;

pub async fn handle(args: ImportSetArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::ImportSetCommands::Load { table, .. } => {
            tracing::info!("Loading data into staging table: {}", table);
            todo!("import-set load not yet implemented")
        }
        crate::cli::args::ImportSetCommands::Transform { sys_id } => {
            tracing::info!("Transforming import set: {}", sys_id);
            todo!("import-set transform not yet implemented")
        }
    }
}
