//! Exact record-domain selection, independent of application scope or session visibility.
use crate::models::identifiers::{IdentifierError, SysId};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainId {
    Global,
    Domain(SysId),
}
impl FromStr for DomainId {
    type Err = IdentifierError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "global" {
            Ok(Self::Global)
        } else {
            value.parse().map(Self::Domain)
        }
    }
}
impl fmt::Display for DomainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => f.write_str("global"),
            Self::Domain(id) => id.fmt(f),
        }
    }
}
impl DomainId {
    /// Constrain every top-level NQ branch. EQ terminators would stop parsing
    /// before an appended filter, so only a final terminator is accepted.
    pub fn apply_to_query(&self, query: Option<&str>) -> anyhow::Result<String> {
        let query = query.unwrap_or("");
        let mut branches = Vec::new();
        for branch in query.split("^NQ") {
            let branch = branch.strip_suffix("^EQ").unwrap_or(branch);
            let first_term = branch
                .split('^')
                .find(|term| !term.is_empty())
                .unwrap_or("");
            let leading_or = first_term.starts_with("OR") && !first_term.starts_with("ORDERBY");
            anyhow::ensure!(
                !branch.split('^').any(|term| term.starts_with("EQ")) && !leading_or,
                "--domain cannot be combined with an embedded EQ terminator or a leading OR condition"
            );
            branches.push(if branch.is_empty() {
                format!("sys_domain={self}")
            } else {
                format!("{branch}^sys_domain={self}")
            });
        }
        Ok(branches.join("^NQ"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_domain_identifiers() {
        assert!("global".parse::<DomainId>().is_ok());
        assert!(
            "09ff3d105f231000b12e3572f2b4775d"
                .parse::<DomainId>()
                .is_ok()
        );
        for value in [
            "TOP",
            "",
            "global^NQactive=true",
            "javascript:gs.getUserID()",
        ] {
            assert!(value.parse::<DomainId>().is_err());
        }
    }
    #[test]
    fn constrains_all_query_branches_and_handles_final_eq() {
        let domain = DomainId::Global;
        assert_eq!(domain.apply_to_query(None).unwrap(), "sys_domain=global");
        assert_eq!(
            domain
                .apply_to_query(Some("active=true^ORpriority=1^NQstate=2^EQ"))
                .unwrap(),
            "active=true^ORpriority=1^sys_domain=global^NQstate=2^sys_domain=global"
        );
        assert!(
            domain
                .apply_to_query(Some("active=true^EQ^priority=1"))
                .is_err()
        );
        assert!(domain.apply_to_query(Some("ORactive=true")).is_err());
        assert!(domain.apply_to_query(Some("^ORactive=true")).is_err());
        assert_eq!(
            domain.apply_to_query(Some("ORDERBYnumber")).unwrap(),
            "ORDERBYnumber^sys_domain=global"
        );
        assert!(
            domain
                .apply_to_query(Some("active=true^EQignored"))
                .is_err()
        );
    }
}
