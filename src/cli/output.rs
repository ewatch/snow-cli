use crate::cli::args::OutputFormat;
use crate::models::record::Record;
use serde::Serialize;
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
    }
    Ok(())
}

/// Write records as CSV to the given writer.
///
/// Uses a BTreeSet to produce deterministic, sorted column headers.
/// Values are stringified from their JSON representation (strings unquoted,
/// others as JSON).
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
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(v) => v.to_string(),
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
