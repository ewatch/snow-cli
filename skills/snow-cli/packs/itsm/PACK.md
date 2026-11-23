# ITSM Workflows Pack (`snow-cli`)

Opt-in pack for diagnosing Incident, Problem, Change Management, SLAs, Approvals, and Assignment / Data Lookup rules.

All commands below are read-only and use `snow-cli-ro`.

## 1. Change Management & State Models

Modern ServiceNow Change Management uses **Change Models** (`chg_model`) and `ChangeRequestStateHandler` / `ChangeRequestStateModel_*` Script Includes to validate state transitions:

```bash
# 1. Inspect a Change Request with its model, type, state, and approval status
snow-cli-ro table get change_request CHG0030001 \
  --fields sys_id,number,type,chg_model,state,approval,on_hold,assignment_group \
  --display-value both

# 2. List active Change Models
snow-cli-ro table list chg_model \
  --query 'active=true' \
  --fields sys_id,name,default_change_model,table_name --limit 30

# 3. Inspect model-defined state transitions (conditions live in sttrm_transition_condition)
snow-cli-ro table list sttrm_state_transition \
  --query 'from_state.sttrm_model=<chg_model_sys_id>' \
  --fields sys_id,from_state.state_label,to_state.state_label,automatic \
  --limit 50

# 4. Trace scripted state-handler & model logic in one call
snow-cli-ro include show ChangeRequestStateHandler ChangeRequestStateModel_normal --chain --source-of canMoveTo,_resetModel

# 5. Inspect the Change flow (e.g. Authorize / Assess / Implement)
snow-cli-ro flow show 'Change - Normal - Authorize'
```

## 2. SLAs (`contract_sla` & `task_sla`)

Use when answering: "Why didn't an SLA attach, pause, or stop on task X?"

```bash
# 1. Inspect attached Task SLAs for a record (e.g. Incident sys_id)
snow-cli-ro table list task_sla \
  --query 'task=<task_sys_id>' \
  --fields sys_id,sla.name,stage,active,has_breached,percentage,business_percentage,start_time,planned_end_time,pause_time \
  --display-value both --limit 20

# 2. Inspect active SLA definitions for a table
snow-cli-ro table list contract_sla \
  --query 'collection=incident^active=true' \
  --fields sys_id,name,type,duration,schedule.name,retroactive,start_condition,pause_condition,stop_condition,cancel_condition,reset_condition \
  --limit 50
```

## 3. Approvals (`sysapproval_approver` & `sysapproval_group`)

Use when answering: "Who is blocking approval on this Change/RITM?" or "Why did an approval skip/auto-approve?"

```bash
# Individual approver records (sysapproval points to Task records; document_id is used for non-Task tables)
snow-cli-ro table list sysapproval_approver \
  --query 'sysapproval=<task_sys_id>' \
  --fields sys_id,approver.name,approver.user_name,state,group.assignment_group.name,comments,sys_updated_on \
  --display-value both --limit 50

# Group approval summaries
snow-cli-ro table list sysapproval_group \
  --query 'parent=<task_sys_id>' \
  --fields sys_id,assignment_group.name,approval,sys_updated_on --display-value both --limit 20

# Approval rules (classic rule-based approvals)
snow-cli-ro table list sysapproval_rule \
  --query 'tableINchange_request,incident,sc_req_item^active=true' \
  --fields sys_id,name,table,condition,order --order-by order --limit 30
```

## 4. Assignment & Data Lookup Rules (Priority / Auto-Assignment)

```bash
# Classic Assignment Rules (run in Before engines before Before BRs >= 1000)
snow-cli-ro table list sysrule_assignment \
  --query 'tableINincident,task^active=true' \
  --fields sys_id,name,table,order,condition,group.name,user.name,script --order-by order --limit 50

# Data Lookup Definitions (e.g. Incident Priority = Impact x Urgency, Assignment Lookup)
snow-cli-ro table list dl_definition \
  --query 'source_tableINincident,task^active=true' \
  --fields sys_id,name,source_table,matcher_table,run_on_form_change,run_on_insert,run_on_update --limit 20

# Inspect Priority Lookup matrix rows
snow-cli-ro table list dl_u_priority \
  --fields impact,urgency,priority,order --order-by order --display-value both --limit 30
```

## 5. Gotchas

- **Classic `type` vs `chg_model`:** Newer instances drive Change states from `chg_model` (extends `sttrm_model`), whose states and transitions live in the shared `sttrm_state` / `sttrm_state_transition` / `sttrm_transition_condition` tables (there is no `chg_model_state_transition`), plus Change Approval Policies, whereas legacy/classic changes fall back to `ChangeRequestStateModel_<type>` Script Includes. Always check `chg_model` first.
- **SLA `retroactive=true`:** Retroactive SLAs anchor `start_time` to an earlier timestamp (e.g. `sys_created_on`) and can appear breached immediately on attachment.
- **Approval `sysapproval` vs `document_id`:** For Task subclasses (`change_request`, `sc_req_item`), filter `sysapproval=<sys_id>`. For non-Task tables, approvals link via `document_id=<sys_id>^source_table=<table>`.
- **Assignment rules only fill empty targets by default:** Classic `sysrule_assignment` typically runs only when `assigned_to` / `assignment_group` is empty unless scripted otherwise.
