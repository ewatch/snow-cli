use super::{SnowClient, pagination};

impl SnowClient {
    /// Fetch paginated records from the Table API.
    ///
    /// Automatically follows pagination using `sysparm_offset` and `sysparm_limit`
    /// until all records are fetched or the configured limit is reached.
    pub async fn get_table_records(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&str>,
    ) -> anyhow::Result<Vec<crate::models::record::Record>> {
        Ok(self
            .get_table_records_with_meta(table, query, fields, pagination, order_by)
            .await?
            .records)
    }

    /// Fetch paginated records from the Table API, preserving result metadata.
    ///
    /// Like [`Self::get_table_records`], but also captures `X-Total-Count`
    /// from the first response and reports whether the returned records are
    /// a truncated subset of all matching rows.
    pub async fn get_table_records_with_meta(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&str>,
    ) -> anyhow::Result<pagination::TableListResult> {
        let path = format!("/api/now/table/{table}");
        let mut all_records = Vec::new();
        let mut offset: usize = 0;
        let page_size = pagination.page_size;
        let limit = pagination.limit;
        let mut total: Option<usize> = None;
        // Set when we stop at the limit and the server may hold more rows.
        let mut maybe_more = false;

        loop {
            // Never request more records than the remaining limit allows.
            let request_size = match limit {
                Some(lim) => page_size.min(lim.saturating_sub(all_records.len())),
                None => page_size,
            };

            let mut params: Vec<(&str, String)> = vec![
                ("sysparm_limit", request_size.to_string()),
                ("sysparm_offset", offset.to_string()),
            ];

            if let Some(q) = query {
                params.push(("sysparm_query", q.to_string()));
            }
            if let Some(f) = fields {
                params.push(("sysparm_fields", f.to_string()));
            }
            if let Some(o) = order_by {
                params.push(("sysparm_orderby", o.to_string()));
            }

            // Convert to &str pairs for the request
            let param_refs: Vec<(&str, &str)> =
                params.iter().map(|(k, v)| (*k, v.as_str())).collect();

            let response = self.get_with_params(&path, &param_refs).await?;
            if total.is_none() {
                total = response
                    .headers()
                    .get("X-Total-Count")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse().ok());
            }
            let body = response.text().await?;
            tracing::debug!(body_len = body.len(), "Parsing JSON response");
            let page: crate::models::record::TableResponse = serde_json::from_str(&body)?;

            let count = page.result.len();
            tracing::debug!(
                table = %table,
                offset = offset,
                fetched = count,
                total_so_far = all_records.len() + count,
                "Fetched page"
            );

            all_records.extend(page.result);

            // Check if we've reached the configured limit
            if let Some(lim) = limit
                && all_records.len() >= lim
            {
                maybe_more = all_records.len() > lim || count == request_size;
                all_records.truncate(lim);
                break;
            }

            // If we got fewer records than requested, we've fetched everything
            if count < request_size {
                break;
            }

            offset += request_size;
        }

        let truncated = match total {
            Some(t) => all_records.len() < t,
            None => maybe_more,
        };

        Ok(pagination::TableListResult {
            records: all_records,
            total,
            truncated,
            fields_truncated: false,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_get_table_records_single_page() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "100"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1", "number": "INC001"},
                    {"sys_id": "2", "number": "INC002"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let records = client
            .get_table_records(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sys_id(), Some("1"));
    }

    #[tokio::test]
    async fn test_get_table_records_with_query_and_fields() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_query", "active=true"))
            .and(query_param("sysparm_fields", "sys_id,number"))
            .and(query_param("sysparm_orderby", "number"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"sys_id": "1", "number": "INC001"}]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let records = client
            .get_table_records(
                &incident_table(),
                Some("active=true"),
                Some("sys_id,number"),
                &pagination,
                Some("number"),
            )
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
    }

    #[tokio::test]
    async fn test_get_table_records_pagination_multiple_pages() {
        let server = MockServer::start().await;

        // Page 1: 2 records (page_size = 2, so fetches next page)
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1", "number": "INC001"},
                    {"sys_id": "2", "number": "INC002"}
                ]
            })))
            .mount(&server)
            .await;

        // Page 2: 1 record (less than page_size, stops)
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "3", "number": "INC003"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(2);
        let records = client
            .get_table_records(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].sys_id(), Some("1"));
        assert_eq!(records[2].sys_id(), Some("3"));
    }

    #[tokio::test]
    async fn test_get_table_records_respects_limit() {
        let server = MockServer::start().await;

        // Returns 3 records per page, but we limit to 2
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1"},
                    {"sys_id": "2"},
                    {"sys_id": "3"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default()
            .with_page_size(10)
            .with_limit(Some(2));

        let records = client
            .get_table_records(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sys_id(), Some("1"));
        assert_eq!(records[1].sys_id(), Some("2"));
    }

    #[tokio::test]
    async fn test_get_table_records_empty_result() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": []
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let records = client
            .get_table_records(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_complete_fetch() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "2")
                    .set_body_json(serde_json::json!({
                        "result": [
                            {"sys_id": "1", "number": "INC001"},
                            {"sys_id": "2", "number": "INC002"}
                        ]
                    })),
            )
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 2);
        assert_eq!(result.total, Some(2));
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_limit_truncation() {
        let server = MockServer::start().await;

        // Limit 2 caps the request size, so only 2 records are requested.
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "4381")
                    .set_body_json(serde_json::json!({
                        "result": [
                            {"sys_id": "1"},
                            {"sys_id": "2"}
                        ]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_limit(Some(2));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 2);
        assert_eq!(result.total, Some(4381));
        assert!(result.truncated);
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_limit_equals_total_not_truncated() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "2")
                    .set_body_json(serde_json::json!({
                        "result": [
                            {"sys_id": "1"},
                            {"sys_id": "2"}
                        ]
                    })),
            )
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_limit(Some(2));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 2);
        assert_eq!(result.total, Some(2));
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_missing_total_header() {
        let server = MockServer::start().await;

        // No X-Total-Count. A limit-bounded fetch whose page came back full
        // is conservatively reported as truncated; total stays unknown.
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1"},
                    {"sys_id": "2"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_limit(Some(2));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 2);
        assert_eq!(result.total, None);
        assert!(result.truncated);
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_missing_total_complete_fetch() {
        let server = MockServer::start().await;

        // No X-Total-Count, but the page came back below the requested size,
        // so the fetch is provably complete.
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 1);
        assert_eq!(result.total, None);
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_empty_result_is_definitive() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "0")
                    .set_body_json(serde_json::json!({ "result": [] })),
            )
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_limit(Some(20));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 0);
        assert_eq!(result.total, Some(0));
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_table_records_with_meta_limit_caps_request_size() {
        let server = MockServer::start().await;

        // limit 3 with page_size 2: pages request 2 then 1 record.
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "5")
                    .set_body_json(serde_json::json!({
                        "result": [
                            {"sys_id": "1"},
                            {"sys_id": "2"}
                        ]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "1"))
            .and(query_param("sysparm_offset", "2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "5")
                    .set_body_json(serde_json::json!({
                        "result": [
                            {"sys_id": "3"}
                        ]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default()
            .with_page_size(2)
            .with_limit(Some(3));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 3);
        assert_eq!(result.total, Some(5));
        assert!(result.truncated);
        assert_eq!(result.records[2].sys_id(), Some("3"));
    }
}
