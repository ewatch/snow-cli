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
use std::io::IsTerminal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = config::AppConfig::load()?;
    let active_profile = match &cli.command {
        cli::args::Commands::Config(args) => match &args.command {
            cli::args::ConfigCommands::Show => {
                config.resolve_active_profile_name(cli.profile.as_deref())?
            }
            _ => cli
                .profile
                .clone()
                .or_else(|| {
                    if config.default_profile.is_empty() {
                        Some("default".to_string())
                    } else {
                        Some(config.default_profile.clone())
                    }
                })
                .unwrap_or_else(|| "default".to_string()),
        },
        _ => config.resolve_active_profile_name(cli.profile.as_deref())?,
    };

    // Initialize tracing based on verbosity level
    let base_filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let filter = if client::is_http_debug_enabled() {
        format!("{base_filter},snow_cli::http=debug")
    } else {
        base_filter.to_string()
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    tracing::debug!("snow-cli starting with verbosity level {}", cli.verbose);

    if should_show_profile_hint() {
        eprintln!("\x1b[32m[profile: {}]\x1b[0m", active_profile);
    }

    match cli.command {
        cli::args::Commands::Config(args) => {
            cli::commands::config::handle(args, &active_profile, &cli.output).await
        }
        cli::args::Commands::Auth(args) => cli::commands::auth::handle(args, &active_profile).await,
        cli::args::Commands::Table(args) => {
            cli::commands::table::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Data(args) => {
            cli::commands::data::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Seed(args) => {
            cli::commands::seed::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Scope(args) => {
            cli::commands::scope::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Attachment(args) => {
            cli::commands::attachment::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::ImportSet(args) => {
            cli::commands::import_set::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Api(args) => {
            cli::commands::api::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Script(args) => {
            cli::commands::script::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Codesearch(args) => {
            cli::commands::codesearch::handle(
                args,
                &active_profile,
                &cli.output,
                cli.instance.as_deref(),
                cli.timeout_secs,
            )
            .await
        }
        cli::args::Commands::Completions { shell } => cli::commands::completions::handle(shell),
    }
}

fn should_show_profile_hint() -> bool {
    if !std::io::stderr().is_terminal() {
        return false;
    }
    if let Ok(value) = std::env::var("SNOW_CLI_PROFILE_HINT") {
        let normalized = value.trim().to_ascii_lowercase();
        return normalized != "0" && normalized != "false" && normalized != "off";
    }
    true
}
