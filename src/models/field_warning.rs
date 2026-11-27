//! Non-fatal warnings attached to a successful read result.

use serde::Serialize;

/// Stable code for a field name ServiceNow did not return.
pub const UNKNOWN_FIELD: &str = "UNKNOWN_FIELD";

/// Where a checked field name came from on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldSource {
    Fields,
    Query,
    OrderBy,
    GroupBy,
    Avg,
    Min,
    Max,
    Sum,
    Having,
}

impl FieldSource {
    /// The CLI flag the field was given in.
    pub fn flag(self) -> &'static str {
        match self {
            Self::Fields => "--fields",
            Self::Query => "--query",
            Self::OrderBy => "--order-by",
            Self::GroupBy => "--group-by",
            Self::Avg => "--avg",
            Self::Min => "--min",
            Self::Max => "--max",
            Self::Sum => "--sum",
            Self::Having => "--having",
        }
    }

    /// What ServiceNow does with an unknown field from this source.
    fn consequence(self) -> &'static str {
        match self {
            Self::Fields => "it is omitted from the records",
            Self::Query => "ServiceNow ignores that term, so the result is NOT filtered by it",
            Self::OrderBy => "ServiceNow ignores it, so the result is NOT sorted by it",
            Self::GroupBy => "ServiceNow ignores it, so groups are NOT split by it",
            Self::Avg => "ServiceNow ignores it or omits the average for that field",
            Self::Min => "ServiceNow ignores it or omits the minimum for that field",
            Self::Max => "ServiceNow ignores it or omits the maximum for that field",
            Self::Sum => "ServiceNow ignores it or omits the sum for that field",
            Self::Having => "ServiceNow ignores or rejects the HAVING filter on that field",
        }
    }
}

/// A structured warning: `{"code","field","in","message"}`.
///
/// Messages are authored guidance naming only the field path, never query
/// values (see the ADR-0010 diagnostic boundary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldWarning {
    pub code: &'static str,
    pub field: String,
    #[serde(rename = "in")]
    pub source: FieldSource,
    pub message: String,
}

impl FieldWarning {
    /// Warning for a field path that `table` did not return.
    pub fn unknown_field(table: &str, field: &str, source: FieldSource) -> Self {
        Self {
            code: UNKNOWN_FIELD,
            field: field.to_string(),
            source,
            message: format!(
                "{table} returned no field '{field}' (unknown or not readable) from {}; {}",
                source.flag(),
                source.consequence()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_in_key_and_actionable_message() {
        let warning =
            FieldWarning::unknown_field("incident", "assigned_two.name", FieldSource::Query);
        let json = serde_json::to_value(&warning).unwrap();
        assert_eq!(json["code"], "UNKNOWN_FIELD");
        assert_eq!(json["in"], "query");
        assert_eq!(json["field"], "assigned_two.name");
        let message = json["message"].as_str().unwrap();
        assert!(message.contains("NOT filtered"), "{message}");
        assert!(!message.contains("query="), "{message}");
    }
}
