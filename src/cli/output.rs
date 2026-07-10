use crate::cli::args::OutputFormat;
use crate::models::record::Record;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;

/// Write a value to stdout in the requested output format.
pub fn print_output<T: Serialize>(value: &T, format: &OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string(value)?;
            println!("{json}");
        }
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            writer.serialize(value)?;
            writer.flush()?;
        }
        OutputFormat::Jsonl => {
            write_jsonl_value(&serde_json::to_value(value)?, &mut std::io::stdout())?;
        }
        OutputFormat::Toon => {
            write_toon(value, &mut std::io::stdout())?;
        }
        OutputFormat::Text => {
            let json = serde_json::to_string_pretty(value)?;
            println!("{json}");
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(value)?, &mut std::io::stdout())?;
        }
    }
    Ok(())
}

/// Write a list of values to stdout in the requested output format.
pub fn print_list<T: Serialize>(values: &[T], format: &OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string(values)?;
            println!("{json}");
        }
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            for value in values {
                writer.serialize(value)?;
            }
            writer.flush()?;
        }
        OutputFormat::Jsonl => {
            write_jsonl_value(&serde_json::to_value(values)?, &mut std::io::stdout())?;
        }
        OutputFormat::Toon => {
            write_toon(values, &mut std::io::stdout())?;
        }
        OutputFormat::Text => {
            let json = serde_json::to_string_pretty(values)?;
            println!("{json}");
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(values)?, &mut std::io::stdout())?;
        }
    }
    Ok(())
}

/// Write a single Record to stdout.
///
/// Records use `#[serde(flatten)]` on a HashMap, which the csv crate cannot
/// serialize directly. This function handles both JSON and CSV output for
/// the dynamic field layout.
pub fn print_record(record: &Record, format: &OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string(record)?;
            println!("{json}");
        }
        OutputFormat::Csv => {
            write_records_csv(std::slice::from_ref(record), &mut std::io::stdout())?;
        }
        OutputFormat::Jsonl => {
            write_jsonl_value(&serde_json::to_value(record)?, &mut std::io::stdout())?;
        }
        OutputFormat::Toon => {
            write_toon(record, &mut std::io::stdout())?;
        }
        OutputFormat::Text => {
            let json = serde_json::to_string_pretty(record)?;
            println!("{json}");
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(record)?, &mut std::io::stdout())?;
        }
    }
    Ok(())
}

/// Write a list of Records to stdout.
///
/// Collects all unique field names across all records to build the CSV header
/// row, then writes each record's values in that column order.
pub fn print_records(records: &[Record], format: &OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string(records)?;
            println!("{json}");
        }
        OutputFormat::Csv => {
            write_records_csv(records, &mut std::io::stdout())?;
        }
        OutputFormat::Jsonl => {
            write_jsonl_value(&serde_json::to_value(records)?, &mut std::io::stdout())?;
        }
        OutputFormat::Toon => {
            write_toon(records, &mut std::io::stdout())?;
        }
        OutputFormat::Text => {
            let json = serde_json::to_string_pretty(records)?;
            println!("{json}");
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(records)?, &mut std::io::stdout())?;
        }
    }
    Ok(())
}

/// Write a JSON value as JSON Lines to the given writer.
///
/// Arrays are emitted as one compact JSON value per line. Objects and scalar
/// values are emitted as a single compact JSON line.
pub(crate) fn write_jsonl_value<W: Write>(value: &Value, writer: &mut W) -> anyhow::Result<()> {
    match value {
        Value::Array(items) => {
            for item in items {
                writeln!(writer, "{}", serde_json::to_string(item)?)?;
            }
        }
        _ => {
            writeln!(writer, "{}", serde_json::to_string(value)?)?;
        }
    }

    Ok(())
}

/// Write any serializable value as TOON to the given writer.
///
/// TOON is most compact for arrays of similarly shaped objects, but the
/// encoder handles all JSON-shaped values so callers do not need special-case
/// fallbacks for nested or irregular API responses.
pub(crate) fn write_toon<T: Serialize + ?Sized, W: Write>(
    value: &T,
    writer: &mut W,
) -> anyhow::Result<()> {
    let json = serde_json::to_value(value)?;
    let toon = toon_format::encode_default(&json)?;
    writeln!(writer, "{toon}")?;
    Ok(())
}

/// Emit `value` in whichever lossless format encodes to the fewest bytes.
///
/// Candidates are compact JSON, JSON Lines, and TOON — all round-trippable
/// (CSV is excluded as lossy, pretty Text never wins). Byte length stands in
/// for token count: for two encodings of the same data the smaller-byte one is
/// almost always the fewer-token one, so the decision matches a real tokenizer
/// while staying dependency-free.
///
/// Tie-breaking is strict-improvement with priority `json > jsonl > toon`: the
/// output stays compact JSON unless another format is *strictly* smaller, so
/// `auto` only ever leaves the most widely-parseable format to actually save
/// bytes.
///
/// To keep peak memory at roughly one encoded copy (not three), each candidate
/// is encoded into a scratch buffer, measured, and dropped; only the winner is
/// re-encoded to `writer`. TOON is skipped for scalar/null values, where it
/// offers no structural benefit and a bare unquoted scalar would be ambiguous.
pub(crate) fn emit_auto<W: Write>(value: &Value, writer: &mut W) -> anyhow::Result<()> {
    // Compact JSON is both the fallback and a candidate; keep it around so the
    // winner-is-json case needs no re-encode. Lengths are measured as the actual
    // emitted bytes (every writer path appends a trailing newline) so the three
    // candidates are compared apples-to-apples.
    let json_string = serde_json::to_string(value)?;
    let json_len = json_string.len() + 1;

    let jsonl_len = {
        let mut buf = Vec::new();
        write_jsonl_value(value, &mut buf)?;
        buf.len()
    };

    // TOON only meaningfully competes for structured data (objects/arrays);
    // skipping scalars avoids emitting an ambiguous bare unquoted value.
    let toon_len = if value.is_object() || value.is_array() {
        let mut buf = Vec::new();
        write_toon(value, &mut buf)?;
        Some(buf.len())
    } else {
        None
    };

    // Strict-improvement with ties -> json > jsonl > toon: stay on the most
    // widely-parseable format unless another is strictly smaller.
    let mut chosen = OutputFormat::Json;
    let mut best = json_len;
    if jsonl_len < best {
        chosen = OutputFormat::Jsonl;
        best = jsonl_len;
    }
    if let Some(toon_len) = toon_len
        && toon_len < best
    {
        chosen = OutputFormat::Toon;
        best = toon_len;
    }

    tracing::info!(
        "auto output -> {} ({} B; json={} jsonl={} toon={})",
        chosen.as_str(),
        best,
        json_len,
        jsonl_len,
        toon_len.map_or_else(|| "n/a".to_string(), |len| len.to_string()),
    );

    match chosen {
        OutputFormat::Jsonl => write_jsonl_value(value, writer)?,
        OutputFormat::Toon => write_toon(value, writer)?,
        // Json (and any non-candidate) fall back to compact JSON.
        _ => writeln!(writer, "{json_string}")?,
    }

    Ok(())
}

/// Resolve the effective output format from the precedence chain:
/// explicit `--output` flag > `SNOW_CLI_OUTPUT` env var > configured default >
/// built-in `json` fallback.
///
/// The env var and configured default are parsed leniently: an unknown value is
/// ignored (with a stderr warning) rather than failing the command, so a typo
/// never blocks the CLI.
pub fn resolve_output_format(
    flag: Option<&OutputFormat>,
    configured_default: Option<&str>,
) -> OutputFormat {
    let env_value = std::env::var("SNOW_CLI_OUTPUT").ok();
    resolve_output_format_from(flag, env_value.as_deref(), configured_default)
}

/// Pure core of [`resolve_output_format`], with the env var supplied explicitly
/// so the precedence logic is testable without mutating process environment.
fn resolve_output_format_from(
    flag: Option<&OutputFormat>,
    env_value: Option<&str>,
    configured_default: Option<&str>,
) -> OutputFormat {
    if let Some(format) = flag {
        return format.clone();
    }

    if let Some(env_value) = env_value {
        let trimmed = env_value.trim();
        if !trimmed.is_empty() {
            match OutputFormat::from_str_opt(trimmed) {
                Some(format) => return format,
                None => tracing::warn!(
                    "ignoring unknown SNOW_CLI_OUTPUT value '{}'; falling back",
                    trimmed
                ),
            }
        }
    }

    if let Some(configured) = configured_default {
        let trimmed = configured.trim();
        if !trimmed.is_empty() {
            match OutputFormat::from_str_opt(trimmed) {
                Some(format) => return format,
                None => tracing::warn!(
                    "ignoring unknown default_output '{}' in config; using json",
                    trimmed
                ),
            }
        }
    }

    OutputFormat::Json
}

/// Write records as CSV to the given writer.
///
/// Uses a BTreeSet to produce deterministic, sorted column headers.
/// Values are stringified from their JSON representation (strings unquoted,
/// others as JSON).
pub(crate) fn neutralize_csv_formula(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

pub(crate) fn write_records_csv<W: Write>(
    records: &[Record],
    writer: &mut W,
) -> anyhow::Result<()> {
    if records.is_empty() {
        return Ok(());
    }

    // Collect all unique field names in sorted order
    let mut columns = BTreeSet::new();
    for record in records {
        for key in record.fields.keys() {
            columns.insert(key.clone());
        }
    }
    let columns: Vec<String> = columns.into_iter().collect();

    let mut csv_writer = csv::Writer::from_writer(writer);

    // Write header
    csv_writer.write_record(&columns)?;

    // Write data rows
    for record in records {
        let row: Vec<String> = columns
            .iter()
            .map(|col| match record.fields.get(col) {
                Some(serde_json::Value::String(s)) => neutralize_csv_formula(s),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(v) => neutralize_csv_formula(&v.to_string()),
            })
            .collect();
        csv_writer.write_record(&row)?;
    }

    csv_writer.flush()?;
    Ok(())
}

/// Write a simple success/status message to stdout as JSON.
pub fn print_status(message: &str, format: &OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::json!({ "status": "success", "message": message });
            println!("{json}");
        }
        OutputFormat::Csv => {
            // For non-tabular status messages, just print plaintext
            println!("{message}");
        }
        OutputFormat::Jsonl => {
            let json = serde_json::json!({ "status": "success", "message": message });
            write_jsonl_value(&json, &mut std::io::stdout())?;
        }
        OutputFormat::Toon => {
            let json = serde_json::json!({ "status": "success", "message": message });
            write_toon(&json, &mut std::io::stdout())?;
        }
        OutputFormat::Text => {
            println!("{message}");
        }
        OutputFormat::Auto => {
            let json = serde_json::json!({ "status": "success", "message": message });
            emit_auto(&json, &mut std::io::stdout())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(serde::Serialize)]
    struct TestRecord {
        id: String,
        name: String,
    }

    #[test]
    fn test_json_serialization() {
        let record = TestRecord {
            id: "abc123".to_string(),
            name: "Test".to_string(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("abc123"));
        assert!(json.contains("Test"));
    }

    #[test]
    fn test_write_jsonl_value_array() {
        let value = serde_json::json!([
            {"number": "INC001", "state": "2"},
            {"number": "INC002", "state": "3"}
        ]);

        let mut output = Vec::new();
        write_jsonl_value(&value, &mut output).unwrap();
        let jsonl = String::from_utf8(output).unwrap();

        let lines: Vec<&str> = jsonl.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(lines[0]).unwrap(),
            serde_json::json!({"number": "INC001", "state": "2"})
        );
    }

    #[test]
    fn test_write_jsonl_value_object() {
        let value = serde_json::json!({"status": "success", "message": "ok"});

        let mut output = Vec::new();
        write_jsonl_value(&value, &mut output).unwrap();
        let jsonl = String::from_utf8(output).unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(jsonl.trim()).unwrap(),
            value
        );
    }

    #[test]
    fn test_write_toon_flat_records() {
        let value = serde_json::json!([
            {"number": "INC001", "state": "2"},
            {"number": "INC002", "state": "3"}
        ]);

        let mut output = Vec::new();
        write_toon(&value, &mut output).unwrap();
        let toon = String::from_utf8(output).unwrap();

        assert!(toon.contains("[2]"));
        assert!(toon.contains("number"));
        assert!(toon.contains("state"));
        assert!(toon.contains("INC001"));
        assert!(toon.contains("INC002"));
    }

    #[test]
    fn test_write_toon_nested_value() {
        let value = serde_json::json!({
            "result": [{"number": "INC001", "tags": ["agent", "llm"]}],
            "count": 1
        });

        let mut output = Vec::new();
        write_toon(&value, &mut output).unwrap();
        let toon = String::from_utf8(output).unwrap();

        assert!(toon.contains("result"));
        assert!(toon.contains("tags"));
        assert!(toon.contains("agent"));
    }

    #[test]
    fn test_write_records_csv_basic() {
        let records = vec![
            Record {
                fields: HashMap::from([
                    ("sys_id".to_string(), serde_json::json!("abc123")),
                    ("number".to_string(), serde_json::json!("INC001")),
                ]),
            },
            Record {
                fields: HashMap::from([
                    ("sys_id".to_string(), serde_json::json!("def456")),
                    ("number".to_string(), serde_json::json!("INC002")),
                ]),
            },
        ];

        let mut output = Vec::new();
        write_records_csv(&records, &mut output).unwrap();
        let csv_str = String::from_utf8(output).unwrap();

        // BTreeSet gives sorted columns: number, sys_id
        assert!(csv_str.starts_with("number,sys_id\n"));
        assert!(csv_str.contains("INC001,abc123"));
        assert!(csv_str.contains("INC002,def456"));
    }

    #[test]
    fn test_write_records_csv_missing_fields() {
        let records = vec![
            Record {
                fields: HashMap::from([
                    ("sys_id".to_string(), serde_json::json!("abc123")),
                    ("state".to_string(), serde_json::json!("1")),
                ]),
            },
            Record {
                fields: HashMap::from([
                    ("sys_id".to_string(), serde_json::json!("def456")),
                    ("priority".to_string(), serde_json::json!("2")),
                ]),
            },
        ];

        let mut output = Vec::new();
        write_records_csv(&records, &mut output).unwrap();
        let csv_str = String::from_utf8(output).unwrap();

        // All 3 columns should be present: priority, state, sys_id
        assert!(csv_str.starts_with("priority,state,sys_id\n"));
        // First record has no priority, second has no state
        let lines: Vec<&str> = csv_str.trim().split('\n').collect();
        assert_eq!(lines.len(), 3); // header + 2 rows
    }

    #[test]
    fn test_write_records_csv_empty() {
        let records: Vec<Record> = vec![];
        let mut output = Vec::new();
        write_records_csv(&records, &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn test_write_records_csv_neutralizes_spreadsheet_formulas() {
        let records = vec![Record {
            fields: HashMap::from([
                (
                    "formula".to_string(),
                    serde_json::json!("=HYPERLINK(\"http://evil\")"),
                ),
                ("plus".to_string(), serde_json::json!("+SUM(1,2)")),
                ("minus".to_string(), serde_json::json!("-1+2")),
                ("at".to_string(), serde_json::json!("@cmd")),
                ("normal".to_string(), serde_json::json!("hello")),
            ]),
        }];

        let mut output = Vec::new();
        write_records_csv(&records, &mut output).unwrap();
        let csv_str = String::from_utf8(output).unwrap();

        assert!(csv_str.contains("'=HYPERLINK"));
        assert!(csv_str.contains("'+SUM"));
        assert!(csv_str.contains("'-1+2"));
        assert!(csv_str.contains("'@cmd"));
        assert!(csv_str.contains("hello"));
    }

    #[test]
    fn test_write_records_csv_numeric_and_null_values() {
        let records = vec![Record {
            fields: HashMap::from([
                ("count".to_string(), serde_json::json!(42)),
                ("active".to_string(), serde_json::json!(true)),
                ("notes".to_string(), serde_json::Value::Null),
            ]),
        }];

        let mut output = Vec::new();
        write_records_csv(&records, &mut output).unwrap();
        let csv_str = String::from_utf8(output).unwrap();

        // Columns sorted: active, count, notes
        assert!(csv_str.starts_with("active,count,notes\n"));
        assert!(csv_str.contains("true,42,"));
    }

    fn run_emit_auto(value: &Value) -> String {
        let mut buf = Vec::new();
        emit_auto(value, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// Independently recompute the expected winner and assert emit_auto matches.
    fn expected_auto(value: &Value) -> String {
        let json_string = serde_json::to_string(value).unwrap();
        let json_out = format!("{json_string}\n");

        let mut jsonl = Vec::new();
        write_jsonl_value(value, &mut jsonl).unwrap();

        let toon = if value.is_object() || value.is_array() {
            let mut buf = Vec::new();
            write_toon(value, &mut buf).unwrap();
            Some(buf)
        } else {
            None
        };

        // Strict-improvement, ties -> json > jsonl > toon.
        let mut best_len = json_out.len();
        let mut best = json_out.clone();
        if jsonl.len() < best_len {
            best_len = jsonl.len();
            best = String::from_utf8(jsonl).unwrap();
        }
        if let Some(toon) = toon
            && toon.len() < best_len
        {
            best = String::from_utf8(toon).unwrap();
        }
        best
    }

    #[test]
    fn emit_auto_picks_smallest_candidate() {
        let cases = vec![
            // Uniform array of flat objects: TOON's tabular form should win.
            serde_json::json!([
                {"number": "INC001", "state": "2", "priority": "1"},
                {"number": "INC002", "state": "3", "priority": "2"},
                {"number": "INC003", "state": "1", "priority": "4"},
            ]),
            // Single object.
            serde_json::json!({"number": "INC001", "state": "2"}),
            // Nested / irregular payload.
            serde_json::json!({"nested": {"x": [1, 2, 3]}, "list": [{"k": "v"}]}),
            // Bare scalar: TOON is skipped, stays JSON.
            serde_json::json!("hello"),
        ];

        for value in &cases {
            assert_eq!(run_emit_auto(value), expected_auto(value), "value: {value}");
        }
    }

    #[test]
    fn emit_auto_wins_on_uniform_array() {
        let value = serde_json::json!([
            {"number": "INC001", "state": "2", "priority": "1"},
            {"number": "INC002", "state": "3", "priority": "2"},
            {"number": "INC003", "state": "1", "priority": "4"},
        ]);
        let out = run_emit_auto(&value);
        // TOON tabular header proves TOON was chosen and it is strictly smaller.
        assert!(out.contains("[3]{"), "expected TOON output, got: {out}");
        assert!(out.len() < serde_json::to_string(&value).unwrap().len() + 1);
    }

    #[test]
    fn emit_auto_keeps_scalars_as_json() {
        assert_eq!(run_emit_auto(&serde_json::json!("hello")), "\"hello\"\n");
        assert_eq!(run_emit_auto(&serde_json::json!(42)), "42\n");
    }

    #[test]
    fn resolve_output_flag_wins() {
        assert_eq!(
            resolve_output_format_from(Some(&OutputFormat::Toon), Some("json"), Some("csv")),
            OutputFormat::Toon
        );
    }

    #[test]
    fn resolve_output_env_beats_config() {
        assert_eq!(
            resolve_output_format_from(None, Some("jsonl"), Some("csv")),
            OutputFormat::Jsonl
        );
    }

    #[test]
    fn resolve_output_config_used_when_no_flag_or_env() {
        assert_eq!(
            resolve_output_format_from(None, None, Some("toon")),
            OutputFormat::Toon
        );
        assert_eq!(
            resolve_output_format_from(None, None, Some("auto")),
            OutputFormat::Auto
        );
    }

    #[test]
    fn resolve_output_falls_back_to_json() {
        // Nothing set.
        assert_eq!(
            resolve_output_format_from(None, None, None),
            OutputFormat::Json
        );
        // Unknown env and config values are ignored, not fatal.
        assert_eq!(
            resolve_output_format_from(None, Some("yaml"), Some("garbage")),
            OutputFormat::Json
        );
        // Blank values are treated as unset.
        assert_eq!(
            resolve_output_format_from(None, Some("  "), None),
            OutputFormat::Json
        );
    }
}
