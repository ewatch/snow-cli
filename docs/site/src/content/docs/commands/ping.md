# `ping`

Use `ping` to confirm, in one cheap request, that the instance is reachable
and accepts the active profile's credentials, and to see who you are
connected as.

```bash
snow-cli ping
```

Sample output:

```json
{
  "instance": "https://dev.service-now.com",
  "user": "admin",
  "user_sys_id": "6816f79cc0a8016401c5a33be04be441",
  "build": "glide-zurich-07-01-2026__patch1",
  "latency_ms": 182
}
```

- `user` and `user_sys_id` come from `sys_user` filtered by
  `javascript:gs.getUserID()`, so they name the session user even for OAuth
  and browser-session profiles. They are `null` if the user row is not
  readable.
- `build` is read on a best-effort basis from `sys_properties`
  (`glide.buildtag`, then `glide.buildtag.last`, `glide.war`,
  `glide.war.assigned`). It is `null` when those properties are not readable,
  which is common for non-admin users.
- `latency_ms` is the round trip of the identity request, including any OAuth
  token acquisition.

Rejected credentials or an unreachable instance produce the usual structured
error on stderr and a non-zero exit code (`5` for API errors such as
`UNAUTHORIZED`).

`ping` only reads data, so it is also available in `snow-cli-ro`.
