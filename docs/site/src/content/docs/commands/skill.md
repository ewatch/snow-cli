# `skill`

Use `skill` to install the `snow-cli` agent skill bundle—either offline from the
binary itself (`--bundled`), from a local directory, or from a hosted
`skill.toml` manifest URL—into your coding agent's skills directory.

```bash
snow-cli skill install [--bundled | <source>] [options]
```

All `skill` subcommands also accept the global flags from the [command overview](/commands/).

## `skill install`

### Choose a source

Pick one source (mutually exclusive):

- `--bundled`: install the skill bundle embedded in this `snow-cli` binary.
  Works offline and always matches the CLI version you have installed.
- `<source>` (local path or `file://` URL): path to a skill directory (for
  example `./skills/snow-cli`) or directly to its `skill.toml`.
- `<source>` (`http://` or `https://` URL): URL to a hosted `skill.toml`
  manifest, such as
  `https://ewatch.github.io/snow-cli/skills/snow-cli/skill.toml`. Listed files
  are fetched relative to that manifest URL and verified against their declared
  SHA-256 digests.

### Choose a target

Pick either `--target` or `--target-dir` (mutually exclusive):

| Flag | Install directory |
|---|---|
| `--target claude` | `~/.claude/skills/<name>` |
| `--target codex` | `~/.codex/skills/<name>` |
| `--target agents` | `~/.agents/skills/<name>` |
| `--target-dir <dir>` | `<dir>/<name>` (for project-local or custom roots) |

`<name>` defaults to the last segment of `name` in `skill.toml` (`snow-cli`)
and can be overridden with `--name <name>`.

### Select optional packs

The bundle always installs its core entrypoint (`SKILL.md`), `skill.toml`, and
the 12 platform reference guides under `references/`. Vertical domain packs
under `packs/<name>/` are opt-in:

- **Core only (default)**: omit `--pack` and `--all-packs` to install core files
  and zero packs.
- **Specific packs**: pass `--pack <name>` once per pack you want.
- **All packs**: pass `--all-packs` to install every pack declared by the
  bundle.
- **Preview without writing**: add `--dry-run` to inspect the planned file list,
  digests, and per-file actions (`create`, `overwrite`, `unchanged`, `conflict`,
  `remove`, or `keep_modified`) without touching disk.
- **Force overwrite/removal of local edits**: add `--force` to overwrite or
  prune files whose on-disk digest differs from the previous install lockfile.

## Examples

Install the core skill offline for Claude Code:

```bash
snow-cli skill install --bundled --target claude
```

Preview a selective pack install before writing anything:

```bash
snow-cli skill install --bundled --target claude --pack itsm --pack cmdb --dry-run
```

Install selected packs into a project-local skills directory:

```bash
snow-cli skill install --bundled --target-dir ./.claude/skills --pack itsm --pack workflows
```

Install from the hosted GitHub Pages bundle with every pack for Codex:

```bash
snow-cli skill install https://ewatch.github.io/snow-cli/skills/snow-cli/skill.toml --target codex --all-packs
```

Install from a local repository checkout:

```bash
snow-cli skill install ./skills/snow-cli --target-dir ./.codex/skills --pack itsm
```

Example output (`--dry-run`):

```json
{
  "status": "dry_run",
  "dry_run": true,
  "name": "snow/snow-cli",
  "version": "0.11.0",
  "source": "bundled",
  "install_dir": "/Users/you/.claude/skills/snow-cli",
  "packs": ["cmdb", "itsm"],
  "files": [
    {"path": "SKILL.md", "sha256": "…", "action": "create"},
    {"path": "packs/cmdb/PACK.md", "sha256": "…", "action": "create"},
    {"path": "packs/itsm/PACK.md", "sha256": "…", "action": "create"},
    {"path": "references/acls.md", "sha256": "…", "action": "create"}
  ],
  "lockfile": "/Users/you/.claude/skills/snow-cli/snow-cli-skill.lock.toml"
}
```

## What is in the bundled skill

### Core platform references (always installed)

`SKILL.md` stays compact and routes the agent on demand to 12 read-only-first
guides under `references/`:

- `acls.md` — ACL evaluation order, `sys_security_acl`, role inheritance, query Business Rules
- `auth-and-identity.md` — users, groups, roles, OAuth/SSO/API auth diagnostics
- `automation-trace.md` — tracing Business Rules, Script Includes, Client Scripts, UI Policies, Flows, and Workflows
- `data-and-attachments.md` — `data export`/`import`, `attachment` workflows, `sys_attachment`
- `data-model.md` — `sys_db_object`, `sys_dictionary`, `sys_choice`, table inheritance, reference chains
- `diagnostics-and-logs.md` — `syslog`, `sysevent`, `sys_trigger`, `sys_audit`, `ecc_queue`
- `domain-separation.md` — `domain` discovery, `sys_domain`, `sys_overrides`, visibility checks
- `imports-and-integrations.md` — Import Sets, Transform Maps, REST/SOAP messages, MID Servers, IntegrationHub
- `notifications.md` — `sysevent_email_action`, `sys_email`, email templates, scripts, and inbound actions
- `scopes-and-updates.md` — `scope` inspection, `sys_update_set`, `sys_update_xml`, `sys_metadata`
- `servicenow-docs-lookup.md` — targeted lookup workflow for local ServiceNow documentation repos
- `ui-and-portal.md` — Service Portal (`sp_*`), UI Actions, UI Pages/Macros, Workspace/UI Builder (`sys_ux_*`)

### Optional domain packs (`--pack <name>`)

| Pack | Focus |
|---|---|
| `itsm` | Change State Models, SLAs, Approvals, Assignment and Data Lookup rules |
| `catalog` | Catalog items, variables, variable sets, catalog scripts/policies, REQ/RITM/SCTASK |
| `cmdb` | CI hierarchy, relationships, IRE identification/reconciliation rules, duplicate audits |
| `workflows` | Flow Designer step inputs/contexts/logs and legacy `wf_workflow` tracing |
| `knowledge` | Knowledge articles, versions, Can/Cannot Read User Criteria, KB health |
| `snu` | SN-Utils browser bridge: live SSO/MFA sessions, form state, screenshots, broker diagnostics |
| `reporting` | `sys_report` sharing, dashboards, Performance Analytics indicators, cubes, scores, and jobs |
| `csm` | Cases, Accounts, Contacts, Entitlements, and external visibility diagnosis |
| `hrsd` | HR COE cases, HR Services, HR Profiles, COE Security Policies, and RCA access blocks |
| `secops` | Security Incident Response (SIR), observables, Vulnerability Response (VR), and RCA blocks |
| `plugins` | Plugin & Store app pre-flight checks, CI/CD REST API activation/install/rollback, progress tracking, and `GlidePluginManagerWorker` fallback (guarded admin/write workflows) |

## Notes

- **Safety checks**: installation rejects unsafe or traversal paths (`..`,
  absolute paths, URL escape/query characters, drive/reserved names), refuses
  to read or write through symlinks at any path component (including the
  lockfile), enforces same-origin/same-directory URL resolution, and blocks
  undeclared executable files. Remote HTTP/HTTPS sources must declare
  `[[files]]` with `sha256` digests in `skill.toml` (verifying transfer
  integrity, not cryptographic signature authenticity).
- **Lockfile & automatic stale pack pruning**: successful non-dry-run installs
  write `snow-cli-skill.lock.toml` inside `<install_dir>` recording the source,
  selected packs, and file SHA-256 digests. When you re-install with fewer
  packs (or after a future upgrade removes/renames a file), previously managed
  files that are no longer selected are automatically pruned (`action: "remove"`)
  as long as they have not been locally modified.
- **Local edit protection**: if a file on disk differs from the previous
  lockfile entry, `skill install` refuses to overwrite it (`VALIDATION_ERROR`,
  exit 2; shown as `action: "conflict"` in `--dry-run`) and preserves modified
  dropped-pack files (`action: "keep_modified"`) unless `--force` is passed.
- **Read-only binary**: `skill` is provided by `snow-cli` (not `snow-cli-ro`),
  while the installed skill guides cover both `snow-cli` and `snow-cli-ro` with
  read-only-first defaults.
