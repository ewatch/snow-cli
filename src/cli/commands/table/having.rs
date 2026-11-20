//! Translation of `table stats --having` clauses into the Aggregate API's
//! native `sysparm_having` syntax.
//!
//! The Aggregate API expects each clause as `aggregate^field^operator^value`
//! (for example `count^*^>^5` or `avg^priority^>=^2`) and ANDs clauses that
//! are separated by commas. A bare `count>5` is rejected with HTTP 400
//! ("Invalid HAVING clause: wrong number of arguments"), so the CLI accepts a
//! readable form and translates it:
//!
//! - `count>5` becomes `count^*^>^5`
//! - `avg(priority)>=2` becomes `avg^priority^>=^2`
//! - native clauses containing `^` are passed through unchanged

use crate::models::validation::ValidationError;

/// Aggregates the Aggregate API accepts in a HAVING clause.
const AGGREGATES: [&str; 5] = ["count", "avg", "min", "max", "sum"];

/// Comparison operators, longest first so `>=` is matched before `>`.
const OPERATORS: [&str; 6] = [">=", "<=", "!=", ">", "<", "="];

/// Translate a user-supplied `--having` expression into `sysparm_having`.
pub(super) fn to_sysparm_having(expression: &str) -> Result<String, ValidationError> {
    let clauses = expression
        .split(',')
        .map(str::trim)
        .map(|clause| {
            if clause.contains('^') {
                Ok(clause.to_owned())
            } else {
                translate_clause(clause)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(clauses.join(","))
}

fn translate_clause(clause: &str) -> Result<String, ValidationError> {
    let (left, operator, value) = OPERATORS
        .iter()
        .find_map(|operator| {
            clause
                .split_once(operator)
                .map(|(left, value)| (left.trim(), *operator, value.trim()))
        })
        .ok_or_else(invalid_clause)?;
    if value.is_empty() {
        return Err(invalid_clause());
    }

    let (aggregate, field) = match left.split_once('(') {
        Some((aggregate, rest)) => {
            let field = rest.strip_suffix(')').ok_or_else(invalid_clause)?.trim();
            (aggregate.trim().to_ascii_lowercase(), field)
        }
        None => (left.to_ascii_lowercase(), ""),
    };
    if !AGGREGATES.contains(&aggregate.as_str()) {
        return Err(invalid_clause());
    }
    let field = match (aggregate.as_str(), field) {
        ("count", "") => "*",
        (_, "") => return Err(invalid_clause()),
        (_, field)
            if field
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '*') =>
        {
            field
        }
        _ => return Err(invalid_clause()),
    };
    Ok(format!("{aggregate}^{field}^{operator}^{value}"))
}

fn invalid_clause() -> ValidationError {
    ValidationError::new(
        "Invalid --having clause. Use count>5, avg(priority)>=2 (aggregates: count, avg, min, \
         max, sum; operators: >, >=, <, <=, =, !=), comma-separated to combine, or the native \
         aggregate^field^operator^value form.",
    )
}

#[cfg(test)]
mod tests {
    use super::to_sysparm_having;

    #[test]
    fn translates_readable_clauses() {
        for (input, expected) in [
            ("count>5", "count^*^>^5"),
            ("COUNT >= 5", "count^*^>=^5"),
            ("count(*)!=0", "count^*^!=^0"),
            ("avg(priority)>=2", "avg^priority^>=^2"),
            (
                "sum(reassignment_count) < 10",
                "sum^reassignment_count^<^10",
            ),
            ("count>15, count<25", "count^*^>^15,count^*^<^25"),
        ] {
            assert_eq!(to_sysparm_having(input).unwrap(), expected, "{input}");
        }
    }

    #[test]
    fn passes_native_clauses_through() {
        assert_eq!(
            to_sysparm_having("COUNT^sys_id^>^5,avg(priority)>2").unwrap(),
            "COUNT^sys_id^>^5,avg^priority^>^2"
        );
    }

    #[test]
    fn rejects_malformed_clauses() {
        for input in [
            "",
            "count",
            "count>",
            "median(priority)>2",
            "avg>2",
            "avg(priority>2",
            "avg(pri ority)>2",
        ] {
            assert!(to_sysparm_having(input).is_err(), "{input}");
        }
    }
}
