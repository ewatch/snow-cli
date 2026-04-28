use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::IsTerminal;

use rand::RngCore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::time::{Duration, Instant, sleep, timeout};

use crate::auth::Authenticator;
use crate::auth::oauth2::{
    OAuth2Auth, authorization_url, exchange_authorization_code, oauth_redirect_host,
    oauth_redirect_path, oauth_redirect_port, pkce_code_challenge_s256,
};
use crate::cli::args::{AuthArgs, AuthCommands};
use crate::config::credentials;
use crate::config::now_sdk;
use crate::config::profile::{AppConfig, AuthMethod, OAuthGrantType, Profile};

const SAML_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const SAML_LOGIN_POLL_INTERVAL: Duration = Duration::from_secs(2);
const OAUTH_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, serde::Deserialize)]
struct AgentBrowserCookiesEnvelope {
    success: bool,
    data: AgentBrowserCookiesData,
}

#[derive(Debug, serde::Deserialize)]
struct AgentBrowserCookiesData {
    cookies: Vec<AgentBrowserCookie>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AgentBrowserCookie {
    name: String,
    value: String,
    domain: String,
}

pub async fn handle(args: AuthArgs, profile_name: &str) -> anyhow::Result<()> {
    match args.command {
        AuthCommands::Login {
            password,
            token,
            client_secret,
            session_cookie,
            no_browser,
            also_now_sdk,
            now_sdk_alias,
            set_now_sdk_default,
        } => {
            handle_login(
                profile_name,
                password,
                token,
                client_secret,
                session_cookie,
                no_browser,
                also_now_sdk,
                now_sdk_alias,
                set_now_sdk_default,
            )
            .await
        }
        AuthCommands::Logout => handle_logout(profile_name).await,
        AuthCommands::Status => handle_status(profile_name).await,
        AuthCommands::Token => handle_token(profile_name).await,
    }
}

/// `auth login` — Store credentials for the active profile.
///
/// Credentials are read from flags (--password, --token, --client-secret)
/// or prompted interactively when stdin is a TTY. The credential type is
/// determined by the profile's auth_method.
///
/// For OAuth2 password grant, both `--client-secret` and `--password` are required
/// (two separate keychain entries).
#[allow(clippy::too_many_arguments)]
async fn handle_login(
    profile_name: &str,
    password: Option<String>,
    token: Option<String>,
    client_secret: Option<String>,
    session_cookie: Option<String>,
    no_browser: bool,
    also_now_sdk: bool,
    now_sdk_alias: Option<String>,
    set_now_sdk_default: bool,
) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("{}", config.profile_not_found_message(profile_name)))?;

    if set_now_sdk_default && !also_now_sdk {
        anyhow::bail!("`--set-now-sdk-default` requires `--also-now-sdk`.");
    }

    if also_now_sdk && profile.auth_method != crate::config::profile::AuthMethod::Basic {
        anyhow::bail!(
            "`--also-now-sdk` is only supported for basic auth profiles in this release."
        );
    }

    let is_tty = std::io::stdin().is_terminal();

    match &profile.auth_method {
        AuthMethod::Basic => {
            let pw = resolve_secret(password, "Password: ", is_tty, || {
                "Password required for basic auth. Use: snow-cli auth login --password <password>"
                    .to_string()
            })?;
            store_basic_login(
                profile_name,
                profile.instance.as_str(),
                profile.username.as_deref(),
                &pw,
                also_now_sdk,
                now_sdk_alias.as_deref(),
                set_now_sdk_default,
            )?;

            let mut result = serde_json::json!({
                "status": "authenticated",
                "profile": profile_name,
                "auth_method": profile.auth_method,
                "credential_type": "password",
            });
            if also_now_sdk {
                let alias_name = now_sdk_alias.unwrap_or_else(|| profile_name.to_string());
                result["now_sdk"] = serde_json::json!({
                    "alias": alias_name,
                    "set_default": set_now_sdk_default,
                });
            }
            println!("{}", serde_json::to_string(&result)?);
        }
        AuthMethod::ApiKey => {
            let tok = resolve_secret(token, "API token: ", is_tty, || {
                "API token required. Use: snow-cli auth login --token <token>".to_string()
            })?;
            credentials::store_credential(profile_name, "api_token", &tok)?;

            let result = serde_json::json!({
                "status": "authenticated",
                "profile": profile_name,
                "auth_method": profile.auth_method,
                "credential_type": "api_token",
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        AuthMethod::Oauth2 => {
            let secret = resolve_secret(client_secret, "Client secret: ", is_tty, || {
                "Client secret required for OAuth2. Use: snow-cli auth login --client-secret <secret>"
                    .to_string()
            })?;
            credentials::store_credential(profile_name, "client_secret", &secret)?;

            let grant_type = profile
                .oauth_grant_type
                .clone()
                .unwrap_or(OAuthGrantType::ClientCredentials);

            if grant_type == OAuthGrantType::AuthorizationCode {
                let (oauth_token, redirect_uri) =
                    run_oauth_authorization_code_login(profile_name, profile, &secret, no_browser)
                        .await?;
                credentials::store_credential(
                    profile_name,
                    "oauth_token",
                    &serde_json::to_string(&oauth_token)?,
                )?;

                let result = serde_json::json!({
                    "status": "authenticated",
                    "profile": profile_name,
                    "auth_method": profile.auth_method,
                    "oauth_grant_type": grant_type,
                    "credential_types": vec!["client_secret", "oauth_token"],
                    "redirect_uri": redirect_uri,
                    "scope": oauth_token.scope.or_else(|| profile.oauth_scope.clone()),
                    "has_refresh_token": oauth_token.refresh_token.is_some(),
                });
                println!("{}", serde_json::to_string(&result)?);
            } else {
                // For password grant, also store the user's password
                if grant_type == OAuthGrantType::Password {
                    let pw = resolve_secret(password, "Password: ", is_tty, || {
                        "Password required for OAuth2 password grant. \
                         Use: snow-cli auth login --client-secret <secret> --password <password>"
                            .to_string()
                    })?;
                    credentials::store_credential(profile_name, "password", &pw)?;
                }

                let result = serde_json::json!({
                    "status": "authenticated",
                    "profile": profile_name,
                    "auth_method": profile.auth_method,
                    "oauth_grant_type": grant_type,
                    "credential_types": if grant_type == OAuthGrantType::Password {
                        vec!["client_secret", "password"]
                    } else {
                        vec!["client_secret"]
                    },
                });
                println!("{}", serde_json::to_string(&result)?);
            }
        }
        AuthMethod::Saml => {
            let cookie =
                resolve_saml_session_cookie(profile_name, profile, session_cookie, is_tty).await?;
            credentials::store_credential(profile_name, "session_cookie", &cookie)?;

            let result = serde_json::json!({
                "status": "authenticated",
                "profile": profile_name,
                "auth_method": profile.auth_method,
                "credential_type": "session_cookie",
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        other => {
            anyhow::bail!("Auth method {:?} does not support `auth login`.", other);
        }
    }

    tracing::info!("Credentials stored for profile '{}'", profile_name);

    Ok(())
}

async fn resolve_saml_session_cookie(
    profile_name: &str,
    profile: &Profile,
    flag_value: Option<String>,
    is_tty: bool,
) -> anyhow::Result<String> {
    if let Some(value) = flag_value {
        return validate_session_cookie(value);
    }

    if !is_tty {
        anyhow::bail!(
            "Session cookie required for SAML auth in non-interactive mode. Use: snow-cli auth login --profile {} --session-cookie 'JSESSIONID=...; glide_user_route=...'",
            profile_name
        );
    }

    let sso_url = profile.sso_login_url.clone().unwrap_or_else(|| {
        format!(
            "{}/login_with_sso.do",
            profile.instance.trim_end_matches('/')
        )
    });

    capture_managed_browser_session_cookie(profile_name, profile, &sso_url).await
}

async fn capture_managed_browser_session_cookie(
    profile_name: &str,
    profile: &Profile,
    sso_url: &str,
) -> anyhow::Result<String> {
    let session_name = saml_browser_session_name(profile_name);
    let instance_host = reqwest::Url::parse(&profile.instance)?
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Profile instance URL is missing a host"))?
        .to_string();

    tracing::info!(url = %sso_url, session = %session_name, "Launching managed browser for SSO login");
    run_agent_browser(&[
        "--session",
        session_name.as_str(),
        "--headed",
        "open",
        sso_url,
    ])
    .await?;

    eprintln!(
        "Complete the login in the opened browser window. Waiting for the ServiceNow session..."
    );

    let deadline = Instant::now() + SAML_LOGIN_TIMEOUT;
    let result = loop {
        if let Ok(cookie_header) = fetch_browser_session_cookie(&session_name, &instance_host).await
        {
            break Ok(cookie_header);
        }

        if Instant::now() >= deadline {
            break Err(anyhow::anyhow!(
                "Timed out waiting for ServiceNow SSO login to complete after {} seconds.",
                SAML_LOGIN_TIMEOUT.as_secs()
            ));
        }

        sleep(SAML_LOGIN_POLL_INTERVAL).await;
    };

    if let Err(error) = close_agent_browser_session(&session_name).await {
        tracing::warn!(error = %error, session = %session_name, "Failed to close managed browser session");
    }

    result
}

async fn fetch_browser_session_cookie(
    session_name: &str,
    instance_host: &str,
) -> anyhow::Result<String> {
    let output =
        run_agent_browser(&["--session", session_name, "cookies", "get", "--json"]).await?;
    let envelope: AgentBrowserCookiesEnvelope = serde_json::from_slice(&output.stdout)?;

    if !envelope.success {
        anyhow::bail!("agent-browser reported an unsuccessful cookie read");
    }

    build_session_cookie_header(&envelope.data.cookies, instance_host)
}

async fn close_agent_browser_session(session_name: &str) -> anyhow::Result<()> {
    run_agent_browser(&["--session", session_name, "close"]).await?;
    Ok(())
}

async fn run_agent_browser(args: &[&str]) -> anyhow::Result<std::process::Output> {
    let output = Command::new("agent-browser")
        .args(args)
        .output()
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to run `agent-browser`: {}. Install it and ensure it is on PATH.",
                error
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("agent-browser {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(output)
}

fn build_session_cookie_header(
    cookies: &[AgentBrowserCookie],
    instance_host: &str,
) -> anyhow::Result<String> {
    let mut matching = cookies
        .iter()
        .filter(|cookie| cookie_domain_matches_host(&cookie.domain, instance_host))
        .map(|cookie| (cookie.name.as_str(), cookie.value.as_str()))
        .collect::<Vec<_>>();

    matching.sort_unstable_by(|a, b| a.0.cmp(b.0));
    matching.dedup_by(|a, b| a.0 == b.0);

    let header = matching
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");

    validate_session_cookie(header)
}

fn cookie_domain_matches_host(cookie_domain: &str, instance_host: &str) -> bool {
    let domain = cookie_domain.trim().trim_start_matches('.');
    !domain.is_empty()
        && (instance_host.eq_ignore_ascii_case(domain)
            || instance_host
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", domain.to_ascii_lowercase())))
}

fn saml_browser_session_name(profile_name: &str) -> String {
    let suffix = profile_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("snow-cli-saml-{suffix}")
}

fn validate_session_cookie(value: String) -> anyhow::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Empty credential provided. Aborting.");
    }

    if !trimmed.contains("JSESSIONID=") {
        anyhow::bail!(
            "The ServiceNow session cookie must include JSESSIONID=. Provide the full Cookie header value, for example 'JSESSIONID=...; glide_user_route=...'."
        );
    }

    Ok(trimmed.to_string())
}

async fn run_oauth_authorization_code_login(
    profile_name: &str,
    profile: &Profile,
    client_secret: &str,
    no_browser: bool,
) -> anyhow::Result<(crate::auth::oauth2::StoredOAuthToken, String)> {
    let bind_host = oauth_redirect_host(profile);
    let port = oauth_redirect_port(profile);
    let redirect_path = oauth_redirect_path(profile);
    let listener = TcpListener::bind((bind_host, port)).await.map_err(|error| {
        anyhow::anyhow!(
            "Failed to bind OAuth redirect listener on {}:{}: {}. Configure a different port with `snow-cli config set-profile {} --oauth-redirect-port <port>` and ensure the ServiceNow OAuth app redirect URL matches.",
            bind_host,
            port,
            error,
            profile_name,
        )
    })?;
    let actual_port = listener.local_addr()?.port();
    let redirect_uri = format!("http://{bind_host}:{actual_port}{redirect_path}");
    let state = generate_oauth_state();
    let code_verifier = generate_pkce_code_verifier();
    let code_challenge = pkce_code_challenge_s256(&code_verifier);
    let auth_url = authorization_url(profile, &redirect_uri, &state, &code_challenge)?;

    eprintln!("OAuth authorization URL:\n{auth_url}\n");
    eprintln!("Waiting for ServiceNow OAuth redirect on {redirect_uri} ...");

    if !no_browser && let Err(error) = open::that(&auth_url) {
        eprintln!(
            "Could not open the authorization URL automatically: {error}. Open the URL above in a browser."
        );
    }

    let code = wait_for_oauth_redirect(listener, &redirect_path, &state).await?;
    let token =
        exchange_authorization_code(profile, &code, &redirect_uri, client_secret, &code_verifier)
            .await?;
    Ok((token, redirect_uri))
}

async fn wait_for_oauth_redirect(
    listener: TcpListener,
    expected_path: &str,
    expected_state: &str,
) -> anyhow::Result<String> {
    let (mut stream, _) = timeout(OAUTH_LOGIN_TIMEOUT, listener.accept())
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Timed out waiting for ServiceNow OAuth redirect after {} seconds.",
                OAUTH_LOGIN_TIMEOUT.as_secs()
            )
        })??;

    let mut buffer = vec![0_u8; 8192];
    let bytes_read = stream.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let first_line = request.lines().next().unwrap_or_default();

    let result = parse_oauth_callback_request(first_line, expected_path, expected_state);
    match &result {
        Ok(_) => {
            write_oauth_http_response(
                &mut stream,
                200,
                "OAuth login complete. You can close this browser window and return to snow-cli.",
            )
            .await?;
        }
        Err(error) => {
            write_oauth_http_response(&mut stream, 400, &format!("OAuth login failed: {error}"))
                .await?;
        }
    }

    result
}

async fn write_oauth_http_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    message: &str,
) -> anyhow::Result<()> {
    let status_text = if status == 200 { "OK" } else { "Bad Request" };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>snow-cli OAuth</title></head><body><h1>{}</h1><p>{}</p></body></html>",
        if status == 200 {
            "Login complete"
        } else {
            "Login failed"
        },
        html_escape(message),
    );
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

fn parse_oauth_callback_request(
    request_line: &str,
    expected_path: &str,
    expected_state: &str,
) -> anyhow::Result<String> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();

    if method != "GET" || target.is_empty() {
        anyhow::bail!("Expected an HTTP GET OAuth callback request.");
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != expected_path {
        anyhow::bail!("Unexpected OAuth callback path '{path}', expected '{expected_path}'.");
    }

    let params = parse_query_params(query)?;
    if let Some(error) = params.get("error") {
        let description = params
            .get("error_description")
            .map(String::as_str)
            .unwrap_or("");
        anyhow::bail!("ServiceNow returned OAuth error '{error}': {description}");
    }

    let state = params
        .get("state")
        .ok_or_else(|| anyhow::anyhow!("OAuth callback did not include a state parameter."))?;
    if state != expected_state {
        anyhow::bail!("OAuth callback state did not match the login request.");
    }

    params
        .get("code")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("OAuth callback did not include an authorization code."))
}

fn parse_query_params(query: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut params = HashMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        params.insert(percent_decode(key)?, percent_decode(value)?);
    }
    Ok(params)
}

fn percent_decode(input: &str) -> anyhow::Result<String> {
    let mut output = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    anyhow::bail!("Invalid percent-encoded value in OAuth callback.");
                }
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3])?;
                let value = u8::from_str_radix(hex, 16).map_err(|_| {
                    anyhow::anyhow!("Invalid percent-encoded value in OAuth callback.")
                })?;
                output.push(value);
                index += 3;
            }
            other => {
                output.push(other);
                index += 1;
            }
        }
    }

    Ok(String::from_utf8(output)?)
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn generate_oauth_state() -> String {
    random_hex_nonce()
}

fn generate_pkce_code_verifier() -> String {
    // RFC 7636 requires 43-128 characters from ALPHA / DIGIT / "-" / "." / "_" / "~".
    // A 32-byte random value hex-encoded to 64 characters satisfies this and is accepted by ServiceNow.
    random_hex_nonce()
}

fn random_hex_nonce() -> String {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut nonce = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut nonce, "{byte:02x}");
    }
    nonce
}

fn store_basic_login(
    profile_name: &str,
    instance: &str,
    username: Option<&str>,
    password: &str,
    also_now_sdk: bool,
    now_sdk_alias: Option<&str>,
    set_now_sdk_default: bool,
) -> anyhow::Result<()> {
    let username = username.ok_or_else(|| {
        anyhow::anyhow!(
            "Basic auth profile '{}' is missing a username. Use `snow-cli config set-profile {} --username <user>` first.",
            profile_name,
            profile_name,
        )
    })?;

    let existing_password = credentials::snapshot_stored_credential(profile_name, "password")?;
    let now_sdk_snapshot = if also_now_sdk {
        Some(now_sdk::snapshot_raw_store()?)
    } else {
        None
    };

    let write_result = (|| -> anyhow::Result<()> {
        credentials::store_credential(profile_name, "password", password)?;

        if also_now_sdk {
            let alias_name = now_sdk_alias.unwrap_or(profile_name);
            let mut store = now_sdk::load_store()?;
            now_sdk::upsert_basic_alias(
                &mut store,
                alias_name,
                instance,
                username,
                password,
                set_now_sdk_default,
            );
            now_sdk::save_store(&store)?;
        }

        Ok(())
    })();

    match write_result {
        Ok(()) => Ok(()),
        Err(error) => {
            credentials::restore_stored_credential(
                profile_name,
                "password",
                existing_password.as_deref(),
            )?;
            if let Some(snapshot) = now_sdk_snapshot.as_ref() {
                now_sdk::restore_raw_store(snapshot.as_deref())?;
            }
            Err(error)
        }
    }
}

/// Resolve a secret value: use the provided flag value, prompt interactively
/// if stdin is a TTY, or return an error with a usage hint.
fn resolve_secret<F>(
    flag_value: Option<String>,
    prompt: &str,
    is_tty: bool,
    error_msg: F,
) -> anyhow::Result<String>
where
    F: FnOnce() -> String,
{
    if let Some(value) = flag_value {
        return Ok(value);
    }

    if is_tty {
        let value = rpassword::prompt_password(prompt)?;
        if value.is_empty() {
            anyhow::bail!("Empty credential provided. Aborting.");
        }
        return Ok(value);
    }

    anyhow::bail!("{}", error_msg());
}

/// `auth logout` — Remove stored credentials for the active profile.
///
/// Removes all credential types associated with the profile's auth method.
async fn handle_logout(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("{}", config.profile_not_found_message(profile_name)))?;

    // Delete all credential types for this auth method
    let cred_types = credential_types_for_auth(profile);
    for cred_type in &cred_types {
        credentials::delete_credential(profile_name, cred_type)?;
    }

    tracing::info!("Credentials removed for profile '{}'", profile_name);

    let result = serde_json::json!({
        "status": "logged_out",
        "profile": profile_name,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}

/// `auth status` — Check if credentials are available for the active profile.
async fn handle_status(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("{}", config.profile_not_found_message(profile_name)))?;

    let cred_types = credential_types_for_auth(profile);
    let authenticated = cred_types
        .iter()
        .all(|ct| credentials::has_credential(profile_name, ct));

    let result = serde_json::json!({
        "profile": profile_name,
        "instance": profile.instance,
        "auth_method": profile.auth_method,
        "credential_types": cred_types,
        "authenticated": authenticated,
        "username": profile.username,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}

/// `auth token` — Print the stored credential to stdout for piping.
///
/// This is useful for integrating with other tools:
/// ```bash
/// curl -H "Authorization: Basic $(snow-cli auth token)" https://...
/// ```
async fn handle_token(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("{}", config.profile_not_found_message(profile_name)))?;

    let primary_cred_type = if profile.auth_method == AuthMethod::Oauth2
        && profile.oauth_grant_type.as_ref() == Some(&OAuthGrantType::AuthorizationCode)
    {
        "oauth_token"
    } else {
        credentials::credential_type_for_auth(&profile.auth_method)
    };
    let credential =
        credentials::get_credential(profile_name, primary_cred_type)?.ok_or_else(|| {
            anyhow::anyhow!(
                "No credentials stored for profile '{}'. Run `snow-cli auth login` first.",
                profile_name
            )
        })?;

    // For basic auth, output the base64-encoded "user:pass" token
    match &profile.auth_method {
        crate::config::profile::AuthMethod::Basic => {
            let username = profile.username.as_deref().unwrap_or("");
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{username}:{credential}"));
            print!("{encoded}");
        }
        crate::config::profile::AuthMethod::Oauth2
            if profile.oauth_grant_type.as_ref() == Some(&OAuthGrantType::AuthorizationCode) =>
        {
            let auth = OAuth2Auth::new(profile_name, profile)?;
            let headers = auth.authenticate().await?;
            let authorization = headers
                .get(http::header::AUTHORIZATION)
                .ok_or_else(|| {
                    anyhow::anyhow!("OAuth authenticator did not return an Authorization header.")
                })?
                .to_str()?;
            let access_token = authorization
                .strip_prefix("Bearer ")
                .unwrap_or(authorization);
            print!("{access_token}");
        }
        _ => {
            // For other auth methods, output the raw credential
            print!("{credential}");
        }
    }

    Ok(())
}

/// Return the list of credential types needed for the profile's auth method.
///
/// OAuth2 password grant requires both `client_secret` and `password`.
fn credential_types_for_auth(profile: &crate::config::profile::Profile) -> Vec<&'static str> {
    match &profile.auth_method {
        AuthMethod::Basic => vec!["password"],
        AuthMethod::ApiKey => vec!["api_token"],
        AuthMethod::Oauth2 => {
            let grant_type = profile
                .oauth_grant_type
                .as_ref()
                .cloned()
                .unwrap_or(OAuthGrantType::ClientCredentials);
            match grant_type {
                OAuthGrantType::Password => vec!["client_secret", "password"],
                OAuthGrantType::AuthorizationCode => vec!["client_secret", "oauth_token"],
                OAuthGrantType::ClientCredentials => vec!["client_secret"],
            }
        }
        AuthMethod::Mtls => vec!["cert_passphrase"],
        AuthMethod::Saml => vec!["session_cookie"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_profile(auth_method: AuthMethod, grant_type: Option<OAuthGrantType>) -> Profile {
        Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method,
            username: Some("admin".to_string()),
            client_id: Some("client123".to_string()),
            oauth_grant_type: grant_type,
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        }
    }

    #[test]
    fn test_validate_session_cookie_accepts_jsessionid() {
        let cookie =
            validate_session_cookie("JSESSIONID=session123; glide_user_route=route".to_string())
                .unwrap();
        assert_eq!(cookie, "JSESSIONID=session123; glide_user_route=route");
    }

    #[test]
    fn test_validate_session_cookie_rejects_missing_jsessionid() {
        let err = validate_session_cookie("glide_user_route=route".to_string())
            .unwrap_err()
            .to_string();
        assert!(err.contains("JSESSIONID"));
    }

    #[test]
    fn test_cookie_domain_matches_host_accepts_exact_and_parent_domains() {
        assert!(cookie_domain_matches_host(
            ".dev.service-now.com",
            "dev.service-now.com"
        ));
        assert!(cookie_domain_matches_host(
            ".service-now.com",
            "dev.service-now.com"
        ));
        assert!(!cookie_domain_matches_host(
            ".example.com",
            "dev.service-now.com"
        ));
    }

    #[test]
    fn test_build_session_cookie_header_filters_to_instance_host() {
        let header = build_session_cookie_header(
            &[
                AgentBrowserCookie {
                    name: "glide_user_route".to_string(),
                    value: "route123".to_string(),
                    domain: ".service-now.com".to_string(),
                },
                AgentBrowserCookie {
                    name: "JSESSIONID".to_string(),
                    value: "session456".to_string(),
                    domain: "dev.service-now.com".to_string(),
                },
                AgentBrowserCookie {
                    name: "other".to_string(),
                    value: "skip".to_string(),
                    domain: ".example.com".to_string(),
                },
            ],
            "dev.service-now.com",
        )
        .unwrap();

        assert_eq!(header, "JSESSIONID=session456; glide_user_route=route123");
    }

    #[test]
    fn test_saml_browser_session_name_sanitizes_profile_name() {
        assert_eq!(
            saml_browser_session_name("Prod EU/1"),
            "snow-cli-saml-prod-eu-1"
        );
    }

    #[test]
    fn test_credential_types_basic() {
        let profile = make_profile(AuthMethod::Basic, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["password"]);
    }

    #[test]
    fn test_credential_types_api_key() {
        let profile = make_profile(AuthMethod::ApiKey, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["api_token"]);
    }

    #[test]
    fn test_credential_types_oauth2_client_credentials() {
        let profile = make_profile(AuthMethod::Oauth2, Some(OAuthGrantType::ClientCredentials));
        assert_eq!(credential_types_for_auth(&profile), vec!["client_secret"]);
    }

    #[test]
    fn test_credential_types_oauth2_password() {
        let profile = make_profile(AuthMethod::Oauth2, Some(OAuthGrantType::Password));
        assert_eq!(
            credential_types_for_auth(&profile),
            vec!["client_secret", "password"]
        );
    }

    #[test]
    fn test_credential_types_oauth2_default_grant() {
        // No grant type set — should default to client_credentials
        let profile = make_profile(AuthMethod::Oauth2, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["client_secret"]);
    }

    #[test]
    fn test_credential_types_oauth2_authorization_code() {
        let profile = make_profile(AuthMethod::Oauth2, Some(OAuthGrantType::AuthorizationCode));
        assert_eq!(
            credential_types_for_auth(&profile),
            vec!["client_secret", "oauth_token"]
        );
    }

    #[test]
    fn test_parse_oauth_callback_request_extracts_code() {
        let code = parse_oauth_callback_request(
            "GET /oauth/callback?code=abc%20123&state=state-xyz HTTP/1.1",
            "/oauth/callback",
            "state-xyz",
        )
        .unwrap();
        assert_eq!(code, "abc 123");
    }

    #[test]
    fn test_parse_oauth_callback_request_rejects_state_mismatch() {
        let err = parse_oauth_callback_request(
            "GET /oauth/callback?code=abc&state=wrong HTTP/1.1",
            "/oauth/callback",
            "expected",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("state"));
    }

    #[test]
    fn test_parse_oauth_callback_request_reports_servicenow_error() {
        let err = parse_oauth_callback_request(
            "GET /oauth/callback?error=access_denied&error_description=Denied+by+user&state=state-xyz HTTP/1.1",
            "/oauth/callback",
            "state-xyz",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("access_denied"));
        assert!(err.contains("Denied by user"));
    }

    #[test]
    fn test_generate_oauth_state_returns_hex_nonce() {
        let state = generate_oauth_state();
        assert_eq!(state.len(), 64);
        assert!(state.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_pkce_code_verifier_is_valid_length_and_charset() {
        let verifier = generate_pkce_code_verifier();
        assert_eq!(verifier.len(), 64);
        assert!(verifier.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn test_credential_types_mtls() {
        let profile = make_profile(AuthMethod::Mtls, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["cert_passphrase"]);
    }

    #[test]
    fn test_credential_types_saml() {
        let profile = make_profile(AuthMethod::Saml, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["session_cookie"]);
    }
}
