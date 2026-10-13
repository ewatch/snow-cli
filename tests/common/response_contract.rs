//! Black-box response decoding; deliberately does not reuse production writers.
use std::{path::Path, time::Duration};

use assert_cmd::{Command, cargo::cargo_bin_cmd};
use serde_json::Value;

pub const BINARIES: [&str; 2] = ["snow-cli", "snow-cli-ro"];
pub const FORMATS: [&str; 6] = ["json", "jsonl", "toon", "csv", "text", "auto"];
pub const TOKEN: &str = "contract-secret-api-token";

pub fn command(binary: &str, config: &Path, instance: &str, format: &str) -> Command {
    let mut cmd = match binary {
        "snow-cli" => cargo_bin_cmd!("snow-cli"),
        "snow-cli-ro" => cargo_bin_cmd!("snow-cli-ro"),
        _ => panic!("unknown binary {binary}"),
    };
    cmd.timeout(Duration::from_secs(10))
        .env("SNOW_CLI_CONFIG", config)
        .env("SNOW_CLI_API_TOKEN", TOKEN)
        .env_remove("SNOW_CLI_OUTPUT")
        .env_remove("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE")
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .args(["--instance", instance, "--output", format]);
    cmd
}

pub fn structured(format: &str, bytes: &[u8]) -> Value {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(!text.trim().is_empty(), "empty {format} response");
    match format {
        "json" | "text" => serde_json::from_str(text).unwrap(),
        "jsonl" => {
            let lines: Vec<Value> = text
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
            if let Some(meta) = lines[0].get("meta") {
                let mut envelope = meta.clone();
                envelope["records"] = Value::Array(lines[1..].to_vec());
                envelope
            } else if lines.len() == 1 {
                lines[0].clone()
            } else {
                Value::Array(lines)
            }
        }
        "toon" => toon_format::decode_default(text).unwrap(),
        "auto" => match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => toon_format::decode_default(text).unwrap(),
        },
        _ => panic!("not a structured format: {format}"),
    }
}

pub fn csv_rows(bytes: &[u8]) -> Vec<Value> {
    let mut reader = csv::Reader::from_reader(bytes);
    let headers = reader.headers().unwrap().clone();
    reader
        .records()
        .map(|row| {
            let row = row.unwrap();
            Value::Object(
                headers
                    .iter()
                    .zip(row.iter())
                    .map(|(k, v)| (k.to_owned(), Value::String(v.to_owned())))
                    .collect(),
            )
        })
        .collect()
}

pub fn error(bytes: &[u8]) -> Value {
    // Logs may precede the final structured error, but are never result data.
    let text = std::str::from_utf8(bytes).unwrap();
    let start = text
        .find("{\"error\":")
        .unwrap_or_else(|| panic!("missing structured error: {text}"));
    let value: Value = serde_json::from_str(&text[start..]).unwrap();
    assert!(value["error"]["code"].is_string());
    assert!(value["error"]["message"].is_string());
    value
}

pub fn no_credentials(output: &std::process::Output) {
    for stream in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(stream).contains(TOKEN));
    }
}
