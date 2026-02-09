use crate::cli::args::{CodesearchArgs, CodesearchCommands, OutputFormat};
use crate::cli::output;

pub async fn handle(
    args: CodesearchArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        CodesearchCommands::Search {
            query,
            source_table,
            limit,
            current_scope,
            search_group,
        } => {
            tracing::info!(query = %query, "Searching code");

            let mut client = crate::client::build_client(profile, instance)?;

            let limit_str = limit.to_string();
            let search_all_scopes = !current_scope;
            let search_all_scopes_str = search_all_scopes.to_string();

            let mut params: Vec<(&str, &str)> = vec![
                ("term", &query),
                ("limit", &limit_str),
                ("search_all_scopes", &search_all_scopes_str),
                ("search_group", &search_group),
            ];

            if let Some(ref t) = source_table {
                params.push(("table", t.as_str()));
            }

            let response = client
                .get_with_params("/api/sn_codesearch/code_search/search", &params)
                .await?;

            let response_body = response.text().await?;

            tracing::debug!(body_len = response_body.len(), "Code search response");

            // Try to parse as JSON with a "result" wrapper (standard SN response)
            // and output as records if possible; otherwise output raw
            match serde_json::from_str::<crate::models::record::TableResponse>(&response_body) {
                Ok(table_resp) => {
                    output::print_records(&table_resp.result, format)?;
                }
                Err(_) => {
                    // Fall back: try to pretty-print as generic JSON
                    match serde_json::from_str::<serde_json::Value>(&response_body) {
                        Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
                        Err(_) => println!("{response_body}"),
                    }
                }
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codesearch_args_construction() {
        let args = CodesearchArgs {
            command: CodesearchCommands::Search {
                query: "GlideRecord".to_string(),
                source_table: Some("sys_script_include".to_string()),
                limit: 500,
                current_scope: false,
                search_group: "sn_devstudio.Studio Search Group".to_string(),
            },
        };
        match args.command {
            CodesearchCommands::Search {
                query,
                source_table,
                limit,
                current_scope,
                search_group,
            } => {
                assert_eq!(query, "GlideRecord");
                assert_eq!(source_table, Some("sys_script_include".to_string()));
                assert_eq!(limit, 500);
                assert!(!current_scope);
                assert_eq!(search_group, "sn_devstudio.Studio Search Group");
            }
        }
    }
}
