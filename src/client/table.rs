use std::collections::HashSet;

use super::{SnowClient, pagination};
use crate::models::display_value::DisplayValue;
use crate::models::validation::ValidationError;

/// Optional Table API read modifiers. The default sends none of these parameters.
#[derive(Debug, Clone, Copy, Default)]
pub struct TableReadOptions {
    /// Request `sysparm_query_no_domain=true` (authorized by ServiceNow).
    pub query_no_domain: bool,
    /// Request display values; `None` leaves ServiceNow's default (stored
    /// values) untouched.
    pub display_value: Option<DisplayValue>,
    /// Request `sysparm_exclude_reference_link=true` so reference fields return
    /// plain strings instead of `{"link": ..., "value": ...}` objects.
    pub exclude_reference_link: bool,
}

impl TableReadOptions {
    /// Table API `sysparm_*` query parameters enabled by these options.
    pub fn query_params(&self) -> Vec<(&'static str, &'static str)> {
        let mut params = Vec::new();
        if self.query_no_domain {
            params.push(("sysparm_query_no_domain", "true"));
        }
        if let Some(mode) = self.display_value {
            params.push(("sysparm_display_value", mode.sysparm_value()));
        }
        if self.exclude_reference_link {
            params.push(("sysparm_exclude_reference_link", "true"));
        }
        params
    }
}

/// Summary returned by streaming page-by-page table reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TablePageSummary {
    pub offset: usize,
    pub returned: usize,
    pub total: Option<usize>,
    pub truncated: bool,
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
        let mut records = Vec::new();
        let summary = self
            .for_each_table_records_with_options(
                table,
                query,
                fields,
                pagination,
                order_by,
                options,
                |page| {
                    records.extend(page);
                    Ok(())
                },
            )
            .await?;
        Ok(pagination::TableListResult {
            records,
            offset: summary.offset,
            total: summary.total,
            truncated: summary.truncated,
            fields_truncated: false,
        })
    }

    /// Stream Table API records page-by-page to `on_page` without buffering
    /// every page in memory.
    #[allow(clippy::too_many_arguments)]
    pub async fn for_each_table_records_with_options<F>(
        &mut self,
        table: &crate::models::identifiers::TableName,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&crate::models::order_by::OrderBy>,
        options: &TableReadOptions,
        on_page: F,
    ) -> anyhow::Result<TablePageSummary>
    where
        F: FnMut(Vec<crate::models::record::Record>) -> anyhow::Result<()>,
    {
        let path = format!("/api/now/table/{table}");
        let ordered_query = order_by.map(|order| order.apply_to_query(query));
        let query = ordered_query.as_deref().or(query);
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(q) = query {
            params.push(("sysparm_query", q));
        }
        if let Some(f) = fields {
            params.push(("sysparm_fields", f));
        }
        params.extend(options.query_params());
        self.for_each_paged_records(&path, &params, pagination, on_page)
            .await
    }

    /// Fetch `sysparm_limit`/`sysparm_offset`-paged records from any
    /// ServiceNow list endpoint that returns `{"result": [...]}` and reports
    /// `X-Total-Count` and/or RFC 8288 `Link` headers (the Table and Attachment APIs).
    ///
    /// `params` are sent on the initial page after the paging parameters, and on
    /// subsequent offset-paged requests when the server omits `Link` headers.
    pub async fn get_paged_records(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
        pagination: &pagination::PaginationConfig,
    ) -> anyhow::Result<pagination::TableListResult> {
        let mut all_records = Vec::new();
        let summary = self
            .for_each_paged_records(path, params, pagination, |page| {
                all_records.extend(page);
                Ok(())
            })
            .await?;
        Ok(pagination::TableListResult {
            records: all_records,
            offset: summary.offset,
            total: summary.total,
            truncated: summary.truncated,
            fields_truncated: false,
        })
    }

    /// Stream `sysparm_limit`/`sysparm_offset`-paged records page-by-page to
    /// `on_page`, enforcing the same pagination, `Link rel="next"`, and
    /// truncation semantics as [`Self::get_paged_records`].
    pub async fn for_each_paged_records<F>(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
        pagination: &pagination::PaginationConfig,
        mut on_page: F,
    ) -> anyhow::Result<TablePageSummary>
    where
        F: FnMut(Vec<crate::models::record::Record>) -> anyhow::Result<()>,
    {
        if pagination.page_size == 0 {
            return Err(ValidationError::new("Pagination page_size must be at least 1").into());
        }
        if pagination.limit == Some(0) {
            return Ok(TablePageSummary {
                offset: pagination.offset,
                returned: 0,
                total: None,
                truncated: false,
            });
        }

        let mut returned = 0usize;
        let mut offset = pagination.offset;
        let page_size = pagination.page_size;
        let limit = pagination.limit;
        let mut total: Option<usize> = None;
        // Set when we stop at the limit and the server may hold more rows.
        let mut maybe_more = false;
        let mut server_exhausted = false;
        let mut next_link_url: Option<String> = None;
        let mut visited_next_urls: HashSet<String> = HashSet::new();

        loop {
            // Never request more records than the remaining limit allows.
            let request_size = match limit {
                Some(lim) => page_size.min(lim.saturating_sub(returned)),
                None => page_size,
            };

            let response = if let Some(next_url) = next_link_url.take() {
                self.get(&next_url).await?
            } else {
                let request_size_param = request_size.to_string();
                let offset_param = offset.to_string();
                let mut param_refs: Vec<(&str, &str)> = vec![
                    ("sysparm_limit", request_size_param.as_str()),
                    ("sysparm_offset", offset_param.as_str()),
                ];
                param_refs.extend_from_slice(params);
                self.get_with_params(path, &param_refs).await?
            };

            if total.is_none() {
                total = response
                    .headers()
                    .get("X-Total-Count")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse().ok());
            }
            let link_info = parse_link_header(response.headers());
            let body = response.text().await?;
            tracing::debug!(body_len = body.len(), "Parsing JSON response");
            let mut page: crate::models::record::TableResponse = serde_json::from_str(&body)
                .map_err(|_| super::error::TransportError::InvalidResponse)?;

            let count = page.result.len();
            let hit_limit = if let Some(lim) = limit {
                let remaining = lim.saturating_sub(returned);
                if count >= remaining {
                    maybe_more = count > remaining
                        || link_info.next_url.is_some()
                        || (!link_info.has_any_link && count == request_size);
                    page.result.truncate(remaining);
                    true
                } else {
                    false
                }
            } else {
                false
            };

            let delivered = page.result.len();
            returned = returned.saturating_add(delivered);
            tracing::debug!(
                path = %path,
                offset = offset,
                fetched = count,
                total_so_far = returned,
                has_next_link = link_info.next_url.is_some(),
                "Fetched page"
            );

            if delivered > 0 {
                on_page(page.result)?;
            }

            if hit_limit {
                break;
            }

            // Follow server-provided Link rel="next" when present, even if
            // count < request_size (ACL-filtered or server-capped pages).
            if let Some(next_href) = link_info.next_url {
                let validated = self.authenticated_url(&next_href)?;
                if !visited_next_urls.insert(validated.clone()) {
                    tracing::warn!(
                        "Table pagination received duplicate Link rel=\"next\" URL; stopping"
                    );
                    break;
                }
                next_link_url = Some(validated);
                continue;
            }
            if link_info.has_any_link {
                // Server emitted Link headers without rel="next": definitive final page.
                server_exhausted = true;
                break;
            }

            if count == 0 {
                server_exhausted = true;
                break;
            }
            if count < request_size {
                break;
            }

            offset = offset
                .checked_add(count)
                .ok_or_else(|| anyhow::anyhow!("Table pagination offset overflow"))?;
        }

        let truncated = if server_exhausted {
            false
        } else {
            match total {
                Some(t) => {
                    if limit.is_some() {
                        returned > 0 && pagination.offset.saturating_add(returned) < t
                    } else {
                        maybe_more
                    }
                }
                None => maybe_more,
            }
        };

        Ok(TablePageSummary {
            offset: pagination.offset,
            returned,
            total,
            truncated,
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

#[derive(Debug, Default)]
struct ParsedLinkHeader {
    has_any_link: bool,
    next_url: Option<String>,
}

/// Split a `Link` header value on commas outside `<...>` angle brackets and `"..."` quotes.
fn split_link_entries(header: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0;
    let mut in_angles = false;
    let mut in_quotes = false;
    let mut escaped = false;

    for (idx, ch) in header.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            }
            continue;
        }
        match ch {
            '<' => in_angles = true,
            '>' => in_angles = false,
            '"' if !in_angles => in_quotes = true,
            ',' if !in_angles => {
                entries.push(&header[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    if start <= header.len() {
        entries.push(&header[start..]);
    }
    entries
}

/// Parse RFC 8288 `Link` headers to detect whether any link header was present
/// and extract the `rel="next"` target URL when provided.
fn parse_link_header(headers: &http::HeaderMap) -> ParsedLinkHeader {
    let mut parsed = ParsedLinkHeader::default();
    for header_value in headers.get_all(http::header::LINK) {
        let Ok(raw) = header_value.to_str() else {
            continue;
        };
        for entry in split_link_entries(raw) {
            let trimmed = entry.trim();
            let Some(rest) = trimmed.strip_prefix('<') else {
                continue;
            };
            let Some((href, params)) = rest.split_once('>') else {
                continue;
            };
            let href = href.trim();
            if href.is_empty() {
                continue;
            }
            parsed.has_any_link = true;
            let is_next = params.split(';').skip(1).any(|param| {
                let Some((key, value)) = param.split_once('=') else {
                    return false;
                };
                if !key.trim().eq_ignore_ascii_case("rel") {
                    return false;
                }
                let value = value.trim().trim_matches('"');
                value
                    .split_whitespace()
                    .any(|rel| rel.eq_ignore_ascii_case("next"))
            });
            if is_next && parsed.next_url.is_none() {
                parsed.next_url = Some(href.to_string());
            }
        }
    }
    parsed
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
            exclude_reference_link: true,
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
    async fn test_get_table_records_with_options_excludes_reference_link_independently() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param_is_missing("sysparm_display_value"))
            .and(query_param("sysparm_exclude_reference_link", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"assigned_to": "62826bf03710200044e0bfc8bcbe5df1"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let options = TableReadOptions {
            exclude_reference_link: true,
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

    #[tokio::test]
    async fn test_get_paged_records_rejects_zero_page_size() {
        let mut client = test_client("https://test.service-now.com", MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(0);
        let err = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .expect_err("page_size=0 must return an error instead of looping");
        assert!(err.to_string().contains("page_size"));
    }

    #[tokio::test]
    async fn test_get_paged_records_limit_zero_short_circuits_without_network_io() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"sys_id": "1"}]
            })))
            .expect(0)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_limit(Some(0));
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();
        assert!(result.records.is_empty());
        assert_eq!(result.total, None);
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_paged_records_follows_link_next_across_acl_filtered_short_pages() {
        let server = MockServer::start().await;

        // Page 1 requested 10 rows, but row-level ACLs left only 1 visible row.
        // Link rel="next" (including a comma inside sysparm_fields) points to offset=10.
        let next_url = format!(
            "{}/api/now/table/incident?sysparm_limit=10&sysparm_offset=10&sysparm_fields=sys_id,number",
            server.uri()
        );
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "15")
                    .insert_header(
                        "Link",
                        format!(
                            "<{next_url}>;rel=\"next\", <{}/api/now/table/incident?sysparm_offset=0>;rel=\"first\"",
                            server.uri()
                        ),
                    )
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "1", "number": "INC001"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        // Page 2 is the final page (Link has rel="prev" only, no rel="next").
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_offset", "10"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "15")
                    .insert_header(
                        "Link",
                        format!(
                            "<{}/api/now/table/incident?sysparm_offset=0>;rel=\"prev\"",
                            server.uri()
                        ),
                    )
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "2", "number": "INC002"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(10);
        let result = client
            .get_table_records_with_meta(
                &incident_table(),
                None,
                Some("sys_id,number"),
                &pagination,
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.returned(), 2);
        assert_eq!(result.records[0].sys_id(), Some("1"));
        assert_eq!(result.records[1].sys_id(), Some("2"));
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_paged_records_continues_on_server_capped_pages_via_link_next() {
        let server = MockServer::start().await;

        // Client asks for page_size=10; server caps page size to 2 (glide.rest.max_limit)
        // and emits Link rel="next" for offset=2.
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "10"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "3")
                    .insert_header(
                        "Link",
                        format!(
                            "<{}/api/now/table/incident?sysparm_limit=2&sysparm_offset=2>;rel=\"next\"",
                            server.uri()
                        ),
                    )
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "1"}, {"sys_id": "2"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "3")
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "3"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(10);
        let result = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(result.returned(), 3);
        assert_eq!(result.records[2].sys_id(), Some("3"));
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn test_get_paged_records_rejects_cross_origin_link_next() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        "Link",
                        "<https://attacker.example/api/now/table/incident?sysparm_offset=10>;rel=\"next\"",
                    )
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "1"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(10);
        let err = client
            .get_table_records_with_meta(&incident_table(), None, None, &pagination, None)
            .await
            .expect_err("cross-origin Link rel=\"next\" must be refused");
        assert!(err.to_string().contains("non-instance URL"), "{err}");
    }

    #[tokio::test]
    async fn test_for_each_paged_records_streams_pages_incrementally() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "3")
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "1"}, {"sys_id": "2"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "3")
                    .set_body_json(serde_json::json!({
                        "result": [{"sys_id": "3"}]
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(2);
        let mut page_sizes = Vec::new();
        let mut seen_ids = Vec::new();
        let summary = client
            .for_each_table_records_with_options(
                &incident_table(),
                None,
                None,
                &pagination,
                None,
                &TableReadOptions::default(),
                |page| {
                    page_sizes.push(page.len());
                    seen_ids.extend(
                        page.into_iter()
                            .filter_map(|r| r.sys_id().map(ToOwned::to_owned)),
                    );
                    Ok(())
                },
            )
            .await
            .unwrap();

        assert_eq!(page_sizes, vec![2, 1]);
        assert_eq!(seen_ids, vec!["1", "2", "3"]);
        assert_eq!(summary.returned, 3);
        assert_eq!(summary.total, Some(3));
        assert!(!summary.truncated);
    }
}
