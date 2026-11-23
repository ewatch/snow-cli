# Navigating Local `ServiceNowDocs` Safely

Use when you need to verify exact ServiceNow platform behavior, release-specific features, API signatures, or table/field semantics against a local clone of `ServiceNow/ServiceNowDocs` (or its raw GitHub URLs).

**Never recursively grep or scan the whole `ServiceNowDocs` repository** — it contains tens of thousands of markdown files across ~58 publications.

## 1. Repository layout

```text
ServiceNowDocs/
├── llms.txt                     # Master index of all publications + branch/release notes
└── markdown/
    ├── <publication>/
    │   ├── index.md             # Table of contents with topic summaries & paths
    │   └── <topic>.md           # Single topic with YAML frontmatter (title, release, canonical_url)
    └── ...
```

## 2. High-signal publications for `snow-cli` work

| Publication folder (`markdown/...`) | Best for |
|---|---|
| `api-reference` | Table API, Aggregate API, Import Set API, Glide* server/client APIs, Scripted REST |
| `application-development` | Scopes, Cross-Scope Privileges, RCA, Update Sets, Fluent/SDK table schemas (`servicenow-sdk/`) |
| `platform-security` | ACLs (`access-control/`), Deny-Unless, Query ACLs, roles, hardening properties |
| `build-workflows` | Business Rules (`business-rules-classic/`), Approvals & execution order (`approvals/`), Legacy Workflow |
| `platform-administration` | Email/Notifications, Dictionary, Schedules, Domain Separation, System Properties |
| `servicenow-platform` | CMDB (`configuration-management-database-cmdb/`), Service Catalog (`service-catalog/`), Data Archiving |
| `integrate-applications` | Import Sets, Data Sources, Transform Maps, Integration Hub / Workflow Data Fabric |
| `it-service-management` | Incident, Problem, Change Models (`change-management/`), SLAs |
| `release-notes` / `delta-*` | Release-specific behavioral changes between families |

## 3. Index-first lookup workflow (3 fast steps)

1. **Check the release & publication name** in `llms.txt` (or `git branch --show-current` in the docs repo).
2. **Search only the relevant publication's `index.md`** (1 file, instant) to find the exact topic path:
   ```bash
   grep -iE '<keyword1>|<keyword2>' /path/to/ServiceNowDocs/markdown/<publication>/index.md | head -n 30
   ```
3. **Read only the matched topic file(s)** (prefer `servicenow-sdk/*.md` under `application-development/` when you need exact table names and field lists, because SDK reference docs enumerate concrete schema properties instead of UI click-paths).

## 4. Gotchas

- **Do not fetch `servicenow.com/docs` directly:** It is a JavaScript SPA that returns unreadable output to HTTP clients; always read local `ServiceNowDocs/markdown/...` files or `raw.githubusercontent.com/ServiceNow/ServiceNowDocs/<branch>/markdown/...`.
- **Check the branch vs the target instance build:** Run `snow-cli-ro auth status --verify` to see the instance's `build`/release before assuming a newer feature (such as Deny-Unless ACLs or Query ACLs) exists on that instance.
- **Most docs describe UI navigation:** Translate UI form labels back to real column names on the live instance with `snow-cli-ro table schema <table> --extended` before querying.
