#![allow(dead_code)]

pub mod auth;
pub mod cli;
pub mod client;
pub mod config;
pub mod error;
pub mod models;
pub mod policy;

use clap::Parser;
use cli::args::Cli;
use cli::readonly_args::ReadOnlyCli;
use policy::{ExecutionPolicy, PolicyMode};
use std::io::IsTerminal;

/// Parse command-line arguments and run the full snow-cli command surface.
pub async fn run_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let policy = if cli.read_only {
        ExecutionPolicy::read_only()
    } else {
        ExecutionPolicy::full_access()
    };
    policy::set_active_policy(policy);
    run_parsed_cli(cli, policy).await
}

/// Parse command-line arguments and run the reduced read-only command surface.
pub async fn run_read_only_cli() -> anyhow::Result<()> {
    let ro_cli = ReadOnlyCli::parse();
    if let cli::readonly_args::ReadOnlyCommands::Completions { shell } = ro_cli.command {
        cli::readonly_args::generate_completions(shell);
        return Ok(());
    }

    let cli = ro_cli.into_full_cli();
    let policy = ExecutionPolicy::read_only();
    policy::set_active_policy(policy);
    run_parsed_cli(cli, policy).await
}

async fn run_parsed_cli(cli: Cli, policy: ExecutionPolicy) -> anyhow::Result<()> {
    let command_uses_connection = command_uses_connection(&cli.command);

    // Check command policy before loading config, resolving credentials, or dispatching handlers.
    policy.ensure_command_allowed(&cli.command)?;

    let config = config::AppConfig::load()?;
    let active_profile = match &cli.command {
        cli::args::Commands::Profile(args) => match &args.command {
            cli::args::ConfigCommands::Show | cli::args::ConfigCommands::Current => {
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
        cli::args::Commands::Completions { .. } => "default".to_string(),
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

    tracing::debug!(
        verbosity = cli.verbose,
        policy_mode = ?policy.mode(),
        "snow-cli starting"
    );

    if policy.mode() == PolicyMode::ReadOnly {
        tracing::debug!("read-only policy is active");
    }

    if command_uses_connection && should_show_profile_hint() {
        eprintln!("\x1b[32mUsing profile: {}\x1b[0m", active_profile);
    }

    match cli.command {
        cli::args::Commands::Profile(args) => {
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

pub fn command_uses_connection(command: &cli::args::Commands) -> bool {
    matches!(
        command,
        cli::args::Commands::Auth(_)
            | cli::args::Commands::Table(_)
            | cli::args::Commands::Data(_)
            | cli::args::Commands::Seed(_)
            | cli::args::Commands::Scope(_)
            | cli::args::Commands::Attachment(_)
            | cli::args::Commands::ImportSet(_)
            | cli::args::Commands::Api(_)
            | cli::args::Commands::Script(_)
            | cli::args::Commands::Codesearch(_)
    )
}

pub fn should_show_profile_hint() -> bool {
    if !std::io::stderr().is_terminal() {
        return false;
    }
    if let Ok(value) = std::env::var("SNOW_CLI_PROFILE_HINT") {
        let normalized = value.trim().to_ascii_lowercase();
        return normalized != "0" && normalized != "false" && normalized != "off";
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::Shell;
    use cli::args::{
        AuthArgs, AuthCommands, Commands, ConfigArgs, ConfigCommands, TableArgs, TableCommands,
    };

    #[test]
    fn connection_commands_show_profile_hint() {
        assert!(command_uses_connection(&Commands::Auth(AuthArgs {
            command: AuthCommands::Status,
        })));
        assert!(command_uses_connection(&Commands::Table(TableArgs {
            command: TableCommands::List {
                table: "incident".to_string(),
                query: None,
                fields: None,
                limit: None,
                order_by: None,
            },
        })));
    }

    #[test]
    fn non_connection_commands_do_not_show_profile_hint() {
        assert!(!command_uses_connection(&Commands::Profile(ConfigArgs {
            command: ConfigCommands::ListProfiles,
        })));
        assert!(!command_uses_connection(&Commands::Profile(ConfigArgs {
            command: ConfigCommands::Show,
        })));
        assert!(!command_uses_connection(&Commands::Completions {
            shell: Shell::Bash,
        }));
    }
}
