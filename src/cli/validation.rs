use std::io::Read;

pub const DEFAULT_MAX_STDIN_BYTES: u64 = 10 * 1024 * 1024;

pub fn read_to_string_limited<R: std::io::Read>(
    reader: R,
    max_bytes: u64,
    label: &str,
) -> anyhow::Result<String> {
    let mut limited = reader.take(max_bytes.saturating_add(1));
    let mut buf = String::new();
    limited.read_to_string(&mut buf)?;
    if buf.len() as u64 > max_bytes {
        anyhow::bail!(
            "{} exceeds the maximum supported size of {} bytes. Use a file-based workflow or reduce the input size.",
            label,
            max_bytes
        );
    }
    Ok(buf)
}

pub fn validate_table_name(table: &str) -> anyhow::Result<()> {
    if table.is_empty() {
        anyhow::bail!("Table name must not be empty.");
    }
    if !table
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        anyhow::bail!(
            "Invalid table name '{}'. Table names may contain only ASCII letters, digits, and underscores.",
            table
        );
    }
    Ok(())
}

pub fn validate_path_segment(label: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("{} must not be empty.", label);
    }
    if value
        .chars()
        .any(|ch| ch == '/' || ch == '?' || ch == '#' || ch.is_control())
    {
        anyhow::bail!(
            "Invalid {} '{}'. Values used in API paths must not contain '/', '?', '#', or control characters.",
            label,
            value
        );
    }
    Ok(())
}

pub fn validate_encoded_query_literal(label: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("{} must not be empty.", label);
    }
    if value
        .chars()
        .any(|ch| ch == '^' || ch == '=' || ch == '<' || ch == '>' || ch == '!' || ch.is_control())
    {
        anyhow::bail!(
            "Invalid {} '{}'. Values embedded in encoded queries must not contain ServiceNow query operator characters.",
            label,
            value
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_to_string_limited_rejects_oversized_input() {
        let err = read_to_string_limited(std::io::Cursor::new(b"abcdef"), 5, "stdin")
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceeds"));
        assert_eq!(
            read_to_string_limited(std::io::Cursor::new(b"abc"), 5, "stdin").unwrap(),
            "abc"
        );
    }

    #[test]
    fn table_names_allow_servicenow_identifiers() {
        assert!(validate_table_name("incident").is_ok());
        assert!(validate_table_name("x_acme_app_table1").is_ok());
    }

    #[test]
    fn table_names_reject_path_and_query_characters() {
        assert!(validate_table_name("incident/foo").is_err());
        assert!(validate_table_name("incident?x=1").is_err());
        assert!(validate_table_name("incident#frag").is_err());
        assert!(validate_table_name("incident^ORactive=true").is_err());
    }

    #[test]
    fn path_segments_reject_path_breakout_characters() {
        assert!(validate_path_segment("sys_id", "abc123").is_ok());
        assert!(validate_path_segment("sys_id", "abc/123").is_err());
        assert!(validate_path_segment("sys_id", "abc?x=1").is_err());
        assert!(validate_path_segment("sys_id", "abc#frag").is_err());
    }

    #[test]
    fn encoded_query_literals_reject_operator_characters() {
        assert!(validate_encoded_query_literal("scope", "x_my_app").is_ok());
        assert!(validate_encoded_query_literal("sys_id", "abc^ORactive=true").is_err());
        assert!(validate_encoded_query_literal("scope", "x=y").is_err());
    }
}
