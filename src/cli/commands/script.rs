use std::io::IsTerminal;

use crate::cli::args::{OutputFormat, ScriptArgs, ScriptCommands};
use crate::cli::output;
use http::HeaderMap;

const FORM_SCRIPT_ENDPOINT: &str = "/sys.scripts.do";
const FORM_SCRIPT_BOOTSTRAP_PATH: &str = "/sys.scripts.modern.do";

struct ScriptRunOptions {
    scope: String,
    endpoint: String,
    rollback: bool,
    sandbox: bool,
    scriptlet: bool,
    quota_managed_transaction: bool,
}

pub async fn run_background_script(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    proxy_url: Option<&str>,
    script: &str,
    scope: &str,
    endpoint: Option<&str>,
) -> anyhow::Result<String> {
    let options = ScriptRunOptions {
        scope: scope.to_string(),
        endpoint: endpoint.unwrap_or(FORM_SCRIPT_ENDPOINT).to_string(),
        rollback: false,
        sandbox: false,
        scriptlet: false,
        quota_managed_transaction: false,
    };

    execute_background_script(profile, instance, timeout_secs, proxy_url, script, &options).await
}

pub async fn handle(
    args: ScriptArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    proxy_url: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        ScriptCommands::Run {
            file,
            code,
            scope,
            endpoint,
            rollback,
            sandbox,
            scriptlet,
            quota_managed_transaction,
        } => {
            let options = ScriptRunOptions {
                scope,
                endpoint,
                rollback,
                sandbox,
                scriptlet,
                quota_managed_transaction,
            };
            handle_run(
                profile,
                format,
                instance,
                timeout_secs,
                proxy_url,
                file,
                code,
                &options,
            )
            .await
        }
    }
}

/// `script run` — Execute a background script on the ServiceNow instance.
///
/// Sends the script body to the ServiceNow background script execution endpoint.
/// The script can be provided inline via `--code` or read from a file via `--file`.
/// If neither flag is provided, reads from stdin.
///
async fn execute_background_script(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    proxy_url: Option<&str>,
    script: &str,
    options: &ScriptRunOptions,
) -> anyhow::Result<String> {
    let script_len = script.len();

    tracing::info!(
        endpoint = options.endpoint,
        scope = options.scope,
        script_len = script_len,
        "Executing background script"
    );

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs, proxy_url)?;
    let requires_form_session = endpoint_requires_form_session(&options.endpoint);
    let form_session = if requires_form_session {
        Some(
            client
                .ensure_form_session(FORM_SCRIPT_BOOTSTRAP_PATH)
                .await?,
        )
    } else {
        None
    };

    let url = if options.endpoint.starts_with("http://") || options.endpoint.starts_with("https://")
    {
        options.endpoint.to_string()
    } else {
        format!("{}{}", client.base_url(), options.endpoint)
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
            options.endpoint, options.scope
        );

        let mut form_field_names = vec!["script", "runscript", "sysparm_ck", "sys_scope"];
        if options.rollback {
            form_field_names.push("record_for_rollback");
        }
        if options.sandbox {
            form_field_names.push("sandbox");
        }
        if options.scriptlet {
            form_field_names.push("scriptlet");
        }
        if options.quota_managed_transaction {
            form_field_names.push("quota_managed_transaction");
        }

        tracing::debug!(
            method = "POST",
            url = %url,
            endpoint = options.endpoint,
            headers = ?request_headers,
            script_len = script_len,
            body_encoding = "application/x-www-form-urlencoded",
            form_fields = ?form_field_names,
            "Sending request"
        );

        let mut form_fields = vec![
            ("script", script),
            ("runscript", "Run script"),
            ("sysparm_ck", form_session.g_ck.as_str()),
            ("sys_scope", options.scope.as_str()),
        ];
        if options.rollback {
            form_fields.push(("record_for_rollback", "on"));
        }
        if options.sandbox {
            form_fields.push(("sandbox", "on"));
        }
        if options.scriptlet {
            form_fields.push(("scriptlet", "on"));
        }
        if options.quota_managed_transaction {
            form_fields.push(("quota_managed_transaction", "on"));
        }

        let request = client
            .http()
            .post(&url)
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Cookie", cookie_header)
            .header("X-UserToken", form_session.g_ck.as_str())
            .form(&form_fields)
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
            endpoint = options.endpoint,
            headers = ?request_headers,
            script_len = script_len,
            body_encoding = "application/json",
            body_keys = ?["script", "scope"],
            "Sending request"
        );

        let body = serde_json::json!({
            "script": script,
            "scope": options.scope,
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

    Ok(response_body)
}

async fn handle_run(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    proxy_url: Option<&str>,
    file: Option<String>,
    code: Option<String>,
    options: &ScriptRunOptions,
) -> anyhow::Result<()> {
    let script = resolve_script(file, code)?;
    let response_body =
        execute_background_script(profile, instance, timeout_secs, proxy_url, &script, options).await?;

    match format {
        OutputFormat::Json => match serde_json::from_str::<serde_json::Value>(&response_body) {
            Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
            Err(_) => println!("{}", response_body),
        },
        OutputFormat::Csv => {
            println!("{}", response_body);
        }
        OutputFormat::Jsonl => match serde_json::from_str::<serde_json::Value>(&response_body) {
            Ok(json) => output::write_jsonl_value(&json, &mut std::io::stdout())?,
            Err(_) => println!("{}", response_body),
        },
        OutputFormat::Toon => match serde_json::from_str::<serde_json::Value>(&response_body) {
            Ok(json) => output::write_toon(&json, &mut std::io::stdout())?,
            Err(_) => println!("{}", response_body),
        },
        OutputFormat::Text => match serde_json::from_str::<serde_json::Value>(&response_body) {
            Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
            Err(_) => println!("{}", response_body),
        },
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

fn decode_basic_html_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
}

fn sanitize_background_script_response(raw: &str) -> String {
    let with_breaks = raw
        .replace("<HTML><BODY>", "")
        .replace("</BODY><HTML>", "")
        .replace("</BODY></HTML>", "")
        .replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("<BR />", "\n")
        .replace("<BR/>", "\n")
        .replace("<BR>", "\n")
        .replace("<hr />", "\n")
        .replace("<hr/>", "\n")
        .replace("<hr>", "\n")
        .replace("<HR />", "\n")
        .replace("<HR/>", "\n")
        .replace("<HR>", "\n")
        .replace("<pre>", "\n")
        .replace("</pre>", "\n")
        .replace("<PRE>", "\n")
        .replace("</PRE>", "\n");

    decode_basic_html_entities(&with_breaks).trim().to_string()
}

fn extract_balanced_json_snippet(text: &str) -> Option<&str> {
    let start = text
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(index, _)| index)?;

    let opening = text[start..].chars().next()?;
    let closing = match opening {
        '{' => '}',
        '[' => ']',
        _ => return None,
    };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            c if c == opening => depth += 1,
            c if c == closing => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + offset + ch.len_utf8();
                    return Some(&text[start..end]);
                }
            }
            _ => {}
        }
    }

    None
}

fn extract_json_from_script_marker(cleaned: &str) -> Option<String> {
    const SCRIPT_MARKER: &str = "*** Script:";

    let marker_index = cleaned.rfind(SCRIPT_MARKER)?;
    let after_marker = cleaned[marker_index + SCRIPT_MARKER.len()..].trim_start();
    let json_snippet = extract_balanced_json_snippet(after_marker)?;

    serde_json::from_str::<serde_json::Value>(json_snippet)
        .ok()
        .map(|_| json_snippet.to_string())
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

    if serde_json::from_str::<serde_json::Value>(&cleaned).is_ok() {
        return Ok(cleaned);
    }

    if let Some(json_payload) = extract_json_from_script_marker(&cleaned) {
        return Ok(json_payload);
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
    fn test_validate_background_script_response_extracts_json_from_script_marker() {
        let raw = r#"<HTML><BODY>[0:00:00.013] Script completed in scope global: script
*** Script: {"ok":true,"dry_run":true,"warnings":[],"requires_confirmation":false}
</BODY></HTML>"#;

        let clean = validate_background_script_response(
            raw,
            200,
            "https://dev.service-now.com/sys.scripts.do",
            None,
        )
        .unwrap();

        assert_eq!(
            clean,
            r#"{"ok":true,"dry_run":true,"warnings":[],"requires_confirmation":false}"#
        );
    }

    #[test]
    fn test_validate_background_script_response_extracts_json_from_html_encoded_wrapper() {
        let raw = r#"<HTML><BODY>[0:00:00.018] Script completed in scope global: script<HR/><PRE>*** Script: {&quot;ok&quot;:true,&quot;dry_run&quot;:true,&quot;warnings&quot;:[],&quot;requires_confirmation&quot;:false}<BR/></PRE><HR/></BODY></HTML>"#;

        let clean = validate_background_script_response(
            raw,
            200,
            "https://dev.service-now.com/sys.scripts.do",
            None,
        )
        .unwrap();

        assert_eq!(
            clean,
            r#"{"ok":true,"dry_run":true,"warnings":[],"requires_confirmation":false}"#
        );
    }

    #[test]
    fn test_validate_background_script_response_preserves_plain_text_when_no_json_marker_exists() {
        let raw = r#"<HTML><BODY>[0:00:00.013] Script completed in scope global: script
*** Script: moved record preview
</BODY></HTML>"#;

        let clean = validate_background_script_response(
            raw,
            200,
            "https://dev.service-now.com/sys.scripts.do",
            None,
        )
        .unwrap();

        assert!(clean.contains("*** Script: moved record preview"));
    }

    #[test]
    fn test_sanitize_background_script_response_preserves_xml_like_output() {
        let raw = "<HTML><BODY>&lt;foo&gt;bar&lt;/foo&gt;</BODY></HTML>";
        let clean = sanitize_background_script_response(raw);
        assert_eq!(clean, "<foo>bar</foo>");
    }

    #[test]
    fn test_sanitize_background_script_response_preserves_angle_bracket_text() {
        let raw = "<HTML><BODY>a &lt; b</BODY></HTML>";
        let clean = sanitize_background_script_response(raw);
        assert_eq!(clean, "a < b");
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
