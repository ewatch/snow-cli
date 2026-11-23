# Flow Designer & Legacy Workflow Deep Dive Pack (`snow-cli`)

Opt-in pack for going beyond `snow-cli-ro flow show`: inspecting Flow Designer step inputs, live/failed execution contexts (`sys_flow_context`), and tracing classic/legacy Workflows (`wf_workflow` / `wf_context`).

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Engine | Table | What it stores | Key fields to project |
|---|---|---|---|
| Flow Designer | `sys_hub_flow` | Flow / Subflow definitions | Prefer `snow-cli-ro flow show <name_or_sys_id>` first |
| Flow Designer | `sys_hub_action_instance` | Action steps inside a flow (v1) | `sys_id,flow,action_type.name,order,comment,action_inputs` |
| Flow Designer | `sys_hub_sub_flow_instance` | Subflow calls inside a flow | `sys_id,flow,subflow.name,order,comment` |
| Flow Designer | `sys_flow_context` | Flow execution runs & state | `sys_id,name,flow.name,state,source_table,source_record,sys_created_on,sys_updated_on` |
| Flow Designer | `sys_flow_log` | Per-execution Flow error/diagnostic log rows | `context,level,message,operation,sys_created_on` |
| Legacy Workflow | `wf_workflow` | Workflow header records | `sys_id,name,table,description,sys_scope` |
| Legacy Workflow | `wf_workflow_version` | Published / checked-out versions | `sys_id,name,workflow,table,published,condition,start,sys_updated_on` |
| Legacy Workflow | `wf_activity` | Activities (nodes) in a workflow version | `sys_id,name,activity_definition.name,workflow_version,stage,x,y` |
| Legacy Workflow | `wf_transition` | Edges & conditions between activities | `from.name,to.name,condition.name,condition.condition` |
| Legacy Workflow | `wf_context` | Runtime workflow execution for a record | `sys_id,workflow_version.name,table,id,state,started,ended,result` |
| Legacy Workflow | `wf_history` | Step-by-step execution trace of a `wf_context` | `context,activity.name,state,result,started,ended,activity_index` |

## 2. Flow Designer: structure first, then execution & inputs

```bash
# 1. Always start with the compact tree (~2 KB)
snow-cli-ro flow show 'Change - Normal - Authorize'

# 2. Check recent or failed runtime executions for a flow or a specific record
snow-cli-ro table list sys_flow_context \
  --query 'source_record=<record_sys_id>' \
  --fields sys_id,name,flow.name,state,source_table,sys_created_on,sys_updated_on \
  --order-by -sys_created_on --limit 20

snow-cli-ro table list sys_flow_context \
  --query 'state=ERROR' \
  --fields sys_id,name,flow.name,source_table,source_record,sys_created_on \
  --order-by -sys_created_on --limit 20

# 3. Read execution error logs for a failed sys_flow_context
snow-cli-ro table list sys_flow_log \
  --query 'context=<flow_context_sys_id>' \
  --fields sys_created_on,level,operation,message \
  --order-by sys_created_on --limit 50

# 4. Inspect action step inputs when flow show isn't detailed enough
snow-cli-ro table list sys_hub_action_instance \
  --query 'flow=<flow_sys_id>' \
  --fields sys_id,action_type.name,order,comment --order-by order --limit 50

# Variable values (inputs) for a specific action instance
snow-cli-ro table list sys_variable_value \
  --query 'document=sys_hub_action_instance^document_key=<action_instance_sys_id>' \
  --fields variable.label,variable.element,value --limit 50
```

## 3. Legacy Workflow: published version → activities/transitions → runtime trace

```bash
# 1. Find active published legacy workflows for a table
snow-cli-ro table list wf_workflow_version \
  --query 'tableINchange_request,sc_req_item^published=true' \
  --fields sys_id,name,table,condition,start,sys_updated_on --limit 30

# 2. Inspect activities and transitions of a published version
snow-cli-ro table list wf_activity \
  --query 'workflow_version=<version_sys_id>' \
  --fields sys_id,name,activity_definition.name,stage --limit 100

snow-cli-ro table list wf_transition \
  --query 'from.workflow_version=<version_sys_id>' \
  --fields from.name,to.name,condition.name,condition.condition --limit 100

# 3. Read a Run Script activity's script (stored in sys_variable_value)
snow-cli-ro table list sys_variable_value \
  --query 'document=wf_activity^document_key=<activity_sys_id>^variable.element=script' \
  --fields variable.element,value

# 4. Trace a live/completed workflow execution on a record (id = target record sys_id)
snow-cli-ro table list wf_context \
  --query 'id=<record_sys_id>' \
  --fields sys_id,workflow_version.name,state,started,ended,result \
  --order-by -started --limit 10

snow-cli-ro table list wf_history \
  --query 'context=<wf_context_sys_id>' \
  --fields activity_index,activity.name,state,result,started,ended \
  --order-by activity_index --limit 100
```

## 4. Gotchas

- **Don't start by querying `sys_hub_*` tables directly:** `snow-cli-ro flow show` already walks `sys_hub_flow`, triggers, logic, actions, subflows, snapshots, and `_v2` tables cleanly in one command. Only drop down to `sys_hub_action_instance` / `sys_variable_value` when you need raw step inputs.
- **Flow reporting / `sys_flow_log` may be off in production:** ServiceNow disables detailed flow context reporting by default on some instances (`com.snc.process_flow.reporting.level` property) — an empty `sys_flow_log` does not necessarily mean the flow didn't run; check `sys_flow_context.state` first.
- **`_v2` Flow Designer tables store inputs compressed:** If `flow show` reports `"source": "v2"`, raw `sys_hub_action_instance_v2` inputs are compressed payloads rather than plain `sys_variable_value` rows.
- **Multiple `wf_workflow_version` rows exist per workflow:** Always filter `published=true` when inspecting what actually runs in production (unpublished drafts remain in the same table).
- **Legacy Workflow scripts live in `sys_variable_value`, not `wf_activity`:** Activity configuration (scripts, conditions, catalog task fields) is stored in `sys_variable_value` with `document=wf_activity^document_key=<wf_activity_sys_id>`.
