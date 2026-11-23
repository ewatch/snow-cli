# Security Operations (SecOps) Pack (`snow-cli`)

Opt-in pack for inspecting Security Incident Response (`SIR`) and Vulnerability Response (`VR`) records, observables, security assignments, and diagnosing SecOps scoped access & Restricted Caller Access (`RCA`).

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Module | Table | What it stores | Key fields to project |
|---|---|---|---|
| SIR | `sn_si_incident` | Security Incidents (extends `task`) | `sys_id,number,short_description,state,priority,category,subcategory,assignment_group.name,assigned_to.name,affected_user.name,cmdb_ci.name,active` |
| SIR | `sn_si_task` | Security Response Tasks | `sys_id,number,parent.number,short_description,state,assignment_group.name,assigned_to.name` |
| SIR | `sn_ti_observable` | Threat Intelligence Observables (IP, hash, URL, domain) | `sys_id,value,type.name,finding,sighting_count` |
| SIR | `sn_si_m2m_task_observable` | M2M linking Security Incidents to Observables | `task.number,observable.value,observable.type.name,observable.finding` |
| VR | `sn_vul_vulnerable_item` | Vulnerable Items (VITs: CI × Vulnerability) | `sys_id,number,state,risk_rating,risk_score,cmdb_ci.name,vulnerability.id,assignment_group.name,active` |
| VR | `sn_vul_entry` | Vulnerability catalog entries (CVEs / QIDs) | `sys_id,id,summary,cvss_base_score,severity,source` |
| VR | `sn_vul_m2m_vul_group_item` | Remediation Tasks / Vulnerability Groups ↔ VITs | `sn_vul_vulnerability.number,sn_vul_vulnerable_item.number` |
| Access | `sys_restricted_caller_access` | Cross-scope RCA grants (active by default in SIR!) | `source_scope.scope,target_scope.scope,source,target,operation,status` |

## 2. Inspect a Security Incident & its Observables / Tasks

```bash
# 1. Inspect a Security Incident
snow-cli-ro table get sn_si_incident SIR0010001 \
  --fields sys_id,number,short_description,state,priority,category,assignment_group.name,assigned_to.name,cmdb_ci.name \
  --display-value both

# 2. List child Security Response Tasks
snow-cli-ro table list sn_si_task \
  --query 'parent=<sir_sys_id>' \
  --fields sys_id,number,short_description,state,assignment_group.name,assigned_to.name \
  --display-value both --limit 30

# 3. List linked Observables (IOCs)
snow-cli-ro table list sn_si_m2m_task_observable \
  --query 'task=<sir_sys_id>' \
  --fields observable.value,observable.type.name,observable.finding --limit 50
```

## 3. Inspect Vulnerability Response (VR) hotspots

```bash
# 1. Count open Vulnerable Items by risk_rating and assignment_group (no row downloads!)
snow-cli-ro table stats sn_vul_vulnerable_item \
  --query 'active=true' \
  --group-by risk_rating,assignment_group.name

# 2. Inspect high-risk open VITs for a specific CI
snow-cli-ro table list sn_vul_vulnerable_item \
  --query 'cmdb_ci=<ci_sys_id>^active=true' \
  --fields number,state,risk_rating,risk_score,vulnerability.id,assignment_group.name \
  --order-by -risk_score --display-value both --limit 25

# 3. Look up a CVE / vulnerability entry
snow-cli-ro table list sn_vul_entry \
  --query 'idLIKELog4j^ORidLIKECVE-2024' \
  --fields sys_id,id,severity,cvss_base_score,source --limit 20
```

## 4. Diagnose SecOps access & RCA blocks

```bash
# Check denied/requested RCA records targeting Security Incident Response or VR scopes
snow-cli-ro table list sys_restricted_caller_access \
  --query 'target_scope.scopeINsn_si,sn_vul^status!=2' \
  --fields source_scope.scope,target_scope.scope,source,target,operation,status --display-value both --limit 50

# Check SecOps ACLs (watch admin_overrides=false and Deny-Unless rules)
snow-cli-ro table list sys_security_acl \
  --query 'active=true^type=record^operationINread,write^nameINsn_si_incident,sn_vul_vulnerable_item' \
  --fields sys_id,name,operation,decision_type,admin_overrides,condition,advanced --limit 50
```

## 5. Gotchas

- **Platform `admin` is often blocked from SecOps data:** Security Incident Response (`sn_si`) and Vulnerability Response (`sn_vul`) frequently set `admin_overrides=false` and require scoped roles (`sn_si.admin`/`sn_si.analyst`, `sn_vul.admin`/`sn_vul.vulnerability_analyst`).
- **RCA is active by default for SIR:** Cross-scope scripts reading/writing `sn_si_*` tables fail until approved in `sys_restricted_caller_access`.
- **`sn_vul_vulnerable_item` can have millions of rows:** Always start with `table stats` or filter by `active=true` + `cmdb_ci` / `assignment_group` / `vulnerability` + `--limit`.
- **Keep IOCs & incident narratives bounded:** Use explicit `--fields` and `--preview` if descriptions/notes contain large raw log dumps or threat-intel payloads.
