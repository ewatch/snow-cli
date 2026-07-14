use std::path::{Path, PathBuf};

use crate::auth::oauth2::validate_oauth_redirect_host;
use crate::cli::args::{
    CliAuthMethod, CliOAuthGrantType, ConfigArgs, ConfigCommands, OutputFormat, ProfileSdkCommands,
};
use crate::cli::output;
use crate::config::credentials;
use crate::config::now_sdk;
use crate::config::profile::{
    AppConfig, AuthMethod, OAuthGrantType, Profile, validate_instance_url,
};

/// Convert CLI auth method enum to config auth method enum.
fn to_auth_method(cli: &CliAuthMethod) -> AuthMethod {
    match cli {
        CliAuthMethod::Basic => AuthMethod::Basic,
        CliAuthMethod::Oauth2 => AuthMethod::Oauth2,
        CliAuthMethod::ApiKey => AuthMethod::ApiKey,
        CliAuthMethod::Mtls => AuthMethod::Mtls,
        CliAuthMethod::BrowserSession => AuthMethod::BrowserSession,
    }
}

/// Convert CLI OAuth grant type to config OAuth grant type.
fn to_oauth_grant_type(cli: &CliOAuthGrantType) -> OAuthGrantType {
    match cli {
        CliOAuthGrantType::ClientCredentials => OAuthGrantType::ClientCredentials,
        CliOAuthGrantType::Password => OAuthGrantType::Password,
        CliOAuthGrantType::AuthorizationCode => OAuthGrantType::AuthorizationCode,
    }
}

pub async fn handle(
    args: ConfigArgs,
    active_profile: &str,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let config_path = AppConfig::config_path();
    match args.command {
        ConfigCommands::Init {
            instance,
            auth_method,
            username,
            client_id,
            oauth_grant_type,
            oauth_scope,
            oauth_redirect_host,
            oauth_redirect_port,
            oauth_redirect_path,
            name,
        } => {
            handle_init_with_oauth_options(
                &config_path,
                instance,
                auth_method,
                username,
                client_id,
                oauth_grant_type,
                oauth_scope,
                oauth_redirect_host,
                oauth_redirect_port,
                oauth_redirect_path,
                name,
            )
            .await
        }
        ConfigCommands::Add {
            name,
            instance,
            auth_method,
            username,
            client_id,
            oauth_grant_type,
            oauth_scope,
            oauth_redirect_host,
            oauth_redirect_port,
            oauth_redirect_path,
            cert_path,
            key_path,
            sso_login_url,
        } => {
            handle_add_profile_with_oauth_options(
                &config_path,
                name,
                instance,
                auth_method,
                username,
                client_id,
                oauth_grant_type,
                oauth_scope,
                oauth_redirect_host,
                oauth_redirect_port,
                oauth_redirect_path,
                cert_path,
                key_path,
                sso_login_url,
            )
            .await
        }
        ConfigCommands::Edit {
            name,
            instance,
            auth_method,
            username,
            client_id,
            oauth_grant_type,
            oauth_scope,
            oauth_redirect_host,
            oauth_redirect_port,
            oauth_redirect_path,
            cert_path,
            key_path,
            sso_login_url,
        } => {
            handle_edit_profile_with_oauth_options(
                &config_path,
                name,
                instance,
                auth_method,
                username,
                client_id,
                oauth_grant_type,
                oauth_scope,
                oauth_redirect_host,
                oauth_redirect_port,
                oauth_redirect_path,
                cert_path,
                key_path,
                sso_login_url,
            )
            .await
        }
        ConfigCommands::SetProfile {
            name,
            instance,
            auth_method,
            username,
            client_id,
            oauth_grant_type,
            oauth_scope,
            oauth_redirect_host,
            oauth_redirect_port,
            oauth_redirect_path,
            cert_path,
            key_path,
            sso_login_url,
        } => {
            handle_set_profile_with_oauth_options(
                &config_path,
                name,
                instance,
                auth_method,
                username,
                client_id,
                oauth_grant_type,
                oauth_scope,
                oauth_redirect_host,
                oauth_redirect_port,
                oauth_redirect_path,
                cert_path,
                key_path,
                sso_login_url,
            )
            .await
        }
        ConfigCommands::ListProfiles => handle_list_profiles(&config_path, output_format).await,
        ConfigCommands::FindProfile { instance } => {
            handle_find_profile(&config_path, output_format, instance).await
        }
        ConfigCommands::Sdk(sdk_args) => match sdk_args.command {
            ProfileSdkCommands::List => handle_list_now_sdk_profiles(output_format).await,
            ProfileSdkCommands::Import {
                alias,
                all,
                set_default,
            } => handle_import_now_sdk(&config_path, output_format, alias, all, set_default).await,
            ProfileSdkCommands::Export {
                profile,
                alias,
                set_default,
            } => {
                handle_export_now_sdk(&config_path, output_format, profile, alias, set_default)
                    .await
            }
        },
        ConfigCommands::ListNowSdkProfiles => handle_list_now_sdk_profiles(output_format).await,
        ConfigCommands::ImportNowSdk {
            alias,
            all,
            set_default,
        } => handle_import_now_sdk(&config_path, output_format, alias, all, set_default).await,
        ConfigCommands::ExportNowSdk {
            profile,
            alias,
            set_default,
        } => handle_export_now_sdk(&config_path, output_format, profile, alias, set_default).await,
        ConfigCommands::UseProfile { name } => handle_default_profile(&config_path, name).await,
        ConfigCommands::Current => {
            handle_current(&config_path, active_profile, output_format).await
        }
        ConfigCommands::Show => handle_show(&config_path, active_profile, output_format).await,
        ConfigCommands::DeleteProfile {
            name,
            yes,
            new_default,
        } => handle_delete_profile(&config_path, name, yes, new_default).await,
        ConfigCommands::Output { format, reset } => {
            handle_output_default(&config_path, format, reset).await
        }
    }
}

#[path = "now_sdk.rs"]
mod now_sdk_commands;
#[path = "output.rs"]
mod output_commands;
mod profiles;

use now_sdk_commands::*;
use output_commands::*;
use profiles::*;

#[cfg(test)]
mod tests;
