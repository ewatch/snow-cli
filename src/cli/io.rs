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
}
