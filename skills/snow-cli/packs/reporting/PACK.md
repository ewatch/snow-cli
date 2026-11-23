# Reports, Dashboards & Performance Analytics Pack (`snow-cli`)

Opt-in pack for inspecting Classic Reports, Platform Analytics / Next Experience Dashboards, Performance Analytics (PA) Indicators, Data Collection Jobs, and running fast ad-hoc aggregations via `table stats`.

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Surface | Table | What it stores | Key fields to project |
|---|---|---|---|
| Classic Reports | `sys_report` | Report definitions & queries | `sys_id,title,table,type,filter,field,aggregate,group,user.user_name,roles,is_published,sys_updated_on` |
| Classic Reports | `sys_report_users_groups` | Explicit report sharing (users/groups) | `report_id.title,user_id.user_name,group_id.name` |
| Classic Reports | `sysauto_report` | Scheduled email reports | `name,report.title,active,run_type,user_list,group_list` |
| Dashboards | `pa_dashboards` | Responsive / classic dashboards | `sys_id,name,owner.name,active,visible_to,sys_scope` |
| Dashboards | `par_dashboard` | Platform Analytics / Next Experience dashboards | `sys_id,name,active,certified,sys_updated_by,sys_updated_on,sys_scope` |
| PA Indicators | `pa_indicators` | Automated/formula/manual KPI definitions | `sys_id,name,type,cube.name,frequency,aggregate,unit.name,formula` |
| PA Sources | `pa_cubes` | Indicator Sources (table + conditions) | `sys_id,name,facts_table,conditions,frequency` |
| PA Jobs | `sysauto_pa` | PA Data Collection scheduled jobs | `sys_id,name,active,run_type,score_operator,score_relative_start,score_relative_end` |
| PA Scores | `pa_scores_l1` | Collected daily/period indicator scores | `indicator.name,start_at,value,breakdown.name,element` |

## 2. Inspect reports & verify underlying data directly

```bash
# 1. Find reports on a specific table
snow-cli-ro table list sys_report \
  --query 'table=incident^titleLIKEopen' \
  --fields sys_id,title,table,type,filter,group,aggregate,user.user_name,is_published \
  --order-by -sys_updated_on --limit 30

# 2. Check who a report is shared with
snow-cli-ro table list sys_report_users_groups \
  --query 'report_id=<report_sys_id>' \
  --fields user_id.user_name,group_id.name --limit 50

# 3. Cross-check a report's counts directly via Aggregate API (no row downloads)
snow-cli-ro table stats incident --query 'active=true' --group-by priority --avg reassignment_count
snow-cli-ro table stats incident --query 'active=true' --group-by assignment_group.name --having 'count>10'
```

## 3. Diagnose Performance Analytics (PA) indicators & stale scores

Use when answering: "Why is this KPI blank/stale?" or "How is indicator X calculated?"

```bash
# 1. Find the indicator and its Indicator Source (cube) or formula
snow-cli-ro table list pa_indicators \
  --query 'nameLIKEopen incidents' \
  --fields sys_id,name,type,cube,cube.name,frequency,aggregate,formula --display-value both --limit 20

# 2. Inspect the Indicator Source's facts_table and base filter conditions
snow-cli-ro table get pa_cubes <cube_sys_id> --fields name,facts_table,conditions,frequency

# 3. Check latest collected scores in pa_scores_l1
snow-cli-ro table list pa_scores_l1 \
  --query 'indicator=<indicator_sys_id>^breakdownISEMPTY' \
  --fields start_at,value,indicator.name \
  --order-by -start_at --limit 15

# 4. Check PA data collection jobs and recent job errors in sys_trigger / pa_job_logs
snow-cli-ro table list sysauto_pa \
  --query 'active=true' \
  --fields sys_id,name,run_type,score_relative_start,score_relative_end --limit 30

snow-cli-ro table list pa_job_logs \
  --query 'job=<sysauto_pa_sys_id>' \
  --fields state,run_time,inserts,updates,errors,warnings \
  --order-by -run_time --limit 10
```

## 4. Gotchas

- **Report visibility combines sharing + `report_on` / `report_view` ACLs:** Even if a report is shared in `sys_report_users_groups`, a user who fails `operation=report_on` (table) or `operation=report_view` (field) ACLs sees an access error or missing group-by column.
- **`pa_scores_l1` vs `pa_scores_l2`:** Unbroken-down and 1st-level breakdown scores live in `pa_scores_l1`; 2nd-level (matrix) breakdown scores live in `pa_scores_l2`. Always filter by `indicator=<sys_id>` and `start_at` because score tables can hold millions of rows.
- **`pa_indicators.type`:** Automated indicators pull from `pa_cubes` via `sysauto_pa` jobs; Formula indicators compute on the fly from `formula` referencing other indicator `sys_id`s (double-brackets `[[...]]`).
- **Classic vs Next Experience Dashboards:** Older dashboards live in `pa_dashboards`; Platform Analytics / Workspace dashboards live in `par_dashboard`. Check both when searching by dashboard name.
