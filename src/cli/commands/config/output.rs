use super::*;

/// `config output [FORMAT] [--reset]` — show or set the default output format.
///
/// With no arguments, prints the configured default and the effective format it
/// resolves to (json when unset). With a FORMAT, persists it. With `--reset`,
/// clears the setting so the built-in json fallback applies.
pub(super) async fn handle_output_default(
    config_path: &Path,
    format: Option<OutputFormat>,
    reset: bool,
) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;

    if reset {
        config.default_output = None;
        config.save_to(config_path)?;
        tracing::info!("Cleared default output format");
        let result = serde_json::json!({
            "status": "updated",
            "default_output": serde_json::Value::Null,
        });
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    match format {
        Some(fmt) => {
            config.default_output = Some(fmt.as_str().to_string());
            config.save_to(config_path)?;
            tracing::info!("Default output format set to '{}'", fmt.as_str());
            let result = serde_json::json!({
                "status": "updated",
                "default_output": fmt.as_str(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        None => {
            let configured = config.default_output.as_deref();
            // Effective value from config alone (ignoring env/flag): the stored
            // value if valid, else the json fallback.
            let effective = configured
                .and_then(OutputFormat::from_str_opt)
                .unwrap_or(OutputFormat::Json);
            let result = serde_json::json!({
                "default_output": configured,
                "effective": effective.as_str(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok(())
}
