//! Table API display-value modes (`sysparm_display_value`).
//!
//! ServiceNow can return a field's stored value (`false`), its display label
//! (`true`), or both (`all`). [`DisplayValue`] names those modes for the CLI
//! (`raw`, `display`, `both`) and accepts the ServiceNow spellings as aliases.
//! Values returned in `both` mode are compacted by [`compact_both`] so fields
//! whose value equals their label stay plain strings.

use std::fmt;
use std::str::FromStr;

use serde_json::Value;

use super::identifiers::IdentifierError;
use super::record::Record;

/// A validated `--display-value` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayValue {
    /// Stored values (`sysparm_display_value=false`), e.g. priority `4`.
    Raw,
    /// Display labels (`sysparm_display_value=true`), e.g. priority `4 - Low`.
    /// Dates are rendered in the session user's timezone and format.
    Display,
    /// Both (`sysparm_display_value=all`), compacted to
    /// `{"value": .., "display": ..}` only where the two differ.
    Both,
}

impl DisplayValue {
    /// The `sysparm_display_value` parameter ServiceNow expects for this mode.
    pub fn sysparm_value(self) -> &'static str {
        match self {
            Self::Raw => "false",
            Self::Display => "true",
            Self::Both => "all",
        }
    }

    /// Canonical CLI spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Display => "display",
            Self::Both => "both",
        }
    }
}

impl fmt::Display for DisplayValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DisplayValue {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "raw" | "false" | "value" => Ok(Self::Raw),
            "display" | "true" => Ok(Self::Display),
            "both" | "all" => Ok(Self::Both),
            _ => Err(IdentifierError::from_message(format!(
                "Invalid display-value mode '{value}'. Use raw, display, or both (aliases: false, true, all)."
            ))),
        }
    }
}

/// Compact the `sysparm_display_value=all` shape of every field in `record`.
///
/// ServiceNow returns `{"display_value": d, "value": v}` per field. A field
/// whose label equals its value becomes the plain value; otherwise it becomes
/// `{"value": v, "display": d}`. Any other value is left unchanged.
pub fn compact_both(record: &mut Record) {
    for value in record.fields.values_mut() {
        let Value::Object(object) = value else {
            continue;
        };
        let (Some(raw), Some(display)) = (object.get("value"), object.get("display_value")) else {
            continue;
        };
        let compact = if raw == display {
            raw.clone()
        } else {
            serde_json::json!({ "value": raw, "display": display })
        };
        *value = compact;
    }
}

/// Split compacted `both` fields into flat columns for tabular formats.
///
/// `{"value": v, "display": d}` under `field` becomes `field = v` and
/// `field.display = d`, so CSV and TOON tables stay one value per cell.
pub fn split_both_columns(record: &mut Record) {
    split_both_columns_uniform(std::slice::from_mut(record));
}

/// Split compacted `both` fields uniformly across every record in a page.
///
/// If *any* record in `records` has a differing `{"value": v, "display": d}`
/// pair for `field`, every record that has `field` receives `<field>.display`
/// (repeating its plain value when value == display) so all rows share a
/// uniform key set and TOON/Auto can use compact `[N]{cols}` tabular encoding.
pub fn split_both_columns_uniform(records: &mut [Record]) {
    let mut split_keys = std::collections::BTreeSet::new();
    for record in records.iter() {
        for (key, value) in &record.fields {
            if is_compact_pair(value) {
                split_keys.insert(key.clone());
            }
        }
    }
    if split_keys.is_empty() {
        return;
    }
    for record in records {
        for key in &split_keys {
            let display_key = format!("{key}.display");
            match record.fields.get(key) {
                Some(value) if is_compact_pair(value) => {
                    if let Some(Value::Object(mut object)) = record.fields.remove(key) {
                        let raw = object.remove("value").unwrap_or(Value::Null);
                        let display = object.remove("display").unwrap_or(Value::Null);
                        record.fields.insert(display_key, display);
                        record.fields.insert(key.clone(), raw);
                    }
                }
                Some(plain) => {
                    let display = plain.clone();
                    record.fields.entry(display_key).or_insert(display);
                }
                None => {}
            }
        }
    }
}

pub(crate) fn is_compact_pair(value: &Value) -> bool {
    matches!(value, Value::Object(object)
        if object.len() == 2 && object.contains_key("value") && object.contains_key("display"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(value: Value) -> Record {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn parses_modes_and_servicenow_aliases() {
        for (input, expected) in [
            ("raw", DisplayValue::Raw),
            ("false", DisplayValue::Raw),
            ("display", DisplayValue::Display),
            ("TRUE", DisplayValue::Display),
            ("both", DisplayValue::Both),
            ("all", DisplayValue::Both),
        ] {
            assert_eq!(input.parse::<DisplayValue>().unwrap(), expected, "{input}");
        }
        assert!("labels".parse::<DisplayValue>().is_err());
    }

    #[test]
    fn maps_to_sysparm_values() {
        assert_eq!(DisplayValue::Raw.sysparm_value(), "false");
        assert_eq!(DisplayValue::Display.sysparm_value(), "true");
        assert_eq!(DisplayValue::Both.sysparm_value(), "all");
    }

    #[test]
    fn compact_both_keeps_equal_values_plain() {
        let mut r = record(serde_json::json!({
            "number": {"display_value": "INC0010025", "value": "INC0010025"},
            "priority": {"display_value": "4 - Low", "value": "4"},
            "assignment_group": {"display_value": "", "value": ""},
            "plain": "x"
        }));
        compact_both(&mut r);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({
                "assignment_group": "",
                "number": "INC0010025",
                "plain": "x",
                "priority": {"value": "4", "display": "4 - Low"}
            })
        );
    }

    #[test]
    fn split_both_columns_flattens_pairs_for_tables() {
        let mut r = record(serde_json::json!({
            "number": "INC1",
            "priority": {"value": "4", "display": "4 - Low"},
            "other": {"value": "1", "link": "x"}
        }));
        split_both_columns(&mut r);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({
                "number": "INC1",
                "other": {"value": "1", "link": "x"},
                "priority": "4",
                "priority.display": "4 - Low"
            })
        );
    }

    #[test]
    fn split_both_columns_uniform_keeps_tabular_keys_consistent_across_rows() {
        let mut records = vec![
            record(serde_json::json!({
                "number": {"display_value": "INC1", "value": "INC1"},
                "assignment_group": {"display_value": "Network", "value": "grp1"},
                "priority": {"display_value": "4 - Low", "value": "4"}
            })),
            record(serde_json::json!({
                "number": {"display_value": "INC2", "value": "INC2"},
                "assignment_group": {"display_value": "", "value": ""},
                "priority": {"display_value": "1 - Critical", "value": "1"}
            })),
        ];
        for r in &mut records {
            compact_both(r);
        }
        split_both_columns_uniform(&mut records);
        // `number` was equal on every row so it is NOT split; `assignment_group`
        // differed on row 0 so row 1 also gets `assignment_group.display`.
        let keys0: Vec<&String> = records[0].fields.keys().collect();
        let keys1: Vec<&String> = records[1].fields.keys().collect();
        assert_eq!(keys0, keys1);
        assert_eq!(
            serde_json::to_value(&records[1]).unwrap(),
            serde_json::json!({
                "assignment_group": "",
                "assignment_group.display": "",
                "number": "INC2",
                "priority": "1",
                "priority.display": "1 - Critical"
            })
        );
    }
}
