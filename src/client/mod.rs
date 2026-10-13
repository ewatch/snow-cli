#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "src/client is the deliberate HTTP transport adapter"
)]

pub mod error;
pub mod pagination;

mod core;
mod debug;
mod identity;
mod native_session;
mod script_response;
mod session;
mod session_bootstrap;
mod table;
#[cfg(test)]
mod test_support;
mod transport;

pub use core::{
    BackgroundScriptOptions, ClientConfig, ClientResponse, ExternalResponse, FormSession,
    SnowClient,
};
pub use identity::CurrentUser;
pub use native_session::NativeSessionSwitchError;
pub use script_response::BackgroundScriptResponse;
pub use transport::{fetch_skill_resource, post_oauth_token_form};

pub(crate) use core::resolve_authenticated_url;
pub(crate) use debug::{
    is_http_debug_enabled, is_http_debug_sensitive_enabled, log_raw_http_request,
    log_raw_http_response,
};
pub(crate) use session::{extract_g_ck_from_body, extract_jsessionid_from_headers};

/// Build an authenticated [`SnowClient`] from the user's configuration.
///
/// Loads the config, resolves the active profile, creates the appropriate
/// authenticator, and constructs the client. An optional `instance_override`
/// (from `--instance` CLI flag) replaces the profile's instance URL.
pub fn build_client(
    profile_name: &str,
    instance_override: Option<&str>,
) -> anyhow::Result<SnowClient> {
    build_client_with_timeout(profile_name, instance_override, None)
}

pub fn build_client_with_timeout(
    profile_name: &str,
    instance_override: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<SnowClient> {
    let config = crate::config::AppConfig::load()?;
    let profile = config.active_profile(Some(profile_name)).ok_or_else(|| {
        crate::config::error::ConfigError::new(
            "PROFILE_NOT_FOUND",
            config.profile_not_found_message(profile_name),
        )
    })?;

    let instance_url = crate::config::profile::validate_instance_url(
        instance_override.unwrap_or(&profile.instance),
    )?;

    let authenticator = if profile.auth_method == crate::config::AuthMethod::NativeSession {
        crate::auth::native_session::NativeSessionAuth::stored(profile_name, &instance_url)
            .map(|auth| Box::new(auth) as Box<dyn crate::auth::Authenticator>)
    } else {
        crate::auth::create_authenticator(profile_name, profile)
    }
    .map_err(crate::auth::classify_auth_failure)?;
    SnowClient::with_config(
        instance_url,
        authenticator,
        ClientConfig {
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
            // Snapshot the active policy here, the single place that reads the
            // process-global. The policy then travels with the client.
            policy: crate::policy::active_policy(),
        },
    )
}

/// Default request timeout in seconds. ServiceNow table, script, and attachment
/// endpoints can legitimately spend tens of seconds on ACL evaluation, query
/// planning, or script execution; keep this high enough for those workflows
/// while still bounding hung network calls for automation.
const DEFAULT_TIMEOUT_SECS: u64 = 90;

const FORM_SCRIPT_ENDPOINT: &str = "/sys.scripts.do";
const FORM_SCRIPT_BOOTSTRAP_PATH: &str = "/sys.scripts.modern.do";
