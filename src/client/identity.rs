use super::{SnowClient, pagination};
use crate::models::identifiers::TableName;

/// `sys_properties` names that carry the instance build, most specific first.
/// Which of these exist and are readable varies by release and role, so the
/// lookup is best-effort.
const BUILD_PROPERTIES: [&str; 4] = [
    "glide.buildtag",
    "glide.buildtag.last",
    "glide.war",
    "glide.war.assigned",
];

/// The user the instance authenticated this client as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser {
    pub user_name: String,
    pub sys_id: String,
}

impl SnowClient {
    /// Resolve the authenticated user with a single one-row Table API read.
    ///
    /// Uses `sys_id=javascript:gs.getUserID()`, which the query sandbox allows
    /// for every role, so the request both proves the credentials and names
    /// the session user. Returns `Ok(None)` when the request succeeds but the
    /// user row is not readable.
    pub async fn current_user(&mut self) -> anyhow::Result<Option<CurrentUser>> {
        let sys_user = TableName::from_static("sys_user");
        let pagination = pagination::PaginationConfig::default()
            .with_page_size(1)
            .with_limit(Some(1));
        let records = self
            .get_table_records(
                &sys_user,
                Some("sys_id=javascript:gs.getUserID()"),
                Some("sys_id,user_name"),
                &pagination,
                None,
            )
            .await?;

        Ok(records.into_iter().next().and_then(|record| {
            Some(CurrentUser {
                user_name: record.get_str("user_name")?.to_string(),
                sys_id: record.sys_id()?.to_string(),
            })
        }))
    }

    /// Read the instance build tag from `sys_properties`.
    ///
    /// Returns `Ok(None)` when none of the build properties is readable, which
    /// is common for non-admin users.
    pub async fn build_tag(&mut self) -> anyhow::Result<Option<String>> {
        let sys_properties = TableName::from_static("sys_properties");
        let pagination =
            pagination::PaginationConfig::default().with_limit(Some(BUILD_PROPERTIES.len()));
        let query = format!("nameIN{}", BUILD_PROPERTIES.join(","));
        let records = self
            .get_table_records(
                &sys_properties,
                Some(&query),
                Some("name,value"),
                &pagination,
                None,
            )
            .await?;

        Ok(BUILD_PROPERTIES.iter().find_map(|property| {
            records
                .iter()
                .find(|record| record.get_str("name") == Some(property))
                .and_then(|record| record.get_str("value"))
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::client::test_support::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn current_user_reads_session_user() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/sys_user"))
            .and(query_param(
                "sysparm_query",
                "sys_id=javascript:gs.getUserID()",
            ))
            .and(query_param("sysparm_limit", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "user_name": "admin"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let user = client.current_user().await.unwrap().unwrap();

        assert_eq!(user.user_name, "admin");
        assert_eq!(user.sys_id, "6816f79cc0a8016401c5a33be04be441");
    }

    #[tokio::test]
    async fn current_user_is_none_when_row_is_not_readable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/sys_user"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})),
            )
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        assert_eq!(client.current_user().await.unwrap(), None);
    }

    #[tokio::test]
    async fn build_tag_prefers_most_specific_property() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/sys_properties"))
            .and(query_param(
                "sysparm_query",
                "nameINglide.buildtag,glide.buildtag.last,glide.war,glide.war.assigned",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"name": "glide.war", "value": "glide-xanadu.zip"},
                    {"name": "glide.buildtag.last", "value": "glide-xanadu-07-02-2024__patch3"},
                    {"name": "glide.buildtag", "value": ""}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        assert_eq!(
            client.build_tag().await.unwrap().as_deref(),
            Some("glide-xanadu-07-02-2024__patch3")
        );
    }
}
