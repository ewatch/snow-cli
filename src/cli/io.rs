use crate::models::validation::ValidationError;
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
        return Err(ValidationError::new(format!(
            "{} exceeds the maximum supported size of {} bytes. Use a file-based workflow or reduce the input size.",
            label,
            max_bytes
        )).into());
    }
    Ok(buf)
}

/// Reads a user-supplied input file, reporting failures as
/// [`InputFileError`](crate::error::InputFileError) so the error names the
/// argument (for example `--file`) without echoing the private path.
pub fn read_input_file(
    path: &std::path::Path,
    label: &'static str,
    argument: &'static str,
) -> anyhow::Result<Vec<u8>> {
    std::fs::read(path).map_err(|error| {
        crate::error::InputFileError {
            label,
            argument,
            kind: error.kind(),
        }
        .into()
    })
}

/// [`read_input_file`] for UTF-8 text inputs.
pub fn read_input_file_to_string(
    path: &std::path::Path,
    label: &'static str,
    argument: &'static str,
) -> anyhow::Result<String> {
    let bytes = read_input_file(path, label, argument)?;
    String::from_utf8(bytes).map_err(|_| {
        ValidationError::new(format!("{label} passed to {argument} is not valid UTF-8")).into()
    })
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
    fn read_input_file_names_argument_not_path() {
        let path = std::path::Path::new("definitely-missing-snow-cli-input.json");
        let error = read_input_file(path, "Dataset file", "--file").unwrap_err();
        let typed = error
            .downcast_ref::<crate::error::InputFileError>()
            .expect("typed input file error");
        assert_eq!(typed.kind, std::io::ErrorKind::NotFound);
        let message = error.to_string();
        assert!(message.contains("--file"));
        assert!(
            !message.contains("definitely-missing"),
            "path must not leak"
        );
    }
}
