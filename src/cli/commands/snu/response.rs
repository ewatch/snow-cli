use super::*;

pub(super) fn print_response_value(
    response: SnuMessage,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let mut value = serde_json::to_value(&response)?;
    if let Value::Object(map) = &mut value {
        map.remove("agentRequestId");
    }
    print_output(&value, output_format)
}

pub(super) fn print_background_script_response(
    response: SnuMessage,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let data = response
        .extra
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SN-Utils background script response did not contain data"))?;

    match output_format {
        OutputFormat::Json | OutputFormat::Text => match serde_json::from_str::<Value>(data) {
            Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
            Err(_) => println!("{}", data),
        },
        OutputFormat::Csv => {
            println!("{}", data);
        }
        OutputFormat::Jsonl => match serde_json::from_str::<Value>(data) {
            Ok(json) => print_output(&json, output_format)?,
            Err(_) => println!("{}", data),
        },
        OutputFormat::Toon => match serde_json::from_str::<Value>(data) {
            Ok(json) => print_output(&json, output_format)?,
            Err(_) => println!("{}", data),
        },
        OutputFormat::Auto => match serde_json::from_str::<Value>(data) {
            Ok(json) => print_output(&json, output_format)?,
            Err(_) => println!("{}", data),
        },
    }

    Ok(())
}
pub(super) fn resolve_script(file: Option<String>, code: Option<String>) -> anyhow::Result<String> {
    resolve_script_from(
        file,
        code,
        std::io::stdin().lock(),
        std::io::stdin().is_terminal(),
    )
}

pub(super) fn resolve_script_from<R: std::io::Read>(
    file: Option<String>,
    code: Option<String>,
    reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    if let Some(c) = code {
        if c.trim().is_empty() {
            anyhow::bail!("Empty script provided via --code.");
        }
        return Ok(c);
    }

    if let Some(path) = file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read script file '{}': {}", path, e))?;
        if content.trim().is_empty() {
            anyhow::bail!("Script file '{}' is empty.", path);
        }
        return Ok(content);
    }

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
