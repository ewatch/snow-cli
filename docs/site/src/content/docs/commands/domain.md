# domain

Inspect domain separation before interpreting query results. These read-only
commands work in both `snow-cli` and `snow-cli-ro` and do not switch the session's
domain or change instance configuration.

```bash
snow-cli --profile dev --output json domain status
snow-cli --profile dev --output json domain list --limit 100
snow-cli --profile dev --output json domain table incident
snow-cli --profile dev --output json domain table incident --counts
snow-cli --profile dev --output json domain inspect <domain_sys_id> --limit 100
```

`domain status` reads the domain activation and query-method properties plus the
authenticated user's home domain (`sys_user.sys_domain`). It does not claim that
the home domain is the current session domain. Missing metadata is unknown;
`enabled: null` does not mean disabled. Users without admin usually cannot read
the properties, so `separation_in_use: true` reports when the home domain is not
`global`, which proves separation is in use; it is null otherwise.
Authentication or API errors remain errors.

`domain list` returns visible domain records, their sys_ids, parent references,
active state, and domain paths. Pagination metadata tells you whether the result
was truncated. `global` is a special domain value and may have no record in this
list. Names and paths are informational; use the sys_id to identify a domain.

`domain table incident` walks the table hierarchy and shows domain fields with
their defining table, including `sys_domain`, `sys_domain_path`, and
`sys_overrides` where readable. For example, incident can inherit domain fields
from task. `has_sys_domain: true` confirms readable field metadata, and `false`
means the dictionary was readable but the table and its ancestors have no
`sys_domain`. `null` means the CLI could not read the dictionary. Field presence
alone does not prove activation or which records the user can access.

## Count records per domain

```bash
snow-cli --profile dev --output json domain table incident --counts
snow-cli --profile dev --output json domain table incident --counts --via-script
```

`--counts` adds a `counts` object. `by_domain` lists each domain's `domain` name,
its `display` path (for example `TOP/ACME`), its `sys_domain` sys_id, and `count`,
from one Aggregate API request. `scope: "visible_domains"` means the counts cover
only the domains this session can see: a user in ACME does not see records in
sibling domains, and nothing in the result says they exist.

`cross_domain` reports records outside those domains. When the session has the
`admin` or `query_no_domain_table_api` role (admin implies it), two Table API
totals, with and without `sysparm_query_no_domain`, give `visible_total`,
`all_total`, and `hidden`. The Aggregate API ignores `sysparm_query_no_domain`
even with the role, which is why the Table API is used. Without the role,
`available` is `false` and `hidden` is `null`. Equal counts never prove that
nothing is hidden.

`--via-script` replaces the Aggregate API with one read-only `GlideAggregate`
using `queryNoDomain()`, giving per-domain counts across all domains with
`scope: "all_domains"`. It needs the admin role, runs as a background script
(recorded in the instance's script execution history), and is refused by
`snow-cli-ro`.

`domain inspect <domain_sys_id|global>` reads the domain and its direct children,
Contains relationships in both directions, and user/group visibility grants
pointing at that domain. Each relationship table has its own returned, total,
truncated, and next-offset metadata. `--limit` bounds each table separately;
`--query-no-domain` explicitly applies the privileged option to each read.
These are observed relationships within server visibility, not an effective
access calculation. Ancestor grants and inherited child visibility can also
matter. API errors remain errors rather than being reported as empty grants.

## Query an exact domain

```bash
snow-cli --profile dev --output json table list incident --domain global --limit 20
snow-cli --profile dev --output json table list incident --domain <domain_sys_id> --query 'active=true' --limit 20
snow-cli --profile dev --output json table stats incident --domain <domain_sys_id> --group-by state
```

`--domain` accepts `global` or a domain sys_id. It constrains exact record ownership;
it excludes descendants and shared global records unless those domains are
explicitly queried. It retains the authenticated user's visibility and ACLs.
It does not select a domain in the picker or change process context. Application
scope (`sys_scope`) is separate from record domain (`sys_domain`).

The CLI checks inherited `sys_domain` metadata before querying, refusing the read
if it cannot verify the field. This avoids ServiceNow ignoring an invalid domain
condition. Each `^NQ` branch receives the domain constraint. Copied queries with
a final `^EQ` are supported; embedded EQ terminators or leading OR conditions are
rejected. Default list projections include `sys_domain` when a domain option is
used; explicit `--fields` remains authoritative.

## Explicit cross-domain reads

```bash
snow-cli --profile dev --output json table list incident --query-no-domain --fields sys_id,number,sys_domain --limit 20
snow-cli --profile dev --output json table get incident <sys_id> --query-no-domain
snow-cli --profile dev --output json domain list --query-no-domain
```

`--query-no-domain` requests the Table API's `sysparm_query_no_domain=true`.
ServiceNow requires admin or `query_no_domain_table_api`; ACLs still apply.
Without either role, ServiceNow silently ignores the option and returns only
visible domains; the CLI then prints a warning on stderr.
Normal requests preserve the server's domain filtering. The option can be combined
with `table list --domain` to query exact ownership across authorized domains,
and it is sent on every page. A server error is returned without falling back.
The option is not available on Aggregate API stats or other command families.
[ServiceNow Table API documentation](https://www.servicenow.com/docs/r/api-reference/rest-apis/c_TableAPI.html).

## Show each record's domain

```bash
snow-cli --profile dev --output json table list sys_user --query 'active=true' --with-domain --limit 20
snow-cli --profile dev --output json table get incident <sys_id> --with-domain
```

`--with-domain` adds `sys_domain` and `sys_domain_path` to the requested fields
and a resolved `sys_domain_name` to each record. `global` has no domain record
and is reported as `global`. Names come from one extra `domain` lookup; a name
the session cannot read is `null`, while the sys_id and path remain. For lists,
`domains_in_page` counts the page's records per domain name, falling back to the
sys_id when the name is unknown. `table get` without `--fields` returns every
field, so only the name is added.

## Interpret results

A domain hierarchy is not a complete access model. Parent domains usually see
child-domain data; shared global data, Contains relationships, user/group
visibility grants, ACLs, and Before Query rules also affect results. Contains
relationships affect data visibility, not process inheritance. Explicit visibility
grants can remain effective regardless of the picker selection.

An empty result can mean no matching data or restricted visibility. Totals refer
to the server's current query context, not every record on the instance. Use domain
status, table metadata, explicit `sys_domain` fields, and authorized comparison
queries before concluding that a record is missing. Avoid durable filters based
on `sys_domain_path`: paths change when the hierarchy changes.
