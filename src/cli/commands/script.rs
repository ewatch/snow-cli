use std::io::IsTerminal;

use crate::cli::args::{OutputFormat, ScriptArgs, ScriptCommands};
use crate::cli::io::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::identifiers::TableName;

const FORM_SCRIPT_ENDPOINT: &str = "/sys.scripts.do";

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

    let response =
        execute_background_script(profile, instance, timeout_secs, script, &options).await?;
    // Internal callers consume script-printed JSON, not the CLI result envelope.
    if endpoint_requires_form_session(&options.endpoint) {
        Ok(extract_background_script_payload(response))
    } else {
        Ok(response)
    }
}

pub async fn handle(
    args: ScriptArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
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

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let requires_form_session = endpoint_requires_form_session(&options.endpoint);
    let scope = if requires_form_session {
        resolve_form_scope(&mut client, &options.scope).await?
    } else {
        options.scope.clone()
    };
    if requires_form_session {
        eprintln!(
            "Using ServiceNow background script endpoint: {} (scope={})",
            options.endpoint, scope
        );
    }

    let response = client
        .execute_background_script(
            &options.endpoint,
            script,
            &scope,
            crate::client::BackgroundScriptOptions {
                rollback: options.rollback,
                sandbox: options.sandbox,
                scriptlet: options.scriptlet,
                quota_managed_transaction: options.quota_managed_transaction,
            },
        )
        .await?;

    let status = response.status();
    let final_url = response.final_url().to_string();
    let response_headers = response.headers().clone();

    tracing::debug!(
        status = status,
        url = %final_url,
        "Received response"
    );

    if !(200..300).contains(&status) {
        let body = response.text().await.unwrap_or_default();
        return Err(crate::client::error::ApiError::from_status(
            status,
            client.base_url(),
            Some(body),
        )
        .into());
    }

    let server_timing = response_headers
        .get("server-timing")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let mut response_body = response.text().await?;
    if requires_form_session {
        response_body = validate_background_script_response(
            &response_body,
            status,
            &final_url,
            server_timing.as_deref(),
        )?;
    }

    tracing::debug!(body_len = response_body.len(), "Script execution response");

    Ok(response_body)
}

/// Convert a human-readable scope name to the sys_id expected by the Script
/// Background form. The global option is the sole form value that is not a sys_id.
async fn resolve_form_scope(
    client: &mut crate::client::SnowClient,
    scope: &str,
) -> anyhow::Result<String> {
    if scope == "global" || is_sys_id(scope) {
        return Ok(scope.to_string());
    }

    let sys_scope = TableName::from_static("sys_scope");
    let pagination = PaginationConfig::default().with_limit(Some(2));
    let records = client
        .get_table_records(
            &sys_scope,
            Some(&format!("scope={scope}")),
            Some("sys_id"),
            &pagination,
            None,
        )
        .await?;

    match records.as_slice() {
        [] => anyhow::bail!(
            "Scope '{scope}' was not found in sys_scope. Pass 'global', a scope name, or a scope sys_id."
        ),
        [record] => record
            .sys_id()
            .map(ToString::to_string)
            .ok_or_else(|| anyhow::anyhow!("Scope '{scope}' did not return a sys_id")),
        _ => anyhow::bail!("Scope '{scope}' matched multiple sys_scope records"),
    }
}

fn is_sys_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn handle_run(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    file: Option<String>,
    code: Option<String>,
    options: &ScriptRunOptions,
) -> anyhow::Result<()> {
    let script = resolve_script(file, code)?;
    let response_body =
        execute_background_script(profile, instance, timeout_secs, &script, options).await?;

    if endpoint_requires_form_session(&options.endpoint) {
        return match format {
            // CSV historically means a raw execution log here, not tabular data.
            OutputFormat::Text | OutputFormat::Csv => {
                println!("{response_body}");
                Ok(())
            }
            _ => output::print_output(
                &parse_script_execution_log(&response_body, &options.scope),
                format,
            ),
        };
    }

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
        OutputFormat::Auto => match serde_json::from_str::<serde_json::Value>(&response_body) {
            Ok(json) => output::emit_auto(&json, &mut std::io::stdout())?,
            Err(_) => println!("{}", response_body),
        },
    }

    Ok(())
}

fn endpoint_requires_form_session(endpoint: &str) -> bool {
    if let Ok(url) = url::Url::parse(endpoint) {
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

/// Recognize evaluator diagnostics, not arbitrary occurrences of "error" in
/// user output. Script print lines and JSON values remain ordinary result data.
fn background_script_failure(cleaned: &str) -> Option<crate::error::ScriptExecutionError> {
    use crate::error::ScriptExecutionError;

    for line in cleaned.lines() {
        let mut line = line.trim();
        // Some instance releases wrap diagnostics in presentation markup.
        // Strip only leading known HTML tags for classification; leave the
        // returned script output untouched.
        while let Some(rest) = line.strip_prefix('<') {
            let Some((tag, remainder)) = rest.split_once('>') else {
                break;
            };
            let name = tag
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(
                name.as_str(),
                "html" | "body" | "div" | "span" | "font" | "p" | "b" | "strong" | "pre"
            ) {
                break;
            }
            line = remainder.trim_start();
        }
        if line.starts_with("*** Script:") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("kittyscript validation failed") {
            return Some(ScriptExecutionError::SandboxValidation);
        }
        if lower.starts_with("script compilation error") {
            return Some(ScriptExecutionError::Compilation);
        }
        if lower.starts_with("evaluator.evaluatestring() problem") {
            return Some(ScriptExecutionError::Evaluation);
        }
        if lower.starts_with("background message,")
            && lower.split(',').skip(1).any(|field| {
                field
                    .chars()
                    .filter(|ch| !ch.is_whitespace())
                    .collect::<String>()
                    == "type:error"
            })
        {
            return Some(ScriptExecutionError::BackgroundMessage);
        }
        let exception = line.strip_prefix("Caused by: ").unwrap_or(line);
        let class = exception.split([':', ' ']).next().unwrap_or_default();
        // Java/Rhino exception headers are strong signals; do not classify
        // standalone "error" or user-printed stack traces as failures.
        if [
            "java.",
            "javax.",
            "com.glide.",
            "com.snc.",
            "org.mozilla.javascript.",
        ]
        .iter()
        .any(|prefix| class.starts_with(prefix))
            && (class.ends_with("Exception") || class.ends_with("Error"))
        {
            return Some(ScriptExecutionError::JavaException);
        }
    }
    None
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

    // Check the complete response before JSON extraction can hide an evaluator
    // failure preceding a valid-looking printed payload.
    if let Some(error) = background_script_failure(&cleaned) {
        return Err(error.into());
    }

    Ok(cleaned)
}

fn extract_background_script_payload(cleaned: String) -> String {
    if serde_json::from_str::<serde_json::Value>(&cleaned).is_ok() {
        return cleaned;
    }
    extract_json_from_script_marker(&cleaned).unwrap_or(cleaned)
}

#[derive(serde::Serialize)]
struct ScriptExecutionResult {
    output: Vec<String>,
    duration_ms: Option<u64>,
    scope: String,
    history_sys_id: Option<String>,
    // Known failures return a typed stderr error before reaching this parser.
    errors: Vec<String>,
}

/// Parse metadata only from server log lines, not script-printed content.
/// Unknown lines remain output so release-specific layouts do not lose data.
fn parse_script_execution_log(cleaned: &str, requested_scope: &str) -> ScriptExecutionResult {
    let mut result = ScriptExecutionResult {
        output: Vec::new(),
        duration_ms: None,
        scope: requested_scope.to_string(),
        history_sys_id: None,
        errors: Vec::new(),
    };
    for line in cleaned.lines() {
        let trimmed = line.trim();
        if let Some(printed) = line.trim_start().strip_prefix("*** Script:") {
            result
                .output
                .push(printed.strip_prefix(' ').unwrap_or(printed).to_string());
            continue;
        }
        if let Some((elapsed, banner)) = trimmed
            .strip_prefix('[')
            .and_then(|line| line.split_once(']'))
            && let Some(scope) = banner.trim().strip_prefix("Script completed in scope ")
            && let Some((scope, _)) = scope.split_once(':')
            && !scope.trim().is_empty()
        {
            result.scope = scope.trim().to_string();
            result.duration_ms = parse_script_duration(elapsed);
            continue;
        }
        if trimmed.starts_with("Script execution history ")
            && let Some(id) = trimmed
                .split_once("sys_script_execution_history.do?sys_id=")
                .map(|(_, tail)| tail)
        {
            let id: String = id.chars().take_while(|ch| ch.is_ascii_hexdigit()).collect();
            if is_sys_id(&id) {
                result.history_sys_id = Some(id);
                continue;
            }
        }
        if !trimmed.is_empty() {
            result.output.push(line.to_string());
        }
    }
    result
}

/// ServiceNow timing banners use [hours:minutes:seconds.milliseconds].
/// Reject malformed/overflowing values rather than fabricating metadata.
fn parse_script_duration(elapsed: &str) -> Option<u64> {
    let mut parts = elapsed.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let (seconds, millis) = parts.next()?.split_once('.')?;
    if parts.next().is_some() || millis.len() != 3 || !millis.bytes().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    let seconds: u64 = seconds.parse().ok()?;
    let millis: u64 = millis.parse().ok()?;
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes * 60_000)?
        .checked_add(seconds * 1_000)?
        .checked_add(millis)
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
    reader: R,
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

    let buf = read_to_string_limited(reader, DEFAULT_MAX_STDIN_BYTES, "script stdin input")?;

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
    fn script_log_metadata_is_optional_and_printed_content_is_preserved() {
        let parsed = parse_script_execution_log(
            "*** Script: [0:00:01.000] Script completed in scope fake: script\n*** Script:   padded  \n*** Script:\nunknown release-specific line",
            "x_requested",
        );
        assert_eq!(parsed.duration_ms, None);
        assert_eq!(parsed.history_sys_id, None);
        assert_eq!(parsed.scope, "x_requested");
        assert_eq!(
            parsed.output,
            [
                "[0:00:01.000] Script completed in scope fake: script",
                "  padded  ",
                "",
                "unknown release-specific line"
            ]
        );
        assert!(parsed.errors.is_empty());
    }

    #[test]
    fn script_duration_is_checked_and_uses_milliseconds() {
        assert_eq!(parse_script_duration("0:00:00.006"), Some(6));
        assert_eq!(parse_script_duration("1:02:03.456"), Some(3_723_456));
        for invalid in [
            "",
            "0:60:00.000",
            "0:00:60.000",
            "0:00:00.6",
            "0:00:00.000:1",
            "18446744073709551615:00:00.000",
            "0:00:00.xxx",
        ] {
            assert_eq!(parse_script_duration(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn invalid_history_is_retained_and_unknown_duration_is_null() {
        let log = "Script execution history <A HREF='sys_script_execution_history.do?sys_id=bad'>here</A>\n[broken] Script completed in scope global: script";
        let parsed = parse_script_execution_log(log, "x_requested");
        assert_eq!(parsed.history_sys_id, None);
        assert_eq!(parsed.duration_ms, None);
        assert_eq!(parsed.scope, "global");
        assert_eq!(parsed.output.len(), 1);
        assert!(parsed.output[0].contains("sys_id=bad"));
    }

    #[test]
    fn test_is_sys_id() {
        assert!(is_sys_id("842da4135c5748b288874edfa209f7de"));
        assert!(!is_sys_id("x_2135095_au_demo"));
        assert!(!is_sys_id("not-a-sys-id"));
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
            extract_background_script_payload(clean),
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
            extract_background_script_payload(clean),
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
