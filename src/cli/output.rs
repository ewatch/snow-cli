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
}
