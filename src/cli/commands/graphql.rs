use std::io::IsTerminal;

use serde_json::{Map, Value};

use crate::cli::args::{GraphqlArgs, OutputFormat};
use crate::cli::io::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::cli::output;

pub async fn handle(
    args: GraphqlArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    let variables_supplied = args.variables.is_some();
    let variables = parse_variables(args.variables.as_deref())?;
    let document = resolve_document(args).await?;

    tracing::info!(
        document_len = document.len(),
        variables_supplied,
        "Executing GraphQL document"
    );

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let data = client.execute_graphql(&document, &variables).await?;
    output::print_output(&data, format)
}

async fn resolve_document(args: GraphqlArgs) -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    resolve_document_from(args, stdin.lock(), stdin.is_terminal()).await
}

async fn resolve_document_from<R: std::io::Read>(
    args: GraphqlArgs,
    reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    let explicit_source_count = usize::from(args.document.is_some())
        + usize::from(args.query.is_some())
        + usize::from(args.query_file.is_some());
    if explicit_source_count > 1 {
        anyhow::bail!(
            "Provide exactly one GraphQL document source: positional DOCUMENT, --query, or --query-file"
        );
    }

    if let Some(document) = args.document {
        return validate_document(document, "positional document");
    }
    if let Some(query) = args.query {
        return validate_document(query, "--query");
    }
    if let Some(path) = args.query_file {
        let document = tokio::fs::read_to_string(&path).await.map_err(|error| {
            anyhow::anyhow!(
                "Failed to read GraphQL query file '{}': {}",
                path.display(),
                error
            )
        })?;
        return validate_document(document, &format!("query file '{}'", path.display()));
    }

    if is_tty {
        anyhow::bail!(
            "No GraphQL document provided. Use positional DOCUMENT, --query, --query-file, or pipe a document to stdin"
        );
    }

    let document = read_to_string_limited(
        reader,
        DEFAULT_MAX_STDIN_BYTES,
        "GraphQL document from stdin",
    )?;
    validate_document(document, "stdin")
}

fn validate_document(document: String, source: &str) -> anyhow::Result<String> {
    if document.trim().is_empty() {
        anyhow::bail!("GraphQL document from {source} is empty");
    }
    Ok(document)
}

fn parse_variables(variables: Option<&str>) -> anyhow::Result<Map<String, Value>> {
    let Some(variables) = variables else {
        return Ok(Map::new());
    };

    let value: Value = serde_json::from_str(variables)
        .map_err(|error| anyhow::anyhow!("Invalid GraphQL variables JSON: {error}"))?;
    value.as_object().cloned().ok_or_else(|| {
        anyhow::anyhow!("GraphQL variables must be a JSON object mapping names to values")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn args(
        document: Option<&str>,
        query: Option<&str>,
        query_file: Option<std::path::PathBuf>,
        variables: Option<&str>,
    ) -> GraphqlArgs {
        GraphqlArgs {
            document: document.map(ToString::to_string),
            query: query.map(ToString::to_string),
            query_file,
            variables: variables.map(ToString::to_string),
        }
    }

    #[tokio::test]
    async fn resolves_each_document_source() {
        let positional = resolve_document_from(
            args(Some("{ positional }"), None, None, None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap();
        assert_eq!(positional, "{ positional }");

        let query = resolve_document_from(
            args(None, Some("{ flag }"), None, None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap();
        assert_eq!(query, "{ flag }");

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("query.graphql");
        tokio::fs::write(&path, "{ file }").await.unwrap();
        let file = resolve_document_from(
            args(None, None, Some(path), None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap();
        assert_eq!(file, "{ file }");

        let stdin = resolve_document_from(
            args(None, None, None, None),
            Cursor::new("{ stdin }"),
            false,
        )
        .await
        .unwrap();
        assert_eq!(stdin, "{ stdin }");
    }

    #[tokio::test]
    async fn rejects_missing_blank_or_unreadable_document_sources() {
        let tty_error = resolve_document_from(
            args(None, None, None, None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(tty_error.contains("No GraphQL document provided"));

        let blank_error =
            resolve_document_from(args(None, None, None, None), Cursor::new("  \n"), false)
                .await
                .unwrap_err()
                .to_string();
        assert!(blank_error.contains("stdin is empty"));

        let blank_query_error = resolve_document_from(
            args(None, Some(" \n"), None, None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(blank_query_error.contains("--query is empty"));

        let directory = tempfile::tempdir().unwrap();
        let blank_path = directory.path().join("blank.graphql");
        tokio::fs::write(&blank_path, " \n").await.unwrap();
        let blank_file_error = resolve_document_from(
            args(None, None, Some(blank_path), None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(blank_file_error.contains("blank.graphql' is empty"));

        let missing_path = std::path::PathBuf::from("missing-graphql-query-file.graphql");
        let file_error = resolve_document_from(
            args(None, None, Some(missing_path), None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(file_error.contains("Failed to read GraphQL query file"));
    }

    #[tokio::test]
    async fn rejects_multiple_or_oversized_sources() {
        let multiple = resolve_document_from(
            args(Some("{ a }"), Some("{ b }"), None, None),
            Cursor::new(Vec::<u8>::new()),
            true,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(multiple.contains("exactly one"));

        let oversized = vec![b'x'; DEFAULT_MAX_STDIN_BYTES as usize + 1];
        let size_error =
            resolve_document_from(args(None, None, None, None), Cursor::new(oversized), false)
                .await
                .unwrap_err()
                .to_string();
        assert!(size_error.contains("exceeds the maximum supported size"));
    }

    #[test]
    fn parses_variables_as_an_object() {
        assert!(parse_variables(None).unwrap().is_empty());

        let variables =
            parse_variables(Some(r#"{"number":"INC0010001","filter":{"active":true}}"#)).unwrap();
        assert_eq!(variables["number"], "INC0010001");
        assert_eq!(variables["filter"]["active"], true);
    }

    #[test]
    fn rejects_malformed_or_non_object_variables() {
        let malformed = parse_variables(Some("{bad json")).unwrap_err().to_string();
        assert!(malformed.contains("Invalid GraphQL variables JSON"));

        let non_object = parse_variables(Some("[1,2,3]")).unwrap_err().to_string();
        assert!(non_object.contains("must be a JSON object"));
    }
}
