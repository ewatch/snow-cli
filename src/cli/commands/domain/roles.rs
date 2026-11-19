//! Session role probes that decide whether cross-domain reads can work.
use crate::client::{SnowClient, pagination::PaginationConfig};
use crate::models::identifiers::TableName;

/// Roles that let the Table API honour `sysparm_query_no_domain`. `admin`
/// implies every role, so an admin never needs the explicit grant.
pub(super) const CROSS_DOMAIN_ROLES: &str = "admin,query_no_domain_table_api";

/// Whether this session holds any of `roles` (comma-separated). `None` when
/// the role table could not be read, so callers report "unknown".
pub(super) async fn session_has_any_role(client: &mut SnowClient, roles: &str) -> Option<bool> {
    let query = format!("user=javascript:gs.getUserID()^role.nameIN{roles}^state=active");
    match client
        .get_table_records(
            &TableName::from_static("sys_user_has_role"),
            Some(&query),
            Some("sys_id"),
            &PaginationConfig::default().with_limit(Some(1)),
            None,
        )
        .await
    {
        Ok(rows) => Some(!rows.is_empty()),
        Err(error) => {
            tracing::debug!(error = %error, "Could not read session roles");
            None
        }
    }
}

/// ServiceNow silently ignores `sysparm_query_no_domain` without the role, so
/// when `query_no_domain` was requested, say so instead of letting a
/// visible-only result pass as complete. Does nothing when it was not.
pub(crate) async fn warn_if_query_no_domain_ignored(
    client: &mut SnowClient,
    query_no_domain: bool,
) {
    if query_no_domain && session_has_any_role(client, CROSS_DOMAIN_ROLES).await == Some(false) {
        tracing::warn!(
            "--query-no-domain has no effect without the admin or query_no_domain_table_api role; results cover only the domains visible to this session"
        );
    }
}
