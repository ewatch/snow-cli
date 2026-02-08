use crate::cli::args::OutputFormat;
use serde::Serialize;

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

#[cfg(test)]
mod tests {
    use serde::Serialize;

    #[derive(Serialize)]
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
}
