# Native sessions

Native session profiles add persistent cookie authentication and context switching
without an SN-Utils dependency. See [ADR 0009](../adr/0009-native-session-context.md)
and the [command reference](../site/src/content/docs/commands/session.md).

## Architecture

`NativeSession` holds origin, cookie header, CSRF token, and known login identity.
Secrets use `Secret` wrappers and only serialize to the OS keychain. TOML stores
an ordinary profile with `auth_method = "native_session"`; it contains no secrets.
A dedicated profile preserves the source Basic/OAuth profile. Attached cookies do
not establish the original login identity, particularly during impersonation.

Authentication validates the session origin against the actual requested
instance, including CLI overrides. Native transports disable redirects, preserve
route cookies, observe Set-Cookie changes, and never auto-login on expiry.
Form login follows only same-origin redirects. CLI commands have no HTTP seam;
all requests and immutable policy checks remain in `src/client`.

Session inspection reads `navpage.do`, extracts CSRF, decodes the specific native
bootstrap JSON string without executing JavaScript, and whitelists metadata.
Current user and picker endpoints prove effective user and selected context.
Bootstrap and current-user identities must match when both are available.
The user's home domain is read separately from `sys_user.sys_domain`; record
domain is not a user-session property. Available elevated roles are intersected
with current roles to report active elevated roles, matching the served native UI.

## Observed endpoint contracts

| Operation | Native endpoint | Request |
|---|---|---|
| Current user | `/api/now/ui/user/current_user` | GET |
| Current contexts | `/api/now/ui/concoursepicker/current` | GET |
| Domain options/switch | `/api/now/ui/concoursepicker/domain` | GET / PUT `{"value":"id"}` |
| Application options/switch | `/api/now/ui/concoursepicker/application` | GET / PUT `{"app_id":"id"}` |
| Update-set options/switch | `/api/now/ui/concoursepicker/updateset` | GET / PUT `{"sysId":"id","name":"picker label"}` |

These are internal UI contracts, not stable public REST APIs. The instance's
served `/uxab/shell-bundle.jsdbx` supplied the mutation methods and payloads.
Unsupported required shapes fail explicitly. Optional metadata stays unknown.

The CLI authorizes mutation before reads, resolves a single accessible option,
sends one PUT, and independently reads back all picker contexts. Failed mutation
or verification never triggers an automatic retry or rollback. A freshly
verified already-current option needs no PUT: this instance rejects redundant
selection of its current Default update set. Read-only mode permits attachment
and inspection but excludes form login and switching.

## Live evidence: dev404396, 2026-10-04

Native cookie-only authentication identified admin. Global domain switched to
TOP and back. Global application switched to `x_471912_ai_ux` and back; its
Default update set changed with the application. A temporary global update set
verified update-set switching and restoration to Default, then was deleted.
The original domain, application, and update set were restored. The compiled
Rust client also exercised login, all three switches, restoration, inspection,
and a Table API read using the same session.

This does not prove restricted tenant isolation or REST update-set capture.
No plugin or Scripted REST resource was installed, no elevation or impersonation
was performed, and no background script was used. Mock tests cover credential
persistence/rotation, cookie-only login and attachment, independent verification,
expiry, origin isolation, redaction, already-current selection, and read-only policy.
