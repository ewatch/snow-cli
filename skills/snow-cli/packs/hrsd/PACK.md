# HR Service Delivery (HRSD) Pack (`snow-cli`)

Opt-in pack for inspecting HR Cases across Centers of Excellence (COEs), HR Services, HR Profiles, HR Criteria, and diagnosing HRSD's strict scoped security (`admin_overrides=false`, COE Security Policies, and Restricted Caller Access).

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `sn_hr_core_case` | Base HR Case table (parent of COE tables) | `sys_id,number,sys_class_name,short_description,state,hr_service.name,subject_person.name,opened_for.name,assignment_group.name,active` |
| `sn_hr_core_service` | HR Service definitions (maps service → COE, template, flow, fulfillment) | `sys_id,name,value,topic_detail.name,service_table,template.name,le_type,active` |
| `sn_hr_core_profile` | Sensitive employee HR Profiles (1:1 with `sys_user`) | `sys_id,user.user_name,user.name,employment_type,employment_start_date,employment_end_date` |
| `sn_hr_core_criteria` | HR Criteria (audience targeting for HR services/content) | `sys_id,name,active,match_all` |
| `sn_hr_core_coe_security_policy` | COE Security Policies (read/write overrides per COE & group) | `sys_id,name,coe,type,applies_to_all_services,active` |
| `sn_hr_core_task` | HR Tasks under an HR Case | `sys_id,number,parent.number,short_description,state,hr_task_type,assigned_to.name` |
| `sys_restricted_caller_access` | Cross-scope RCA grants (active by default in HRSD!) | `source_scope.scope,target_scope.scope,source,target,operation,status` |

## 2. Discover COE tables & inspect an HR Case

```bash
# 1. List all HR Case COE subclasses (Payroll, Benefits, Relations, Workforce Admin, etc.)
snow-cli-ro table find sn_hr_core_case --limit 30

# 2. Inspect an HR Case and note its concrete COE table (sys_class_name) and HR Service
snow-cli-ro table get sn_hr_core_case HRC0001001 \
  --fields sys_id,number,sys_class_name,short_description,state,hr_service,hr_service.name,assignment_group.name \
  --display-value both

# 3. Inspect the HR Service definition driving this case
snow-cli-ro table get sn_hr_core_service <hr_service_sys_id> \
  --fields name,value,service_table,topic_detail.name,template.name,active

# 4. List child HR Tasks for the case
snow-cli-ro table list sn_hr_core_task \
  --query 'parent=<case_sys_id>' \
  --fields sys_id,number,short_description,state,hr_task_type,assigned_to.name \
  --display-value both --limit 30
```

## 3. Diagnose HRSD access & cross-scope blocks

```bash
# 1. Check COE Security Policies on the specific COE table
snow-cli-ro table list sn_hr_core_coe_security_policy \
  --query 'coe=sn_hr_core_case_payroll^active=true' \
  --fields sys_id,name,coe,type,applies_to_all_services --limit 30

# 2. Check ACLs on the COE table & base sn_hr_core_case (watch admin_overrides=false and decision_type!)
snow-cli-ro table list sys_security_acl \
  --query 'active=true^type=record^operationINread,write^nameINsn_hr_core_case,sn_hr_core_case_payroll' \
  --fields sys_id,name,operation,decision_type,admin_overrides,condition,advanced --limit 50

# 3. Check denied or requested Restricted Caller Access records targeting sn_hr_core
snow-cli-ro table list sys_restricted_caller_access \
  --query 'target_scope.scope=sn_hr_core^status!=2' \
  --fields source_scope.scope,target_scope.scope,source,target,operation,status --display-value both --limit 50
```

## 4. Gotchas

- **Platform `admin` does NOT automatically see HR data:** Most HRSD ACLs set `admin_overrides=false` and require scoped HR roles (`sn_hr_core.admin`, `sn_hr_core.basic`, `sn_hr_core.case_reader`, etc.) plus matching COE Security Policies. If `snow-cli` returns 403 or empty results as `admin`, check HR roles first!
- **Never project sensitive PII unnecessarily:** On `sn_hr_core_profile` and HR cases, always pass explicit minimal `--fields` (never `--fields '*'`) to avoid pulling SSN, compensation, or medical fields into context.
- **RCA is enforced by default in HRSD:** Any script outside `sn_hr_core` touching HR tables or script includes requires an approved `sys_restricted_caller_access` record in `sn_hr_core`.
- **Query ACLs (`query_match` / `query_range`) are common on `sn_hr_core_profile`:** Range queries on sensitive fields like SSN or salary may be blocked even when record read succeeds.
