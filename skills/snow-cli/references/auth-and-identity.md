# Authentication, Users, Groups & Identity

Use when answering: "Who am I authenticated as?", "Why does user X have (or lack) role R?", "Which groups/delegates cover user X?", or "Why is an OAuth/API client getting 401/403?"

All inspection commands below are read-only and work with `snow-cli-ro` (do not run `auth login/logout` or `session login/switch` unless explicitly asked). For configuring OAuth 2.0 user login (OOB ServiceNow SDK `--sdk-oauth` paste-code flow or custom OAuth app with localhost callback), see `references/oauth-setup.md`.

## 1. Key tables & CLI commands

| Command / Table | What it stores | Key fields to project |
|---|---|---|
| `snow-cli-ro auth status --verify` | Active profile identity, verified user, latency, instance build | — |
| `snow-cli-ro session show` | Native cookie session effective user, impersonation, roles, elevated roles | Requires native session profile |
| `sys_user` | User accounts & lockout/web-service flags | `sys_id,user_name,name,email,active,locked_out,web_service_access_only,internal_integration_user,source,sys_domain` |
| `sys_user_group` | Groups & managers | `sys_id,name,active,manager.name,parent.name,type` |
| `sys_user_grmember` | User ↔ Group memberships | `user.user_name,group.name,group` |
| `sys_user_role` | Role definitions | `sys_id,name,elevated_privilege,assignable_by,sys_scope` |
| `sys_user_role_contains` | Role hierarchy (parent role → contained child role) | `role.name,contains.name` |
| `sys_user_has_role` | Effective user roles (direct + inherited + group-granted) | `user.user_name,role.name,state,inherited,granted_by.name` |
| `sys_group_has_role` | Roles assigned to a group | `group.name,role.name,inherits` |
| `sys_user_delegate` | Active approval/assignment/notification delegates | `user.user_name,delegate.user_name,starts,ends,approvals,assignments,notifications` |
| `oauth_entity` | OAuth clients & providers | `name,type,client_id,active,access_token_lifespan,refresh_token_lifespan,public_client` |

## 2. Verify active CLI identity first

```bash
# Confirm target instance, user, build, and live auth in one request
snow-cli-ro auth status --verify

# If using a native session profile, inspect effective user, impersonation & elevated roles
snow-cli-ro --profile dev-session session show
```

## 3. Trace where a user's roles come from (and why one is missing)

```bash
# 1. Inspect user account status & flags
snow-cli-ro table list sys_user \
  --query 'user_name=abel.tuter' \
  --fields sys_id,user_name,name,email,active,locked_out,web_service_access_only,internal_integration_user,sys_domain

# 2. List all active effective roles and see whether each is direct or inherited via a group/role
snow-cli-ro table list sys_user_has_role \
  --query 'user=<user_sys_id>^state=active' \
  --fields role.name,inherited,granted_by.name --order-by role.name --all

# 3. Check user's group memberships and what roles those groups carry
snow-cli-ro table list sys_user_grmember \
  --query 'user=<user_sys_id>' \
  --fields group,group.name,group.active --limit 100

snow-cli-ro table list sys_group_has_role \
  --query 'group=<group_sys_id>' \
  --fields group.name,role.name,inherits --limit 100

# 4. Walk role inheritance: which parent roles grant role X, or what X contains?
snow-cli-ro table list sys_user_role_contains \
  --query 'contains.name=itil' \
  --fields role.name,contains.name --limit 50

snow-cli-ro table list sys_user_role_contains \
  --query 'role.name=itil' \
  --fields role.name,contains.name --limit 50
```

## 4. Check delegates & OAuth / SSO configuration

```bash
# Active delegations for a user
snow-cli-ro table list sys_user_delegate \
  --query 'user=<user_sys_id>^ends>=javascript:gs.nowDateTime()' \
  --fields user.user_name,delegate.user_name,starts,ends,approvals,assignments,notifications --limit 20

# Inspect OAuth application registry entries (never request client_secret!)
snow-cli-ro table list oauth_entity \
  --query 'active=true' \
  --fields sys_id,name,type,client_id,active,access_token_lifespan,refresh_token_lifespan --limit 50

# Check multi-provider SSO / authentication properties (requires admin read)
snow-cli-ro table list sys_properties \
  --query 'nameLIKEglide.authenticate.multisso^ORnameLIKEglide.oauth' \
  --fields name,value --limit 30
```

## 5. Gotchas

- **Never project secret fields:** Never pass `--fields '*'` on `oauth_entity`, `sys_user`, or credential tables (`discovery_credentials`) when inspecting auth config — explicitly project safe columns so secrets/hashes aren't pulled into context.
- **Explicit roles (`snc_internal` / `snc_external`):** If the Explicit Roles plugin is active, users without `snc_internal` or `snc_external` may lose default access even when other roles look right.
- **`web_service_access_only=true`:** Integration users with this flag can call REST/Table APIs (`snow-cli`) normally, but cannot log into the browser UI or create native `session login` cookie sessions.
- **Elevated roles (`security_admin`, etc.):** Having `security_admin` in `sys_user_has_role` does **not** activate it for REST/Basic auth requests — elevated roles only take effect after explicit elevation in an interactive session (`session show` reports `available` vs `active` elevated roles).
- **401 vs 403 in `snow-cli`:** `UNAUTHORIZED` (401) means invalid/expired credentials, locked/inactive user, or SSO/OAuth failure; `FORBIDDEN`/`ACL_DENIED` (403) means authentication succeeded but ACLs, Table API access, or IP access controls blocked the call.
