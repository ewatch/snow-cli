use std::io::IsTerminal;

use crate::cli::args::{OutputFormat, ScriptArgs, ScriptCommands};
use http::HeaderMap;

const FORM_SCRIPT_ENDPOINT: &str = "/sys.scripts.do";
const FORM_SCRIPT_BOOTSTRAP_PATH: &str = "/sys.scripts.modern.do";

pub async fn handle(
    args: ScriptArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        ScriptCommands::Run {
            file,
            code,
            scope,
            endpoint,
        } => handle_run(profile, format, instance, file, code, &scope, &endpoint).await,
    }
}

/// `script run` — Execute a background script on the ServiceNow instance.
///
/// Sends the script body to the ServiceNow background script execution endpoint.
/// The script can be provided inline via `--code` or read from a file via `--file`.
/// If neither flag is provided, reads from stdin.
///
async fn handle_run(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    file: Option<String>,
    code: Option<String>,
    scope: &str,
    endpoint: &str,
) -> anyhow::Result<()> {
    let script = resolve_script(file, code)?;
    let script_len = script.len();

    tracing::info!(
        endpoint = endpoint,
        scope = scope,
        script_len = script_len,
        "Executing background script"
    );

    let mut client = crate::client::build_client(profile, instance)?;
    let requires_form_session = endpoint_requires_form_session(endpoint);
    let form_session = if requires_form_session {
        Some(
            client
                .ensure_form_session(FORM_SCRIPT_BOOTSTRAP_PATH)
                .await?,
        )
    } else {
        None
    };

    let url = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("{}{}", client.base_url(), endpoint)
    };

    let response = if requires_form_session {
        let form_session = form_session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing form session for script execution."))?;
        let cookie_header = form_session.cookie_header.clone();
        let request_headers = sanitized_request_headers(
            &HeaderMap::new(),
            &[
                ("Accept", "text/html,application/xhtml+xml"),
                ("Cookie", cookie_header.as_str()),
                ("X-UserToken", form_session.g_ck.as_str()),
            ],
        );

        eprintln!(
            "Using ServiceNow background script endpoint: {} (scope={})",
            endpoint, scope
        );

        tracing::debug!(
            method = "POST",
            url = %url,
            endpoint = endpoint,
            headers = ?request_headers,
            script_len = script_len,
            body_encoding = "application/x-www-form-urlencoded",
            form_fields = ?[
                "script",
                "runscript",
                "sysparm_ck",
                "sys_scope",
                "record_for_rollback",
                "quota_managed_transaction"
            ],
            "Sending request"
        );

        let request = client
            .http()
            .post(&url)
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Cookie", cookie_header)
            .header("X-UserToken", form_session.g_ck.as_str())
            .form(&[
                ("script", script.as_str()),
                ("runscript", "Run script"),
                ("sysparm_ck", form_session.g_ck.as_str()),
                ("sys_scope", scope),
                ("record_for_rollback", "on"),
                ("quota_managed_transaction", "on"),
            ])
            .build()?;

        crate::client::log_raw_http_request(&request);

        client.http().execute(request).await?
    } else {
        let auth_headers = client.authenticator().authenticate().await?;
        let request_headers = sanitized_request_headers(
            &auth_headers,
            &[
                ("Accept", "application/json"),
                ("Content-Type", "application/json"),
            ],
        );

        tracing::debug!(
            method = "POST",
            url = %url,
            endpoint = endpoint,
            headers = ?request_headers,
            script_len = script_len,
            body_encoding = "application/json",
            body_keys = ?["script", "scope"],
            "Sending request"
        );

        let body = serde_json::json!({
            "script": script,
            "scope": scope,
        });

        let request = client
            .http()
            .post(&url)
            .headers(auth_headers.clone())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&body)?)
            .build()?;

        crate::client::log_raw_http_request(&request);

        client.http().execute(request).await?
    };

    let status = response.status();
    let final_url = response.url().to_string();
    let response_headers = response.headers().clone();

    crate::client::log_raw_http_response(&final_url, status, response.headers());

    tracing::debug!(
        status = status.as_u16(),
        url = %final_url,
        "Received response"
    );

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Script execution failed with status {}: {}",
            status.as_u16(),
            body
        );
    }

    let server_timing = response_headers
        .get("server-timing")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let mut response_body = response.text().await?;
    if requires_form_session {
        response_body = validate_background_script_response(
            &response_body,
            status.as_u16(),
            &final_url,
            server_timing.as_deref(),
        )?;
    }

    tracing::debug!(body_len = response_body.len(), "Script execution response");

    match format {
        OutputFormat::Json => match serde_json::from_str::<serde_json::Value>(&response_body) {
            Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
            Err(_) => println!("{}", response_body),
        },
        OutputFormat::Csv => {
            println!("{}", response_body);
        }
    }

    Ok(())
}

fn endpoint_requires_form_session(endpoint: &str) -> bool {
    if let Ok(url) = reqwest::Url::parse(endpoint) {
        return url
            .path()
            .trim_end_matches('/')
            .ends_with(FORM_SCRIPT_ENDPOINT);
    }

    endpoint
        .split(['?', '#'])
        .next()
        .unwrap_or(endpoint)
        .trim_end_matches('/')
        .ends_with(FORM_SCRIPT_ENDPOINT)
}

fn sanitized_request_headers(
    auth_headers: &HeaderMap,
    extra_headers: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut headers = auth_headers
        .iter()
        .map(|(name, value)| {
            let header_name = name.as_str().to_string();
            let value = value
                .to_str()
                .map(str::to_string)
                .unwrap_or_else(|_| "<non-utf8>".to_string());
            let sanitized = sanitize_header_value(name.as_str(), &value);
            (header_name, sanitized)
        })
        .collect::<Vec<_>>();

    for (name, value) in extra_headers {
        headers.push((name.to_string(), sanitize_header_value(name, value)));
    }

    headers
}

fn sanitize_header_value(name: &str, value: &str) -> String {
    if is_sensitive_header(name) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

fn is_sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization" | "cookie" | "set-cookie" | "x-usertoken" | "x-user-token"
    ) || name.contains("token")
        || name.contains("secret")
        || name.contains("key")
        || name.contains("auth")
}

fn sanitize_background_script_response(raw: &str) -> String {
    raw.replace("<HTML><BODY>", "")
        .replace("</BODY><HTML>", "")
        .replace("</BODY></HTML>", "")
        .trim()
        .to_string()
}

fn validate_background_script_response(
    raw: &str,
    status_code: u16,
    final_url: &str,
    server_timing: Option<&str>,
) -> anyhow::Result<String> {
    let normalized = raw.to_ascii_lowercase();
    if normalized.contains("user not authenticated")
        || normalized.contains("security constraints prevent access")
        || normalized.contains("id=\"sysverb_login\"")
        || normalized.contains("name=\"login\"")
        || final_url.contains("/login.do")
    {
        anyhow::bail!(
            "Background script request was not authorized (status {}). Final URL: {}. \
             Verify the profile user has rights to execute background scripts.",
            status_code,
            final_url
        );
    }

    let cleaned = sanitize_background_script_response(raw);
    if cleaned.is_empty() {
        let no_script_execution = server_timing
            .map(|header| header.to_ascii_lowercase().contains("scripting;dur=0"))
            .unwrap_or(false);

        if no_script_execution {
            anyhow::bail!(
                "Background script returned an empty response (status {}, URL {}). \
                 ServiceNow reported scripting;dur=0, which usually means script execution \
                 was not triggered. Ensure the request includes runscript and sys_scope \
                 form fields, plus matching sysparm_ck and X-UserToken values.",
                status_code,
                final_url
            );
        }

        anyhow::bail!(
            "Background script returned an empty response (status {}, URL {}). \
             This can happen when no output is produced. Use gs.print('...') to emit output, \
             and ensure the account can execute /sys.scripts.do.",
            status_code,
            final_url
        );
    }

    Ok(cleaned)
}

/// Resolve script content from `--file`, `--code`, or stdin.
fn resolve_script(file: Option<String>, code: Option<String>) -> anyhow::Result<String> {
    resolve_script_from(
        file,
        code,
        std::io::stdin().lock(),
        std::io::stdin().is_terminal(),
    )
}

/// Internal implementation for testability.
fn resolve_script_from<R: std::io::Read>(
    file: Option<String>,
    code: Option<String>,
    mut reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    // --code takes precedence
    if let Some(c) = code {
        if c.trim().is_empty() {
            anyhow::bail!("Empty script provided via --code.");
        }
        return Ok(c);
    }

    // --file reads from filesystem
    if let Some(path) = file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read script file '{}': {}", path, e))?;
        if content.trim().is_empty() {
            anyhow::bail!("Script file '{}' is empty.", path);
        }
        return Ok(content);
    }

    // Fall back to stdin
    if is_tty {
        anyhow::bail!(
            "No script provided. Use --code '<script>', --file <path>, or pipe script to stdin."
        );
    }

    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;

    if buf.trim().is_empty() {
        anyhow::bail!("No script received from stdin.");
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_sanitize_background_script_response_html_wrappers() {
        let raw = "<HTML><BODY>Script output</BODY></HTML>";
        let clean = sanitize_background_script_response(raw);
        assert_eq!(clean, "Script output");
    }

    #[test]
    fn test_sanitize_background_script_response_legacy_closing_tag() {
        let raw = "<HTML><BODY>Script output</BODY><HTML>";
        let clean = sanitize_background_script_response(raw);
        assert_eq!(clean, "Script output");
    }

    #[test]
    fn test_validate_background_script_response_rejects_empty() {
        let err = validate_background_script_response(
            "",
            200,
            "https://dev.service-now.com/sys.scripts.do",
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("empty response"));
    }

    #[test]
    fn test_validate_background_script_response_rejects_empty_with_execution_hint() {
        let err = validate_background_script_response(
            "",
            200,
            "https://dev.service-now.com/sys.scripts.do",
            Some("wall;dur=12, scripting;dur=0"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("runscript"));
        assert!(err.to_string().contains("sys_scope"));
    }

    #[test]
    fn test_validate_background_script_response_rejects_login_page() {
        let err = validate_background_script_response(
            "<html><body>User Not Authenticated</body></html>",
            200,
            "https://dev.service-now.com/login.do",
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not authorized"));
    }

    #[test]
    fn test_endpoint_requires_form_session_for_background_script_endpoint() {
        assert!(endpoint_requires_form_session("/sys.scripts.do"));
        assert!(endpoint_requires_form_session("/sys.scripts.do?foo=bar"));
        assert!(endpoint_requires_form_session(
            "https://dev.service-now.com/sys.scripts.do?foo=bar"
        ));
        assert!(!endpoint_requires_form_session("/api/x_myapp/script/run"));
    }

    #[test]
    fn test_sanitize_header_value_redacts_sensitive_headers() {
        assert_eq!(
            sanitize_header_value("Authorization", "Bearer secret"),
            "[REDACTED]"
        );
        assert_eq!(
            sanitize_header_value("x-usertoken", "token-123"),
            "[REDACTED]"
        );
        assert_eq!(
            sanitize_header_value("Cookie", "JSESSIONID=abc"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_sanitize_header_value_preserves_non_sensitive_headers() {
        assert_eq!(
            sanitize_header_value("Accept", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn test_sanitized_request_headers_redacts_auth_and_extra_headers() {
        let mut auth_headers = HeaderMap::new();
        auth_headers.insert(
            "Authorization",
            http::HeaderValue::from_static("Bearer secret"),
        );
        auth_headers.insert(
            "X-Correlation-Id",
            http::HeaderValue::from_static("abc-123"),
        );

        let sanitized = sanitized_request_headers(
            &auth_headers,
            &[("Accept", "application/json"), ("X-UserToken", "gck-123")],
        );

        assert!(sanitized.iter().any(
            |(name, value)| name.eq_ignore_ascii_case("authorization") && value == "[REDACTED]"
        ));
        assert!(sanitized.iter().any(
            |(name, value)| name.eq_ignore_ascii_case("x-correlation-id") && value == "abc-123"
        ));
        assert!(
            sanitized
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("x-usertoken")
                    && value == "[REDACTED]")
        );
    }

    #[test]
    fn test_resolve_script_from_code() {
        let script = resolve_script_from(
            None,
            Some("gs.info('hello')".to_string()),
            Cursor::new(b""),
            true,
        )
        .unwrap();
        assert_eq!(script, "gs.info('hello')");
    }

    #[test]
    fn test_resolve_script_from_code_empty_errors() {
        let result = resolve_script_from(None, Some("   ".to_string()), Cursor::new(b""), true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty script"));
    }

    #[test]
    fn test_resolve_script_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.js");
        std::fs::write(&script_path, "gs.info('from file')").unwrap();

        let script = resolve_script_from(
            Some(script_path.to_string_lossy().to_string()),
            None,
            Cursor::new(b""),
            true,
        )
        .unwrap();
        assert_eq!(script, "gs.info('from file')");
    }

    #[test]
    fn test_resolve_script_from_file_not_found() {
        let result = resolve_script_from(
            Some("/nonexistent/script.js".to_string()),
            None,
            Cursor::new(b""),
            true,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }

    #[test]
    fn test_resolve_script_from_file_empty() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("empty.js");
        std::fs::write(&script_path, "   ").unwrap();

        let result = resolve_script_from(
            Some(script_path.to_string_lossy().to_string()),
            None,
            Cursor::new(b""),
            true,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("is empty"));
    }

    #[test]
    fn test_resolve_script_from_stdin() {
        let script =
            resolve_script_from(None, None, Cursor::new(b"gs.info('from stdin')"), false).unwrap();
        assert_eq!(script, "gs.info('from stdin')");
    }

    #[test]
    fn test_resolve_script_from_tty_no_input_errors() {
        let result = resolve_script_from(None, None, Cursor::new(b""), true);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No script provided")
        );
    }

    #[test]
    fn test_resolve_script_from_stdin_empty_errors() {
        let result = resolve_script_from(None, None, Cursor::new(b""), false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No script received")
        );
    }

    #[test]
    fn test_resolve_script_code_takes_precedence_over_file() {
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test.js");
        std::fs::write(&script_path, "file content").unwrap();

        // clap's `group` would prevent this, but our function prioritizes --code
        let script = resolve_script_from(
            Some(script_path.to_string_lossy().to_string()),
            Some("code content".to_string()),
            Cursor::new(b""),
            true,
        )
        .unwrap();
        assert_eq!(script, "code content");
    }
}
