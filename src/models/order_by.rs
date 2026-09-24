//! Parsed sort specification for Table API reads.
//!
//! The Table API has no sort parameter of its own: ordering is expressed only
//! as `ORDERBY<field>` / `ORDERBYDESC<field>` clauses inside `sysparm_query`.
//! [`OrderBy`] validates a user-facing sort spec once and renders those
//! clauses, so an unsupported parameter can never be sent and silently ignored.

use std::fmt;
use std::str::FromStr;

use super::identifiers::IdentifierError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortDirection {
    Asc,
    Desc,
}

/// One validated `field` + direction pair.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SortKey {
    field: String,
    direction: SortDirection,
}

/// A validated, non-empty list of sort keys.
///
/// Accepted forms, comma-separated and applied in order:
/// - `field` or `field:asc` sorts ascending
/// - `-field` or `field:desc` sorts descending
/// - `ORDERBYfield` / `ORDERBYDESCfield`, the encoded-query spelling
///
/// Field names may contain ASCII letters, digits, `_`, and `.` (dot-walked
/// references).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBy(Vec<SortKey>);

impl OrderBy {
    /// Render the encoded-query clauses, e.g. `ORDERBYDESCsys_created_on^ORDERBYnumber`.
    pub fn to_encoded_query(&self) -> String {
        self.0
            .iter()
            .map(|key| match key.direction {
                SortDirection::Asc => format!("ORDERBY{}", key.field),
                SortDirection::Desc => format!("ORDERBYDESC{}", key.field),
            })
            .collect::<Vec<_>>()
            .join("^")
    }

    /// Append the sort clauses to an optional encoded query.
    ///
    /// Clauses go after the existing query, including any `ORDERBY` it already
    /// contains, so explicit query ordering keeps precedence and nothing is
    /// silently dropped.
    pub fn apply_to_query(&self, query: Option<&str>) -> String {
        let clauses = self.to_encoded_query();
        match query.map(|q| q.trim().trim_end_matches('^')) {
            Some(q) if !q.is_empty() => format!("{q}^{clauses}"),
            _ => clauses,
        }
    }
}

fn err(message: impl Into<String>) -> IdentifierError {
    IdentifierError::from_message(message)
}

fn parse_key(item: &str) -> Result<SortKey, IdentifierError> {
    let (field, direction) = if let Some(field) = item.strip_prefix("ORDERBYDESC") {
        (field, SortDirection::Desc)
    } else if let Some(field) = item.strip_prefix("ORDERBY") {
        (field, SortDirection::Asc)
    } else if let Some(field) = item.strip_prefix('-') {
        (field, SortDirection::Desc)
    } else if let Some((field, suffix)) = item.rsplit_once(':') {
        let direction = match suffix.to_ascii_lowercase().as_str() {
            "asc" => SortDirection::Asc,
            "desc" => SortDirection::Desc,
            _ => {
                return Err(err(format!(
                    "Invalid sort direction '{suffix}' in '{item}'. Use 'field:asc' or 'field:desc'."
                )));
            }
        };
        (field, direction)
    } else {
        (item, SortDirection::Asc)
    };

    let valid_chars = field
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.');
    if field.is_empty()
        || !valid_chars
        || field.starts_with('.')
        || field.ends_with('.')
        || field.contains("..")
    {
        return Err(err(format!(
            "Invalid sort field '{item}'. Use 'field', '-field' (descending), or 'field:desc'; field names may contain only ASCII letters, digits, '_', and '.'."
        )));
    }

    Ok(SortKey {
        field: field.to_string(),
        direction,
    })
}

impl FromStr for OrderBy {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let keys = value
            .split(',')
            .map(str::trim)
            .map(parse_key)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self(keys))
    }
}

impl TryFrom<String> for OrderBy {
    type Error = IdentifierError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<OrderBy> for String {
    fn from(value: OrderBy) -> Self {
        value.to_string()
    }
}

/// Canonical spec form (`field`, `-field`), which parses back to the same value.
impl fmt::Display for OrderBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, key) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            if key.direction == SortDirection::Desc {
                f.write_str("-")?;
            }
            f.write_str(&key.field)?;
        }
        Ok(())
    }
}

impl serde::Serialize for OrderBy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for OrderBy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clauses(spec: &str) -> String {
        spec.parse::<OrderBy>().unwrap().to_encoded_query()
    }

    #[test]
    fn plain_field_sorts_ascending() {
        assert_eq!(clauses("number"), "ORDERBYnumber");
        assert_eq!(clauses("number:asc"), "ORDERBYnumber");
        assert_eq!(clauses("number:ASC"), "ORDERBYnumber");
    }

    #[test]
    fn hyphen_and_desc_suffix_sort_descending() {
        assert_eq!(clauses("-sys_created_on"), "ORDERBYDESCsys_created_on");
        assert_eq!(clauses("sys_created_on:desc"), "ORDERBYDESCsys_created_on");
    }

    #[test]
    fn encoded_query_spelling_is_accepted() {
        assert_eq!(clauses("ORDERBYname"), "ORDERBYname");
        assert_eq!(clauses("ORDERBYDESCname"), "ORDERBYDESCname");
    }

    #[test]
    fn multiple_fields_keep_their_order() {
        assert_eq!(
            clauses("priority, -sys_created_on,number:desc"),
            "ORDERBYpriority^ORDERBYDESCsys_created_on^ORDERBYDESCnumber"
        );
    }

    #[test]
    fn dot_walked_fields_are_allowed() {
        assert_eq!(clauses("caller_id.name"), "ORDERBYcaller_id.name");
    }

    #[test]
    fn invalid_specs_are_rejected() {
        for spec in [
            "",
            " ",
            "number,",
            "-",
            "number:sideways",
            "num ber",
            "number^active=true",
            "--number",
            ".number",
            "caller_id..name",
        ] {
            assert!(spec.parse::<OrderBy>().is_err(), "accepted {spec:?}");
        }
    }

    #[test]
    fn clauses_are_appended_after_the_query() {
        let order: OrderBy = "-sys_created_on".parse().unwrap();
        assert_eq!(order.apply_to_query(None), "ORDERBYDESCsys_created_on");
        assert_eq!(order.apply_to_query(Some("")), "ORDERBYDESCsys_created_on");
        assert_eq!(
            order.apply_to_query(Some("active=true")),
            "active=true^ORDERBYDESCsys_created_on"
        );
        assert_eq!(
            order.apply_to_query(Some("active=true^")),
            "active=true^ORDERBYDESCsys_created_on"
        );
    }

    #[test]
    fn existing_query_ordering_is_kept_first() {
        let order: OrderBy = "number".parse().unwrap();
        assert_eq!(
            order.apply_to_query(Some("active=true^ORDERBYDESCpriority")),
            "active=true^ORDERBYDESCpriority^ORDERBYnumber"
        );
    }

    #[test]
    fn display_round_trips() {
        let order: OrderBy = "a:asc,b:desc,ORDERBYDESCc".parse().unwrap();
        assert_eq!(order.to_string(), "a,-b,-c");
        assert_eq!(order.to_string().parse::<OrderBy>().unwrap(), order);
    }

    #[test]
    fn serde_uses_spec_string() {
        let order: OrderBy = serde_json::from_str("\"-number\"").unwrap();
        assert_eq!(order.to_encoded_query(), "ORDERBYDESCnumber");
        assert_eq!(serde_json::to_string(&order).unwrap(), "\"-number\"");
        assert!(serde_json::from_str::<OrderBy>("\"bad field\"").is_err());
    }
}
