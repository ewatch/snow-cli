//! `ping` — one cheap authenticated round trip that reports connectivity,
//! the session user, and the instance build.

use std::time::Instant;

use crate::cli::args::OutputFormat;
use crate::cli::output;

#[derive(Debug, serde::Serialize)]
struct PingResult {
    instance: String,
    user: Option<String>,
    user_sys_id: Option<String>,
    /// Best-effort; `null` when `sys_properties` is not readable (common for non-admin users).
    build: Option<String>,
    /// Round trip of the identity request, including any token acquisition.
    latency_ms: u64,
}

pub async fn handle(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

    let started = Instant::now();
    let user = client.current_user().await?;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let build = client.build_tag().await.unwrap_or_else(|error| {
        tracing::debug!(error = %error, "Instance build tag is not readable");
        None
    });

    let result = PingResult {
        instance: client.base_url().to_string(),
        user: user.as_ref().map(|user| user.user_name.clone()),
        user_sys_id: user.map(|user| user.sys_id),
        build,
        latency_ms,
    };
    output::print_output(&result, format)
}
