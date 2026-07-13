//! Parse-don't-validate identifier newtypes.
//!
//! Each type wraps a `String` that has already been checked against the
//! ServiceNow identifier rules it names. Once constructed, the value is
//! proven valid for the remainder of its life — callers no longer need to
//! re-validate a `&str` before using it in a URL path or an encoded query.
//!
//! Construct these via [`std::str::FromStr`] (used automatically by clap for
//! typed CLI arguments) or [`TryFrom<String>`] (for values obtained at
//! runtime, e.g. read from a file or an API response). There is no
//! "unchecked" constructor: every instance has passed validation.

use std::fmt;
use std::str::FromStr;

/// Error returned when a string fails identifier validation.
///
/// Deliberately not `anyhow::Error`: `clap`'s `value_parser!` requires
/// `FromStr::Err: Display + Send + Sync + 'static` (no dependency on the
/// `anyhow` crate), so this is a small standalone error type instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierError(String);

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IdentifierError {}

fn err(message: impl Into<String>) -> IdentifierError {
    IdentifierError(message.into())
}

/// Macro to generate the shared boilerplate (Display/AsRef/as_str/TryFrom)
/// for each identifier newtype, given its validating constructor.
macro_rules! identifier_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Borrow the validated value as a `&str`.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::try_from(value.to_string())
            }
        }
    };
}

identifier_newtype!(TableName);
identifier_newtype!(SysId);
identifier_newtype!(PathSegment);
identifier_newtype!(EncodedQueryValue);

impl TryFrom<String> for TableName {
    type Error = IdentifierError;

    /// Validates ServiceNow table name rules: non-empty, ASCII alphanumeric
    /// or `_` only.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(err("Table name must not be empty."));
        }
        if !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err(err(format!(
                "Invalid table name '{value}'. Table names may contain only ASCII letters, digits, and underscores."
            )));
        }
        Ok(Self(value))
    }
}

fn validate_path_segment_chars(value: &str) -> Result<(), IdentifierError> {
    if value.is_empty() {
        return Err(err("Value must not be empty."));
    }
    // `.` and `..` are relative path segments that WHATWG URL normalization
    // resolves during `Url::join`, which could walk the request to a different
    // resource. Reject them outright.
    if value == "." || value == ".." {
        return Err(err(format!(
            "Invalid value '{value}'. Relative path segments ('.' and '..') are not allowed."
        )));
    }
    // Reject `\` as well as `/`: on http(s) URLs the WHATWG parser treats a
    // backslash as a path separator, so a value like `..\other` would break
    // out of its intended segment.
    if value
        .chars()
        .any(|ch| ch == '/' || ch == '\\' || ch == '?' || ch == '#' || ch.is_control())
    {
        return Err(err(format!(
            "Invalid value '{value}'. Values used in API paths must not contain '/', '\\', '?', '#', or control characters."
        )));
    }
    Ok(())
}

impl TryFrom<String> for SysId {
    type Error = IdentifierError;

    /// Validates the ServiceNow `sys_id` format: exactly 32 hexadecimal
    /// characters. This is stricter than generic path-segment safety, and
    /// because hex digits exclude `/`, `\`, and `.`, it also guarantees a
    /// `sys_id` cannot alter the request path it is embedded in.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err(format!(
                "Invalid sys_id '{value}'. A sys_id must be exactly 32 hexadecimal characters."
            )));
        }
        Ok(Self(value))
    }
}

impl TryFrom<String> for PathSegment {
    type Error = IdentifierError;

    /// Validates generic path-segment safety (non-empty, no `/`, `\`, `?`,
    /// `#`, control characters, or `.`/`..` segments) for non-`sys_id` path
    /// components. Unlike [`SysId`] this does not require a hexadecimal format.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_path_segment_chars(&value)?;
        Ok(Self(value))
    }
}

impl TryFrom<String> for EncodedQueryValue {
    type Error = IdentifierError;

    /// Validates that a value embedded in an encoded ServiceNow query does
    /// not contain query operator characters (`^ = < > !`) or control
    /// characters.
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(err("Value must not be empty."));
        }
        if value.chars().any(|ch| {
            ch == '^' || ch == '=' || ch == '<' || ch == '>' || ch == '!' || ch.is_control()
        }) {
            return Err(err(format!(
                "Invalid value '{value}'. Values embedded in encoded queries must not contain ServiceNow query operator characters."
            )));
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_allow_servicenow_identifiers() {
        assert!("incident".parse::<TableName>().is_ok());
        assert!("x_acme_app_table1".parse::<TableName>().is_ok());
    }

    #[test]
    fn table_names_reject_empty() {
        let err = "".parse::<TableName>().unwrap_err().to_string();
        assert!(err.contains("must not be empty"));
    }

    #[test]
    fn table_names_reject_path_and_query_characters() {
        assert!("incident/foo".parse::<TableName>().is_err());
        assert!("incident?x=1".parse::<TableName>().is_err());
        assert!("incident#frag".parse::<TableName>().is_err());
        assert!("incident^ORactive=true".parse::<TableName>().is_err());
    }

    #[test]
    fn path_segments_reject_path_breakout_characters() {
        assert!("abc123".parse::<PathSegment>().is_ok());
        assert!("abc/123".parse::<PathSegment>().is_err());
        assert!("abc?x=1".parse::<PathSegment>().is_err());
        assert!("abc#frag".parse::<PathSegment>().is_err());
    }

    #[test]
    fn sys_id_rejects_empty() {
        assert!("".parse::<SysId>().is_err());
    }

    #[test]
    fn path_segment_allows_non_hex_but_rejects_slash() {
        assert!("abc123".parse::<PathSegment>().is_ok());
        assert!("not-a-hex-id".parse::<PathSegment>().is_ok());
        assert!("abc/123".parse::<PathSegment>().is_err());
    }

    #[test]
    fn encoded_query_literals_reject_operator_characters() {
        assert!("x_my_app".parse::<EncodedQueryValue>().is_ok());
        assert!("abc^ORactive=true".parse::<EncodedQueryValue>().is_err());
        assert!("x=y".parse::<EncodedQueryValue>().is_err());
    }

    #[test]
    fn encoded_query_literal_rejects_empty() {
        assert!("".parse::<EncodedQueryValue>().is_err());
    }

    #[test]
    fn try_from_string_matches_from_str() {
        assert_eq!(
            TableName::try_from("incident".to_string()).unwrap(),
            "incident".parse::<TableName>().unwrap()
        );
        assert!(TableName::try_from("bad/table".to_string()).is_err());
    }

    #[test]
    fn as_str_and_display_and_as_ref_roundtrip() {
        let table: TableName = "incident".parse().unwrap();
        assert_eq!(table.as_str(), "incident");
        assert_eq!(table.to_string(), "incident");
        assert_eq!(AsRef::<str>::as_ref(&table), "incident");
    }

    // Regression: review finding #1 (path traversal). A backslash is
    // normalized to `/` by WHATWG URL parsing on http(s), and `.`/`..`
    // segments are resolved during `Url::join`, either of which can retarget
    // a request (e.g. `table delete incident '..\sys_user\<id>'` hitting
    // `sys_user`). These must be rejected before a value reaches a URL.
    #[test]
    fn path_segment_rejects_backslash_and_dot_segments() {
        assert!(r"a\b".parse::<PathSegment>().is_err());
        assert!(r"..\etc".parse::<PathSegment>().is_err());
        assert!("..".parse::<PathSegment>().is_err());
        assert!(".".parse::<PathSegment>().is_err());
    }

    // Regression: review finding #2. A `sys_id` is exactly 32 hexadecimal
    // characters; enforcing that both gives the newtype a real invariant and
    // (because hex excludes `/`, `\`, `.`) closes the finding #1 traversal for
    // the `sys_id` path position used by `table get/update/delete`.
    #[test]
    fn sys_id_requires_32_char_hex() {
        assert!("6816f79cc0a8016401c5a33be04be441".parse::<SysId>().is_ok());
        assert!("6816F79CC0A8016401C5A33BE04BE441".parse::<SysId>().is_ok());
        assert!("abc123".parse::<SysId>().is_err()); // too short
        assert!("6816f79cc0a8016401c5a33be04be44".parse::<SysId>().is_err()); // 31
        assert!(
            "6816f79cc0a8016401c5a33be04be4411"
                .parse::<SysId>()
                .is_err()
        ); // 33
        assert!("g816f79cc0a8016401c5a33be04be441".parse::<SysId>().is_err()); // non-hex
        assert!(
            r"..\sys_user\aaaaaaaaaaaaaaaaaaaa"
                .parse::<SysId>()
                .is_err()
        );
    }
}
