use crate::cli::args::TableArgs;

pub async fn handle(args: TableArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::TableCommands::List { table, .. } => {
            tracing::info!("Listing records from table: {}", table);
            todo!("table list not yet implemented")
        }
        crate::cli::args::TableCommands::Get { table, sys_id, .. } => {
            tracing::info!("Getting record {} from table: {}", sys_id, table);
            todo!("table get not yet implemented")
        }
        crate::cli::args::TableCommands::Create { table, .. } => {
            tracing::info!("Creating record in table: {}", table);
            todo!("table create not yet implemented")
        }
        crate::cli::args::TableCommands::Update { table, sys_id, .. } => {
            tracing::info!("Updating record {} in table: {}", sys_id, table);
            todo!("table update not yet implemented")
        }
        crate::cli::args::TableCommands::Delete { table, sys_id, .. } => {
            tracing::info!("Deleting record {} from table: {}", sys_id, table);
            todo!("table delete not yet implemented")
        }
    }
}
