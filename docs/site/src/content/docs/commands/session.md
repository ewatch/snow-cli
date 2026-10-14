# session

Create a persistent native ServiceNow cookie session, inspect its user context,
and switch its domain, application scope, or update set. These commands use
ServiceNow's own UI endpoints. SN-Utils is not required.

## Create a dedicated session profile

Start from a Basic profile with a stored password:

```bash
snow-cli --profile dev session login dev-session
snow-cli --profile dev-session session show
```

`session login` posts to `login.do`, verifies cookie-only authentication, and
stores cookies and the CSRF token in the OS keychain. The source profile is
preserved. Use `--password-stdin` to pipe a password from a secret manager rather
than use the stored password. No password or cookie argument is accepted.

For SSO, MFA, or an existing browser session, pipe its complete Cookie header:

```bash
secret-manager read servicenow-cookie | snow-cli --profile dev session attach browser-session --cookie-stdin
snow-cli --profile browser-session session show
```

Replace the secret-manager command with your credential source. Attachment
validates the cookie session before storing it. The authenticated login identity
is unknown for attached sessions; the effective user is verified separately.
An attached session shares context with any browser using the same cookies.

## Switch context

```bash
snow-cli --profile dev-session session options domain
snow-cli --profile dev-session session switch domain <domain_sys_id>
snow-cli --profile dev-session session switch domain global

snow-cli --profile dev-session session options application
snow-cli --profile dev-session session switch application <scope_name_or_picker_id>

snow-cli --profile dev-session session options updateset
snow-cli --profile dev-session session switch updateset <picker_sys_id>
```

Select IDs returned by the current session's picker. Application scope names are
resolved through its accessible options. Options can change after switching
application or domain, so refresh them before selecting an update set.

Each switch reads the session before and after, verifies the requested selection,
and returns all three contexts. Application switching can also change the update
set. An already-current selection is verified without sending a redundant PUT.
Switching requires all three picker selections to be readable before and after
the operation, including when the requested selection is already current.
Missing or malformed contexts fail explicitly rather than reporting verified
success. This includes instances whose UI does not expose a domain picker;
`session show` can still report unknown context, but switching is unsupported:
the command fails with `VALIDATION_ERROR` (exit 2) before sending anything. A
switch that was sent but cannot be verified reports `SESSION_SWITCH_UNCERTAIN`
(exit 1, `retry: manual_review`).
If mutation or read-back fails, inspect with `session show`; the CLI does not
retry the mutation or perform an automatic rollback.

Use the same session profile for later commands:

```bash
snow-cli --profile dev-session table list incident --limit 20
snow-cli --profile dev-session table list incident --domain <domain_sys_id>
```

`--domain` filters exact record ownership; switching the domain changes the
session picker context. Home domain, session domain, and record domain are
separate. ACLs, domain visibility, and endpoint behavior still apply. A selected
application does not prove that every REST write is captured in its update set;
record scope and each write endpoint's semantics also matter. Explicit command
options such as `script run --scope` remain authoritative.

## Inspect other session context

`session show` reports the effective user, known login identity, impersonation
state, home and selected domain, application, update set, roles, available elevated
roles, and active elevated roles. It also reports language, time zone, date/time
format, encryption contexts, and timeout properties when the UI exposes them.
Missing values are null or listed as unavailable. Timeout property values are
reported as supplied by the instance, without assigning units.

Inspection never includes cookies, CSRF tokens, or arbitrary user preferences.
`auth token` does not export native session credentials.

## Expiry, isolation, and read-only access

Cookies and CSRF tokens persist in the keychain and update when the instance
rotates them. A native session is bound to its instance origin; a cross-instance
`--instance` override is rejected before credentials are sent. Native requests
do not automatically follow redirects or fall back to Basic/OAuth authentication.
Renew an expired session with `session login` or `session attach`.

```bash
snow-cli --profile dev-session session forget
```

`forget` removes local credentials and retains the profile. It does not log out
or modify the remote session. `auth logout` also removes stored credentials.

`snow-cli-ro` supports `show`, `options`, `attach`, and `forget`. It excludes native
login and switching. The full binary with `--read-only` blocks login and switching
before network requests.

The native UI endpoints are release-dependent internal contracts, not documented
public REST APIs. Unsupported picker responses fail explicitly; optional bootstrap
metadata is reported as unknown. Instance security settings can also prevent
cookie-based API access. Separate session profiles help avoid concurrent context
changes, but sessions for the same user can still interact through platform user
preferences; refresh and verify context before sensitive writes.
