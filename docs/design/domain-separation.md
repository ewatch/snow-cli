# Domain-aware reads

ServiceNow separates data, processes, and UI configuration by domain. Domain
separation is independent of application scope. For integrations, the platform
adds visibility constraints before applying contextual security. The CLI must
never infer effective access from a parent tree alone: Contains relationships,
user/group visibility grants, global data, ACLs, and Before Query rules also matter.

## Read contract

- Existing requests retain server defaults. No session domain is switched.
- `domain status` reports the activation property as true/false/null. Absent or
  unreadable metadata is unknown; request failures remain errors. The user's home
  domain is returned separately from the unknown current session domain.
- `domain list` reads the `domain` table with parent references and pagination
  metadata. It does not manufacture a `global` record: `global` is a special value.
- `domain inspect` reads direct children, incoming/outgoing Contains links, and
  user/group visibility grants, with independent pagination metadata. It does
  not claim to calculate effective access.
- `domain table` walks `sys_db_object.super_class` and reads domain fields from
  `sys_dictionary`, preserving the defining table. Presence is evidence of field
  support, not proof of activation or effective visibility.
- `table list/stats --domain` accepts only `global` or a 32-digit hexadecimal sys_id.
  It filters exact record ownership, excluding descendants and shared global rows
  unless explicitly selected. It is not a domain picker or an authorization grant.
  Every NQ branch is constrained. A final EQ terminator is removed before adding
  the constraint; embedded terminators and leading OR conditions are rejected.
  The inherited sys_domain field must be readable before issuing the target query,
  because ServiceNow can silently ignore invalid query fields.
- `table list/get --query-no-domain` sends `sysparm_query_no_domain=true` explicitly.
  It is preserved on every page. ServiceNow requires admin or the
  `query_no_domain_table_api` role. ACLs still apply. No retry without the flag or
  automatic escalation occurs. The Aggregate API has no equivalent documented
  flag, so stats does not expose it.
- Domain commands and read options are available in `snow-cli-ro`. A read-only
  policy limits mutations, not which domains the credentials can access.

## Research evidence (2026-10-04)

The local ServiceNowDocs checkout (Brazil documentation) explains session versus
record scope, visibility grants versus Contains relationships, inherited domain
fields, and the need to avoid using mutable domain paths as durable identifiers.

Read-only Table API requests on dev404396 found:

- `glide.sys.domain.enabled=true`, `glide.sys.domain.provider=domain_paths`, and
  `glide.sys.domain.paths.installed=true`.
- Nine domain records returned, with TOP and child/customer domains.
- One Contains relationship links the MSP domain to TOP. User and group visibility
  tables returned no records for admin; non-global active users are present.
- The non-global incident query returned no rows in admin context. This instance
  therefore cannot demonstrate incident isolation between populated tenants.
- The authenticated admin user's home domain is `global`.
- Incident inherits `sys_domain` and `sys_domain_path` from task. The dictionary
  uses both plain strings and reference objects with a `value` property.

These observations verify metadata discovery. They do not prove isolation for a
restricted tenant user; the initial domain investigation performed no impersonation, plugin activation,
domain changes, or test-data creation. Subsequent native session switching and
temporary update-set verification are documented in [native sessions](native-sessions.md). The installed CLI's keychain lookup stalled,
so authenticated REST reads supplied the instance evidence.

## Sources

- [Database queries and integrations](https://www.servicenow.com/docs/r/platform-security/bp-db-query-with-ds.html)
- [Domain scope](https://www.servicenow.com/docs/r/platform-security/c_DomainScope.html)
- [Visibility and Contains domains](https://www.servicenow.com/docs/r/platform-security/c_DomainVisibility.html)
- [Avoiding domain paths in scripts](https://www.servicenow.com/docs/r/platform-security/bp-no-domain-path-in-scripts.html)
- [Table API parameter and role requirements](https://www.servicenow.com/docs/r/api-reference/rest-apis/c_TableAPI.html)
