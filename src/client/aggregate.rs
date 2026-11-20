//! Aggregate API (`GET /api/now/stats/{table}`) requests and envelope checks.

use super::{SnowClient, error::TransportError};

/// The `result` of an Aggregate API response.
///
/// ServiceNow returns `result` as an object for ungrouped queries and as an
/// array (one entry per group) for `sysparm_group_by` queries. Entries keep
/// their raw JSON shape; callers decide how to interpret `stats` and
/// `groupby_fields`.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregateResult {
    /// Ungrouped query: a single `{"stats": ...}` object.
    Single(serde_json::Value),
    /// Grouped query: one `{"stats": ..., "groupby_fields": [...]}` per group.
    Grouped(Vec<serde_json::Value>),
}

impl AggregateResult {
    /// Validate an Aggregate API response body's envelope.
    pub fn from_envelope(mut response: serde_json::Value) -> Result<Self, TransportError> {
        match response.get_mut("result").map(serde_json::Value::take) {
            Some(single @ serde_json::Value::Object(_)) => Ok(Self::Single(single)),
            Some(serde_json::Value::Array(groups)) => Ok(Self::Grouped(groups)),
            _ => Err(TransportError::InvalidEnvelope(
                "Unexpected Aggregate API response: missing 'result' object or array",
            )),
        }
    }
}

impl SnowClient {
    /// Query the Aggregate API for `table` with caller-supplied `sysparm_*`
    /// parameters, sent in the given order.
    pub async fn aggregate(
        &mut self,
        table: &crate::models::identifiers::TableName,
        params: &[(&str, &str)],
    ) -> anyhow::Result<AggregateResult> {
        let response: serde_json::Value = self
            .get_json_with_params(&format!("/api/now/stats/{table}"), params)
            .await?;
        Ok(AggregateResult::from_envelope(response)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn envelope_distinguishes_single_and_grouped_results() {
        assert_eq!(
            AggregateResult::from_envelope(
                serde_json::json!({"result": {"stats": {"count": "4"}}})
            )
            .unwrap(),
            AggregateResult::Single(serde_json::json!({"stats": {"count": "4"}}))
        );
        assert_eq!(
            AggregateResult::from_envelope(serde_json::json!({"result": []})).unwrap(),
            AggregateResult::Grouped(Vec::new())
        );
    }

    #[test]
    fn envelope_rejects_missing_or_scalar_result() {
        for response in [
            serde_json::json!({"error": "boom"}),
            serde_json::json!({"result": "4"}),
            serde_json::json!([]),
        ] {
            let error = AggregateResult::from_envelope(response).unwrap_err();
            assert!(matches!(error, TransportError::InvalidEnvelope(_)));
            assert!(
                error
                    .to_string()
                    .contains("Unexpected Aggregate API response")
            );
        }
    }

    #[tokio::test]
    async fn aggregate_sends_params_to_the_stats_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/stats/incident"))
            .and(query_param("sysparm_count", "true"))
            .and(query_param("sysparm_group_by", "state"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"stats": {"count": "2"}, "groupby_fields": [{"field": "state", "value": "1"}]}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let result = client
            .aggregate(
                &incident_table(),
                &[("sysparm_count", "true"), ("sysparm_group_by", "state")],
            )
            .await
            .unwrap();
        assert!(matches!(result, AggregateResult::Grouped(groups) if groups.len() == 1));
    }
}
