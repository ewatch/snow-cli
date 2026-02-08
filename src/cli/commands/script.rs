use std::io::IsTerminal;

use crate::cli::args::{OutputFormat, ScriptArgs, ScriptCommands};

pub async fn handle(
    args: ScriptArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        ScriptCommands::Run { file, code, scope } => {
            handle_run(profile, format, instance, file, code, &scope).await
        }
    }
}

/// `script run` — Execute a background script on the ServiceNow instance.
///
/// Sends the script body to the ServiceNow background script execution endpoint.
/// The script can be provided inline via `--code` or read from a file via `--file`.
/// If neither flag is provided, reads from stdin.
///
/// Uses the `/api/now/sp/widget/widget-simple-list` or a custom Scripted REST
/// endpoint. The most common approach is to POST to a Scripted REST API or
/// use the `sys_script_execution` table API. This implementation targets a
/// Scripted REST endpoint that should be deployed on the target instance.
///
/// Endpoint: POST /api/now/script/run (or configurable)
///
/// Request body:
/// ```json
/// {
///   "script": "gs.info('hello')",
///   "scope": "global"
/// }
/// ```
async fn handle_run(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    file: Option<String>,
    code: Option<String>,
    scope: &str,
) -> anyhow::Result<()> {
    let script = resolve_script(file, code)?;

    tracing::info!(
        scope = scope,
        script_len = script.len(),
        "Executing background script"
    );

    let body = serde_json::json!({
        "script": script,
        "scope": scope,
    });

    let mut client = crate::client::build_client(profile, instance)?;

    let response = client
        .post("/api/now/script/run", &serde_json::to_string(&body)?)
        .await?;

    let response_body = response.text().await?;

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
