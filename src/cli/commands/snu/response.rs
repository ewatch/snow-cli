use super::*;

pub(super) fn print_response_value(
    response: SnuMessage,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let value = response_value(&response)?;
    print_output(&value, output_format)
}

pub(super) fn response_value(response: &SnuMessage) -> anyhow::Result<Value> {
    let mut value = serde_json::to_value(response)?;
    if let Value::Object(map) = &mut value {
        map.remove("agentRequestId");
        if let Some(instance) = &response.instance {
            map.insert("instance".to_string(), redact_session_for_output(instance));
        }
    }
    Ok(value)
}

pub(super) fn print_background_script_response(
    response: SnuMessage,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let data = response
        .extra
        .get("output")
        .or_else(|| response.extra.get("data"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SN-Utils background script response did not contain output"))?;

    crate::cli::output::print_passthrough(data, output_format)
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
        let content = crate::cli::io::read_input_file_to_string(
            std::path::Path::new(&path),
            "Script file",
            "--file",
        )?;
        if content.trim().is_empty() {
            return Err(crate::models::validation::ValidationError::new(
                "Script file passed to --file is empty.",
            )
            .into());
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
