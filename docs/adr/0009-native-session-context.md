# ADR-0009: Native session context without SN-Utils

## Status

Accepted

## Context

Domain, application scope, and update-set pickers belong to a UI session. Ordinary
Basic/OAuth REST authentication does not establish that a browser session's
selected context will apply. CLI users need persistent, inspectable context
without requiring an extension or installing instance resources.

## Decision

Add dedicated native cookie profiles with secrets in the OS keychain. Use native
UI endpoints through the client seam to inspect context and switch only options
accessible to that session. Verify all three contexts after each mutation. Keep
home domain, selected session domain, and record ownership distinct. Never claim
HTTP success alone proves a switch or that picker state proves REST capture.

Bind credentials to an origin, disable automatic redirects and authentication
fallback for native requests, preserve cookie rotation, and expose no credential
export. The native authenticator is the sole owner of current cookies and CSRF;
form requests take fresh request snapshots, never cache native credentials. One
cookie merger handles bootstrap, login, REST, and script response updates,
including deletion and expiry. Report optional metadata as unknown. Read-only mode permits inspection
and attachment while denying native form login and context mutation.

## Alternatives Considered

SN-Utils can be extended separately but is not required by this implementation.
Writing user preferences or running background scripts can affect other sessions
and does not prove picker state, so neither is used as a switching fallback.
A dedicated Scripted REST API would require instance installation.

## Consequences

Context persists across CLI invocations and normal table/API requests reuse it.
Native endpoints depend on instance release and security settings; unsupported
contracts fail explicitly. Browser cookies share context with their browser,
and platform preferences can affect other sessions for the same user. Failed
verification leaves the state uncertain and requires a fresh inspection. Typed
switch failures distinguish request failure, verification failure, mismatch, and
incomplete context after the request;
they preserve underlying causes and render `SESSION_SWITCH_UNCERTAIN` with
`manual_review` guidance. Classified causes retain safe diagnostics and HTTP
status; unclassified source text is not exposed. Unsupported output formats are
rejected before context mutation.
