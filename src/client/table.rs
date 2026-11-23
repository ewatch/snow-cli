use super::{SnowClient, pagination};
use crate::models::display_value::DisplayValue;

/// Optional Table API read modifiers. The default sends neither parameter.
#[derive(Debug, Clone, Copy, Default)]
pub struct TableReadOptions {
    /// Request `sysparm_query_no_domain=true` (authorized by ServiceNow).
    pub query_no_domain: bool,
    /// Request display values; `None` leaves ServiceNow's default (raw values
    /// with reference links) untouched.
    pub display_value: Option<DisplayValue>,
}

/// `sysparm_display_value` plus `sysparm_exclude_reference_link=true` for an
/// explicit display-value mode, or nothing when the mode is unset. Reference
/// links are excluded so every mode yields uniform, compact field values.
pub fn display_value_params(mode: Option<DisplayValue>) -> Vec<(&'static str, &'static str)> {
    match mode {
        Some(mode) => vec![
            ("sysparm_display_value", mode.sysparm_value()),
            ("sysparm_exclude_reference_link", "true"),
        ],
        None => Vec::new(),
    }
}

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
        order_by: Option<&crate::models::order_by::OrderBy>,
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
    ///
    /// The Table API has no sort parameter, so `order_by` is appended to
    /// `sysparm_query` as `ORDERBY`/`ORDERBYDESC` clauses.
    pub async fn get_table_records_with_meta(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&crate::models::order_by::OrderBy>,
    ) -> anyhow::Result<pagination::TableListResult> {
        self.get_table_records_with_domain(table, query, fields, pagination, order_by, false)
            .await
    }

    /// Table API read with an explicit domain-visibility bypass request.
    /// ServiceNow authorizes `query_no_domain`; this never changes session state.
    pub async fn get_table_records_with_domain(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&crate::models::order_by::OrderBy>,
        query_no_domain: bool,
    ) -> anyhow::Result<pagination::TableListResult> {
        let options = TableReadOptions {
            query_no_domain,
            ..TableReadOptions::default()
        };
        self.get_table_records_with_options(table, query, fields, pagination, order_by, &options)
            .await
    }

    /// Table API read with optional read modifiers (domain bypass, display
    /// values). With default options the request is identical to
    /// [`Self::get_table_records_with_meta`].
    pub async fn get_table_records_with_options(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&crate::models::order_by::OrderBy>,
        options: &TableReadOptions,
    ) -> anyhow::Result<pagination::TableListResult> {
        let path = format!("/api/now/table/{table}");
        let ordered_query = order_by.map(|order| order.apply_to_query(query));
        let query = ordered_query.as_deref().or(query);
        let mut params: Vec<(&str, &str)> = Vec::new();
        if options.query_no_domain {
            params.push(("sysparm_query_no_domain", "true"));
        }
        if let Some(q) = query {
            params.push(("sysparm_query", q));
        }
        if let Some(f) = fields {
            params.push(("sysparm_fields", f));
        }
        params.extend(display_value_params(options.display_value));
        self.get_paged_records(&path, &params, pagination).await
    }

    /// Fetch `sysparm_limit`/`sysparm_offset`-paged records from any
    /// ServiceNow list endpoint that returns `{"result": [...]}` and reports
    /// `X-Total-Count` (the Table and Attachment APIs).
    ///
    /// `params` are sent on every page after the paging parameters.
    pub async fn get_paged_records(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
        pagination: &pagination::PaginationConfig,
    ) -> anyhow::Result<pagination::TableListResult> {
        let mut all_records = Vec::new();
        let mut offset = pagination.offset;
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
            let request_size_param = request_size.to_string();
            let offset_param = offset.to_string();
            let mut param_refs: Vec<(&str, &str)> = vec![
                ("sysparm_limit", request_size_param.as_str()),
                ("sysparm_offset", offset_param.as_str()),
            ];
            param_refs.extend_from_slice(params);

            let response = self.get_with_params(path, &param_refs).await?;
            if total.is_none() {
                total = response
                    .headers()
                    .get("X-Total-Count")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse().ok());
            }
            let body = response.text().await?;
            tracing::debug!(body_len = body.len(), "Parsing JSON response");
            let page: crate::models::record::TableResponse = serde_json::from_str(&body)
                .map_err(|_| super::error::TransportError::InvalidResponse)?;

            let count = page.result.len();
            tracing::debug!(
                path = %path,
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

            offset = offset
                .checked_add(request_size)
                .ok_or_else(|| anyhow::anyhow!("Table pagination offset overflow"))?;
        }

        let truncated = match total {
            Some(t) => {
                !all_records.is_empty() && pagination.offset.saturating_add(all_records.len()) < t
            }
            None => maybe_more,
        };

        Ok(pagination::TableListResult {
            records: all_records,
            offset: pagination.offset,
            total,
            truncated,
            fields_truncated: false,
        })
    }

    /// Count matching records via the `X-Total-Count` header without
    /// downloading them.
    ///
    /// Issues a single `sysparm_limit=1` request and returns the server's
    /// reported total, or `None` when the instance omits the header. This is
    /// how scope inspection counts artifacts for large scopes (e.g. `global`)
    /// without paginating through every matching row.
    pub async fn count_table_records(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
    ) -> anyhow::Result<Option<usize>> {
        self.count_table_records_with_domain(table, query, false)
            .await
    }

    /// Like [`Self::count_table_records`], optionally requesting
    /// `sysparm_query_no_domain` so the total spans every domain the server
    /// lets this session read across.
    pub async fn count_table_records_with_domain(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        query_no_domain: bool,
    ) -> anyhow::Result<Option<usize>> {
        let config = pagination::PaginationConfig::default()
            .with_page_size(1)
            .with_limit(Some(1));
        Ok(self
            .get_table_records_with_domain(
                table,
                query,
                Some("sys_id"),
                &config,
                None,
                query_no_domain,
            )
            .await?
            .total)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::*;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
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
            .and(query_param("sysparm_query", "active=true^ORDERBYnumber"))
            .and(query_param("sysparm_fields", "sys_id,number"))
            .and(query_param_is_missing("sysparm_orderby"))
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
                Some(&"number".parse().unwrap()),
            )
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
    }

    #[tokio::test]
    async fn test_get_table_records_with_options_sends_display_value_params() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_display_value", "all"))
            .and(query_param("sysparm_exclude_reference_link", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"priority": {"display_value": "4 - Low", "value": "4"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let options = TableReadOptions {
            display_value: Some(DisplayValue::Both),
            ..TableReadOptions::default()
        };
        let result = client
            .get_table_records_with_options(
                &incident_table(),
                None,
                None,
                &pagination::PaginationConfig::default(),
                None,
                &options,
            )
            .await
            .unwrap();
        assert_eq!(result.returned(), 1);
    }

    #[tokio::test]
    async fn test_default_options_send_no_display_value_params() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param_is_missing("sysparm_display_value"))
            .and(query_param_is_missing("sysparm_exclude_reference_link"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        client
            .get_table_records_with_meta(
                &incident_table(),
                None,
                None,
                &pagination::PaginationConfig::default(),
                None,
            )
            .await
            .unwrap();
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

    #[tokio::test]
    async fn test_get_table_records_with_meta_offset_across_pages() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "6")
                    .set_body_json(serde_json::json!({"result": [
                        {"sys_id": "4"}, {"sys_id": "5"}
                    ]})),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "1"))
            .and(query_param("sysparm_offset", "5"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "6")
                    .set_body_json(serde_json::json!({"result": [{"sys_id": "6"}]})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default()
            .with_offset(3)
            .with_page_size(2)
            .with_limit(Some(3));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 3);
        assert_eq!(result.offset, 3);
        assert_eq!(result.next_offset(), None);
        assert!(!result.truncated);
        assert_eq!(result.records[2].sys_id(), Some("6"));
    }
}
