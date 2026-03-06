use std::io::IsTerminal;

use crate::cli::args::{ApiArgs, ApiCommands, OutputFormat};

pub async fn handle(
    args: ApiArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        ApiCommands::Get { path, header } => {
            tracing::info!("API GET {}", path);

            let extra_headers = parse_headers(&header)?;
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let response = if extra_headers.is_empty() {
                client.get(&path).await?
            } else {
                client
                    .request_with_headers(reqwest::Method::GET, &path, None, &[], &extra_headers)
                    .await?
            };

            print_response(response, format).await
        }
        ApiCommands::Post { path, data, header } => {
            tracing::info!("API POST {}", path);

            let body = read_data(data)?;
            let extra_headers = parse_headers(&header)?;
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let response = if extra_headers.is_empty() {
                client.post(&path, &body).await?
            } else {
                client
                    .request_with_headers(
                        reqwest::Method::POST,
                        &path,
                        Some(&body),
                        &[],
                        &extra_headers,
                    )
                    .await?
            };

            print_response(response, format).await
        }
        ApiCommands::Put { path, data, header } => {
            tracing::info!("API PUT {}", path);

            let body = read_data(data)?;
            let extra_headers = parse_headers(&header)?;
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let response = if extra_headers.is_empty() {
                client.put(&path, &body).await?
            } else {
                client
                    .request_with_headers(
                        reqwest::Method::PUT,
                        &path,
                        Some(&body),
                        &[],
                        &extra_headers,
                    )
                    .await?
            };

            print_response(response, format).await
        }
        ApiCommands::Delete { path, header } => {
            tracing::info!("API DELETE {}", path);

            let extra_headers = parse_headers(&header)?;
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let response = if extra_headers.is_empty() {
                client.delete(&path).await?
            } else {
                client
                    .request_with_headers(reqwest::Method::DELETE, &path, None, &[], &extra_headers)
                    .await?
            };

            print_response(response, format).await
        }
    }
}

/// Parse custom headers from `key:value` format.
fn parse_headers(headers: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    headers
        .iter()
        .map(|h| {
            let (key, value) = h.split_once(':').ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid header format '{}'. Use 'Key: Value', for example: -H 'X-Trace-Id: abc123'.",
                    h
                )
            })?;
            Ok((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Read data from `--data` flag or stdin (same pattern as table commands).
fn read_data(data: Option<String>) -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    read_data_from(data, stdin.lock(), stdin.is_terminal())
}

fn read_data_from<R: std::io::Read>(
    data: Option<String>,
    mut reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    if let Some(d) = data {
        return Ok(d);
    }

    if is_tty {
        anyhow::bail!(
            "No data provided. Use --data or pipe a request body. Examples: \
             snow-cli api post /api/x_myapp/action --data '{{\"dry_run\":true}}' \
             | echo '{{\"dry_run\":true}}' | snow-cli api post /api/x_myapp/action"
        );
    }

    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;

    if buf.trim().is_empty() {
        anyhow::bail!(
            "No data received from stdin. Pipe a non-empty body, for example: \
             echo '{{\"dry_run\":true}}' | snow-cli api post /api/x_myapp/action"
        );
    }

    Ok(buf)
}

/// Print the raw HTTP response body to stdout.
///
/// For JSON format: pretty-prints if the body is valid JSON, otherwise raw.
/// For CSV format: prints raw body as-is (API responses may not be tabular).
async fn print_response(response: reqwest::Response, format: &OutputFormat) -> anyhow::Result<()> {
    let status = response.status();
    let body = response.text().await?;

    tracing::debug!(status = status.as_u16(), body_len = body.len(), "Response");

    match format {
        OutputFormat::Json => {
            // Try to pretty-print as JSON; if it's not valid JSON, output raw
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
                Err(_) => println!("{}", body),
            }
        }
        OutputFormat::Csv => {
            // Raw API responses aren't necessarily tabular; output as-is
            println!("{}", body);
        }
        OutputFormat::Text => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
            Err(_) => println!("{}", body),
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_headers_valid() {
        let headers = vec![
            "Content-Type: application/xml".to_string(),
            "X-Custom:value".to_string(),
        ];
        let parsed = parse_headers(&headers).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "Content-Type");
        assert_eq!(parsed[0].1, "application/xml");
        assert_eq!(parsed[1].0, "X-Custom");
        assert_eq!(parsed[1].1, "value");
    }

    #[test]
    fn test_parse_headers_trims_whitespace() {
        let headers = vec!["  Key  :  Value  ".to_string()];
        let parsed = parse_headers(&headers).unwrap();
        assert_eq!(parsed[0].0, "Key");
        assert_eq!(parsed[0].1, "Value");
    }

    #[test]
    fn test_parse_headers_invalid_format() {
        let headers = vec!["no-colon-here".to_string()];
        let result = parse_headers(&headers);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid header"));
    }

    #[test]
    fn test_parse_headers_empty() {
        let headers: Vec<String> = vec![];
        let parsed = parse_headers(&headers).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_parse_headers_with_colon_in_value() {
        // Values like URLs contain colons
        let headers = vec!["Location: https://example.com:8080/path".to_string()];
        let parsed = parse_headers(&headers).unwrap();
        assert_eq!(parsed[0].0, "Location");
        assert_eq!(parsed[0].1, "https://example.com:8080/path");
    }

    #[test]
    fn test_read_data_from_flag() {
        let data = read_data_from(
            Some(r#"{"key":"value"}"#.to_string()),
            Cursor::new(b""),
            false,
        )
        .unwrap();
        assert_eq!(data, r#"{"key":"value"}"#);
    }

    #[test]
    fn test_read_data_from_stdin() {
        let data = read_data_from(None, Cursor::new(b"raw body content"), false).unwrap();
        assert_eq!(data, "raw body content");
    }

    #[test]
    fn test_read_data_from_tty_errors() {
        let result = read_data_from(None, Cursor::new(b""), true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No data provided"));
    }
}
