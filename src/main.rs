#[allow(dead_code)]
mod auth;
mod cli;
#[allow(dead_code)]
mod client;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod models;

use clap::Parser;
use cli::args::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing based on verbosity level
    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    tracing::debug!("snow-cli starting with verbosity level {}", cli.verbose);

    match cli.command {
        cli::args::Commands::Config(args) => {
            cli::commands::config::handle(args, &cli.profile, &cli.output).await
        }
        cli::args::Commands::Auth(args) => cli::commands::auth::handle(args, &cli.profile).await,
        cli::args::Commands::Table(args) => cli::commands::table::handle(args).await,
        cli::args::Commands::Incident(args) => cli::commands::incident::handle(args).await,
        cli::args::Commands::Attachment(args) => cli::commands::attachment::handle(args).await,
        cli::args::Commands::ImportSet(args) => cli::commands::import_set::handle(args).await,
        cli::args::Commands::Api(args) => cli::commands::api::handle(args).await,
        cli::args::Commands::Completions { shell } => cli::commands::completions::handle(shell),
    }
}
