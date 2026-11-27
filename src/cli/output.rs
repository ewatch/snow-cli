use crate::cli::args::OutputFormat;
use crate::models::record::Record;
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{BufWriter, Write};

mod preview;
pub use preview::print_table_list;
pub(crate) use preview::{
    DomainSummary, TableListExtras, ensure_preview_supported, print_raw_value,
    print_record_preview, print_table_list_preview, write_table_list_jsonl_meta,
};

/// Write a value to stdout in the requested output format.
pub fn print_output<T: Serialize>(value: &T, format: &OutputFormat) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    write_output(value, format, &mut writer)?;
    writer.flush()?;
    Ok(())
}

/// Canonical serializer shared by stdout entry points and framed envelopes.
pub(crate) fn write_output<T: Serialize + ?Sized, W: Write>(
    value: &T,
    format: &OutputFormat,
    writer: &mut W,
) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(&mut *writer, value)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Csv => write_serializable_csv(value, writer)?,
        OutputFormat::Jsonl => {
            write_jsonl_value(&serde_json::to_value(value)?, writer)?;
        }
        OutputFormat::Toon => {
            write_toon(value, writer)?;
        }
        OutputFormat::Text => {
            serde_json::to_writer_pretty(&mut *writer, value)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(value)?, writer)?;
        }
    }
    Ok(())
}

/// Format a JSON value as a CSV cell.
///
/// String values are neutralized against spreadsheet formula injection;
/// non-string JSON values (such as negative numbers `-4` or `-12.5` and
/// booleans) are preserved verbatim.
fn format_csv_cell(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => neutralize_csv_formula(text),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Serialize an arbitrary value as CSV while preserving struct field order,
/// tolerating optional/omitted fields across rows, and neutralizing formula
/// prefixes on string cells only.
fn write_serializable_csv<T: Serialize + ?Sized, W: Write>(
    value: &T,
    writer: &mut W,
) -> anyhow::Result<()> {
    // Round-trip through JSON text + IndexMap so struct field declaration order
    // is preserved (serde_json::Value's default Map sorts keys alphabetically).
    let raw = serde_json::to_string(value)?;
    if let Ok(object) = serde_json::from_str::<IndexMap<String, Value>>(&raw) {
        return write_ordered_object_csv(&object, writer);
    }
    if let Ok(rows) = serde_json::from_str::<Vec<IndexMap<String, Value>>>(&raw) {
        return write_ordered_objects_csv(&rows, writer);
    }
    match serde_json::from_str::<Value>(&raw)? {
        Value::Array(items) => {
            if items.is_empty() {
                return Ok(());
            }
            let mut csv = csv::Writer::from_writer(writer);
            for item in &items {
                csv.write_record([format_csv_cell(Some(item))])?;
            }
            csv.flush()?;
            Ok(())
        }
        Value::Null => Ok(()),
        other => {
            let mut csv = csv::Writer::from_writer(writer);
            csv.write_record([format_csv_cell(Some(&other))])?;
            csv.flush()?;
            Ok(())
        }
    }
}

/// One JSON object as CSV: its keys in declaration order as the header, its
/// values as one row.
fn write_ordered_object_csv<W: Write>(
    object: &IndexMap<String, Value>,
    writer: &mut W,
) -> anyhow::Result<()> {
    let mut csv = csv::Writer::from_writer(writer);
    csv.write_record(object.keys())?;
    csv.write_record(object.values().map(|value| format_csv_cell(Some(value))))?;
    csv.flush()?;
    Ok(())
}

/// A list of JSON objects as CSV: union of keys across all rows in first-seen
/// declaration order as a fixed header, with empty cells for omitted keys and
/// formula neutralization on string cells only.
pub(crate) fn write_ordered_objects_csv<W: Write>(
    rows: &[IndexMap<String, Value>],
    writer: &mut W,
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut columns = IndexSet::new();
    for row in rows {
        for key in row.keys() {
            columns.insert(key.as_str());
        }
    }
    let columns: Vec<&str> = columns.into_iter().collect();

    let mut csv = csv::Writer::from_writer(writer);
    csv.write_record(&columns)?;
    for row in rows {
        let cells: Vec<String> = columns
            .iter()
            .map(|col| format_csv_cell(row.get(*col)))
            .collect();
        csv.write_record(&cells)?;
    }
    csv.flush()?;
    Ok(())
}

/// Write a list of values as CSV to the given writer.
pub(crate) fn write_list_csv<T: Serialize, W: Write>(
    values: &[T],
    writer: &mut W,
) -> anyhow::Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    write_serializable_csv(values, writer)
}

/// Write a list of values to stdout in the requested output format.
pub fn print_list<T: Serialize>(values: &[T], format: &OutputFormat) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(&mut writer, values)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Csv => {
            write_list_csv(values, &mut writer)?;
        }
        OutputFormat::Jsonl => {
            write_jsonl_slice(values, &mut writer)?;
        }
        OutputFormat::Toon => {
            write_toon(values, &mut writer)?;
        }
        OutputFormat::Text => {
            serde_json::to_writer_pretty(&mut writer, values)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(values)?, &mut writer)?;
        }
    }
    writer.flush()?;
    Ok(())
}

/// Write a raw response body from a passthrough command (`api`, `script run`
/// on custom endpoints).
///
/// A JSON body is re-encoded in the requested format, so explicit JSON stays
/// compact. A body that is not JSON, or any body with `--output csv` (raw
/// responses are not tabular), is written unchanged.
pub fn print_passthrough(body: &str, format: &OutputFormat) -> anyhow::Result<()> {
    let json = match format {
        OutputFormat::Csv => None,
        _ => serde_json::from_str::<Value>(body).ok(),
    };
    match json {
        Some(json) => {
            let stdout = std::io::stdout();
            let mut writer = BufWriter::new(stdout.lock());
            write_json_value(&json, format, &mut writer)?;
            writer.flush()?;
            Ok(())
        }
        None => {
            println!("{body}");
            Ok(())
        }
    }
}

/// Serialize an already-parsed `serde_json::Value` without re-cloning it into a second `Value`.
fn write_json_value<W: Write>(
    value: &Value,
    format: &OutputFormat,
    writer: &mut W,
) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(&mut *writer, value)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Csv => write_serializable_csv(value, writer)?,
        OutputFormat::Jsonl => write_jsonl_value(value, writer)?,
        OutputFormat::Toon => write_toon_value(value, writer)?,
        OutputFormat::Text => {
            serde_json::to_writer_pretty(&mut *writer, value)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Auto => emit_auto(value, writer)?,
    }
    Ok(())
}

/// Write a single Record to stdout.
///
/// Records use `#[serde(flatten)]` on a BTreeMap, which the csv crate cannot
/// serialize directly. This function handles both JSON and CSV output for
/// the dynamic field layout.
pub fn print_record(record: &Record, format: &OutputFormat) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    match format {
        OutputFormat::Json | OutputFormat::Jsonl => {
            serde_json::to_writer(&mut writer, record)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Csv => {
            write_records_csv(std::slice::from_ref(record), &mut writer)?;
        }
        OutputFormat::Toon => {
            write_toon(record, &mut writer)?;
        }
        OutputFormat::Text => {
            serde_json::to_writer_pretty(&mut writer, record)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(record)?, &mut writer)?;
        }
    }
    writer.flush()?;
    Ok(())
}

/// Write a list of Records to stdout.
///
/// Collects all unique field names across all records to build the CSV header
/// row, then writes each record's values in that column order.
pub fn print_records(records: &[Record], format: &OutputFormat) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(&mut writer, records)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Csv => {
            write_records_csv(records, &mut writer)?;
        }
        OutputFormat::Jsonl => {
            write_jsonl_slice(records, &mut writer)?;
        }
        OutputFormat::Toon => {
            write_toon(records, &mut writer)?;
        }
        OutputFormat::Text => {
            serde_json::to_writer_pretty(&mut writer, records)?;
            writer.write_all(b"\n")?;
        }
        OutputFormat::Auto => {
            emit_auto(&serde_json::to_value(records)?, &mut writer)?;
        }
    }
    writer.flush()?;
    Ok(())
}

/// Stream a slice of items as JSON Lines directly to `writer` without cloning
/// into an intermediate `serde_json::Value`.
pub(crate) fn write_jsonl_slice<T: Serialize, W: Write>(
    items: &[T],
    writer: &mut W,
) -> anyhow::Result<()> {
    for item in items {
        serde_json::to_writer(&mut *writer, item)?;
        writer.write_all(b"\n")?;
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
                serde_json::to_writer(&mut *writer, item)?;
                writer.write_all(b"\n")?;
            }
        }
        _ => {
            serde_json::to_writer(&mut *writer, value)?;
            writer.write_all(b"\n")?;
        }
    }

    Ok(())
}

/// Write an already-constructed `serde_json::Value` as TOON without cloning it.
pub(crate) fn write_toon_value<W: Write>(value: &Value, writer: &mut W) -> anyhow::Result<()> {
    let toon = toon_format::encode_default(value)?;
    writer.write_all(toon.as_bytes())?;
    writer.write_all(b"\n")?;
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
    write_toon_value(&json, writer)
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
/// Each candidate is encoded at most once without cloning `value`, and losing
/// buffers are dropped immediately so only the winning payload is retained and
/// written to `writer`. TOON is skipped for scalar/null values, where it
/// offers no structural benefit and a bare unquoted scalar would be ambiguous.
pub(crate) fn emit_auto<W: Write>(value: &Value, writer: &mut W) -> anyhow::Result<()> {
    // Compact JSON is both the fallback and the baseline candidate.
    let mut best_buf = serde_json::to_vec(value)?;
    best_buf.push(b'\n');
    let json_len = best_buf.len();
    let mut chosen = OutputFormat::Json;

    // For non-arrays, JSONL emits the exact same compact JSON line as JSON, so
    // it can never be strictly smaller than `json_len`.
    let jsonl_len = if value.is_array() {
        let mut jsonl_buf = Vec::with_capacity(json_len);
        write_jsonl_value(value, &mut jsonl_buf)?;
        let len = jsonl_buf.len();
        if len < best_buf.len() {
            best_buf = jsonl_buf;
            chosen = OutputFormat::Jsonl;
        }
        len
    } else {
        json_len
    };

    // TOON only meaningfully competes for structured data (objects/arrays);
    // skipping scalars avoids emitting an ambiguous bare unquoted value.
    let toon_len = if value.is_object() || value.is_array() {
        let mut toon = toon_format::encode_default(value)?;
        toon.push('\n');
        let len = toon.len();
        if len < best_buf.len() {
            best_buf = toon.into_bytes();
            chosen = OutputFormat::Toon;
        }
        Some(len)
    } else {
        None
    };

    tracing::info!(
        "auto output -> {} ({} B; json={} jsonl={} toon={})",
        chosen.as_str(),
        best_buf.len(),
        json_len,
        jsonl_len,
        toon_len.map_or_else(|| "n/a".to_string(), |len| len.to_string()),
    );

    writer.write_all(&best_buf)?;
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

/// Streaming CSV writer for paginated record reads with a stable column schema.
///
/// The column header is derived from the first non-empty page (sorted via
/// `BTreeSet` to match [`write_records_csv`]) and written once; each subsequent
/// page streams its rows immediately without buffering all records in memory.
pub(crate) struct CsvRecordStream<W: Write> {
    csv_writer: csv::Writer<W>,
    columns: Option<Vec<String>>,
}

impl<W: Write> CsvRecordStream<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self {
            csv_writer: csv::Writer::from_writer(writer),
            columns: None,
        }
    }

    pub(crate) fn write_page(&mut self, records: &[Record]) -> anyhow::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let columns = match &self.columns {
            Some(columns) => columns,
            None => {
                let mut set = BTreeSet::new();
                for record in records {
                    for key in record.fields.keys() {
                        set.insert(key.clone());
                    }
                }
                let cols: Vec<String> = set.into_iter().collect();
                self.csv_writer.write_record(&cols)?;
                self.columns.insert(cols)
            }
        };
        let mut row = Vec::with_capacity(columns.len());
        for record in records {
            row.clear();
            for col in columns {
                row.push(format_csv_cell(record.fields.get(col)));
            }
            self.csv_writer.write_record(&row)?;
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> anyhow::Result<()> {
        self.csv_writer.flush()?;
        Ok(())
    }
}

pub(crate) fn write_records_csv<W: Write>(
    records: &[Record],
    writer: &mut W,
) -> anyhow::Result<()> {
    let mut stream = CsvRecordStream::new(writer);
    stream.write_page(records)?;
    stream.finish()
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
    use std::collections::BTreeMap;

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
                fields: BTreeMap::from([
                    ("sys_id".to_string(), serde_json::json!("abc123")),
                    ("number".to_string(), serde_json::json!("INC001")),
                ]),
            },
            Record {
                fields: BTreeMap::from([
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
                fields: BTreeMap::from([
                    ("sys_id".to_string(), serde_json::json!("abc123")),
                    ("state".to_string(), serde_json::json!("1")),
                ]),
            },
            Record {
                fields: BTreeMap::from([
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
            fields: BTreeMap::from([
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
            fields: BTreeMap::from([
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

    #[test]
    fn test_write_records_csv_preserves_negative_numbers_without_formula_prefix() {
        let records = vec![Record {
            fields: BTreeMap::from([
                ("delta".to_string(), serde_json::json!(-4)),
                ("score".to_string(), serde_json::json!(-12.5)),
                ("formula".to_string(), serde_json::json!("-12.5")),
                ("expr".to_string(), serde_json::json!("=1+1")),
            ]),
        }];

        let mut output = Vec::new();
        write_records_csv(&records, &mut output).unwrap();
        let csv_str = String::from_utf8(output).unwrap();

        assert_eq!(csv_str, "delta,expr,formula,score\n-4,'=1+1,'-12.5,-12.5\n");
    }

    #[test]
    fn test_write_list_csv_handles_optional_fields_and_neutralizes_formulas() {
        #[derive(serde::Serialize)]
        struct Item {
            column: String,
            label: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            reference: Option<String>,
            offset: i64,
        }

        let items = vec![
            Item {
                column: "short_description".to_string(),
                label: "=HYPERLINK(\"http://evil\")".to_string(),
                reference: None,
                offset: -4,
            },
            Item {
                column: "caller_id".to_string(),
                label: "+Caller".to_string(),
                reference: Some("sys_user".to_string()),
                offset: -12,
            },
        ];

        let mut output = Vec::new();
        write_list_csv(&items, &mut output).unwrap();
        let csv_str = String::from_utf8(output).unwrap();

        assert!(
            csv_str.starts_with("column,label,offset,reference\n"),
            "{csv_str}"
        );
        assert!(
            csv_str.contains("short_description,\"'=HYPERLINK(\"\"http://evil\"\")\",-4,\n"),
            "{csv_str}"
        );
        assert!(
            csv_str.contains("caller_id,'+Caller,-12,sys_user\n"),
            "{csv_str}"
        );
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
    fn csv_writes_a_single_object_as_header_and_row() {
        let mut out = Vec::new();
        write_output(
            &serde_json::json!({"status": "updated", "default_profile": "=cmd", "n": 2, "x": null}),
            &OutputFormat::Csv,
            &mut out,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        let mut lines = text.lines();
        let header = lines.next().unwrap();
        assert!(
            header.contains("status") && header.contains("default_profile"),
            "{text}"
        );
        let row = lines.next().unwrap();
        assert!(
            row.contains("updated") && row.contains("'=cmd") && row.contains('2'),
            "{text}"
        );
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

    /// Lightweight zero-dependency regression test covering representative
    /// ServiceNow payload classes (servicenow-cli-90.4): verifies lossless
    /// round-trip across JSON, JSONL, TOON, and `auto`, and confirms `auto`
    /// picks compact TOON for uniform tabular lists while keeping JSON for
    /// scalars and cases where TOON does not save bytes.
    #[test]
    fn format_round_trip_and_auto_selection_across_servicenow_payload_classes() {
        let flat_table_rows = serde_json::json!({
            "total": 4,
            "returned": 4,
            "offset": 0,
            "next_offset": null,
            "truncated": false,
            "records": [
                {"sys_id": "0001", "number": "INC0010001", "state": "In Progress", "priority": "1 - Critical", "assigned_to": "Abel Tuter"},
                {"sys_id": "0002", "number": "INC0010002", "state": "New", "priority": "2 - High", "assigned_to": "Beth Anglin"},
                {"sys_id": "0003", "number": "INC0010003", "state": "On Hold", "priority": "3 - Moderate", "assigned_to": ""},
                {"sys_id": "0004", "number": "INC0010004", "state": "Resolved", "priority": "4 - Low", "assigned_to": "David Loo"}
            ]
        });
        let nested_references = serde_json::json!({
            "records": [
                {"number": "INC0010001", "assigned_to": {"value": "u1", "link": "https://dev.service-now.com/api/now/table/sys_user/u1"}},
                {"number": "INC0010002", "assigned_to": ""}
            ]
        });
        let irregular_objects = serde_json::json!([
            {"sys_id": "1", "name": "Rule A", "condition": "active=true"},
            {"sys_id": "2", "name": "Rule B", "tags": ["a", "b"], "meta": {"order": 100}}
        ]);
        let long_text = serde_json::json!({
            "sys_id": "abc123",
            "name": "ChangeRequestStateHandler",
            "script": format!("// line 1: \"quoted\", commas, colons: {}\nvar x = 1;\n", "é".repeat(500))
        });
        let empty_list = serde_json::json!({
            "total": 0,
            "returned": 0,
            "offset": 0,
            "next_offset": null,
            "truncated": false,
            "records": []
        });
        let aggregate_rows = serde_json::json!([
            {"state": "New", "priority": "1 - Critical", "count": 4, "avg_reassignment_count": 1.25},
            {"state": "In Progress", "priority": "2 - High", "count": 12, "avg_reassignment_count": 2.5},
            {"state": "Resolved", "priority": "3 - Moderate", "count": 30, "avg_reassignment_count": 0.75}
        ]);
        let error_payload = serde_json::json!({
            "error": {
                "code": "INVALID_TABLE",
                "message": "Invalid table name 'incidnt'",
                "class": "api",
                "retry": "never",
                "status": 400
            }
        });
        let export_artifact = serde_json::json!({
            "kind": "table-export",
            "table": "incident",
            "exported_at": "2026-11-27T16:00:00Z",
            "records": [
                {"sys_id": "0001", "number": "INC0010001", "short_description": "VPN down"},
                {"sys_id": "0002", "number": "INC0010002", "short_description": "Printer jam"}
            ]
        });

        let cases: &[(&str, &Value, bool)] = &[
            ("flat_table_rows", &flat_table_rows, true),
            ("nested_references", &nested_references, false),
            ("irregular_objects", &irregular_objects, false),
            ("long_text", &long_text, false),
            ("empty_list", &empty_list, false),
            ("aggregate_rows", &aggregate_rows, true),
            ("error_payload", &error_payload, false),
            ("export_artifact", &export_artifact, true),
        ];

        for (name, value, expect_toon_smaller_than_json) in cases {
            let mut json_buf = Vec::new();
            write_output(value, &OutputFormat::Json, &mut json_buf).unwrap();
            let decoded_json: Value = serde_json::from_slice(&json_buf).unwrap();
            assert_eq!(&decoded_json, *value, "{name}: json round-trip");

            let mut jsonl_buf = Vec::new();
            write_output(value, &OutputFormat::Jsonl, &mut jsonl_buf).unwrap();
            let jsonl_str = String::from_utf8(jsonl_buf).unwrap();
            let lines: Vec<Value> = jsonl_str
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let decoded_jsonl = if value.is_array() {
                Value::Array(lines)
            } else {
                assert_eq!(lines.len(), 1, "{name}: object jsonl line count");
                lines.into_iter().next().unwrap()
            };
            assert_eq!(&decoded_jsonl, *value, "{name}: jsonl round-trip");

            let mut toon_buf = Vec::new();
            write_output(value, &OutputFormat::Toon, &mut toon_buf).unwrap();
            let toon_str = String::from_utf8(toon_buf).unwrap();
            let decoded_toon: Value = toon_format::decode_default(&toon_str).unwrap();
            assert_eq!(&decoded_toon, *value, "{name}: toon round-trip");

            let auto_out = run_emit_auto(value);
            assert_eq!(auto_out, expected_auto(value), "{name}: auto selector");
            assert!(
                auto_out.len() <= json_buf.len(),
                "{name}: auto ({} B) must never exceed compact JSON ({} B)",
                auto_out.len(),
                json_buf.len()
            );
            let decoded_auto: Value = if value.is_array() && auto_out.lines().count() > 1 {
                match auto_out
                    .lines()
                    .map(serde_json::from_str::<Value>)
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(items) => Value::Array(items),
                    Err(_) => toon_format::decode_default(&auto_out).unwrap(),
                }
            } else {
                serde_json::from_str::<Value>(&auto_out)
                    .unwrap_or_else(|_| toon_format::decode_default(&auto_out).unwrap())
            };
            assert_eq!(&decoded_auto, *value, "{name}: auto round-trip");

            if *expect_toon_smaller_than_json {
                assert!(
                    toon_str.len() < json_buf.len(),
                    "{name}: expected TOON ({} B) < JSON ({} B)",
                    toon_str.len(),
                    json_buf.len()
                );
            }
        }
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
