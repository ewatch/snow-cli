# Service Catalog Pack (`snow-cli`)

Opt-in pack for inspecting Service Catalog items, variables, variable sets, catalog client scripts/UI policies, and submitted Requests (`REQ` → `RITM` → `SCTASK`) with their variable values.

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Layer | Table | What it stores | Key fields to project |
|---|---|---|---|
| Definition | `sc_cat_item` | Catalog items & record producers | `sys_id,name,active,category.title,flow_designer_flow.name,workflow.name,sys_class_name,sys_scope` |
| Definition | `item_option_new` | Variables (on an item or inside a variable set) | `sys_id,name,question_text,type,mandatory,order,cat_item,variable_set,default_value,reference,active` |
| Definition | `io_set_item` | M2M linking Variable Sets (`item_option_new_set`) to Catalog Items | `sc_cat_item,variable_set,variable_set.title,variable_set.internal_name,order` |
| Behavior | `catalog_script_client` | Catalog Client Scripts | `sys_id,name,applies_to,cat_item,variable_set,type,cat_variable,applies_req_item,applies_target_record,active` |
| Behavior | `catalog_ui_policy` | Catalog UI Policies | `sys_id,short_description,applies_to,catalog_item,variable_set,catalog_conditions,order,applies_req_item,applies_catalog,active` |
| Behavior | `catalog_ui_policy_action` | Catalog UI Policy field actions | `ui_policy,catalog_variable,mandatory,visible,disabled,cleared` |
| Runtime | `sc_request` | Top-level Request (`REQ…`) | `sys_id,number,requested_for.name,request_state,approval,stage` |
| Runtime | `sc_req_item` | Requested Item (`RITM…`) | `sys_id,number,request.number,cat_item.name,state,stage,approval,assignment_group.name` |
| Runtime | `sc_task` | Catalog Task (`SCTASK…`) | `sys_id,number,request_item.number,state,short_description,assignment_group.name,assigned_to.name` |
| Runtime | `sc_item_option_mtom` | M2M linking a `RITM` to submitted variable answers (`sc_item_option`) | `request_item,sc_item_option.item_option_new.name,sc_item_option.item_option_new.question_text,sc_item_option.value` |

## 2. Inspect a Catalog Item's full design (variables, sets, automation, fulfillment)

```bash
# 1. Find the Catalog Item (or Record Producer: sys_class_name=sc_cat_item_producer)
snow-cli-ro table list sc_cat_item \
  --query 'nameLIKElaptop^active=true' \
  --fields sys_id,name,sys_class_name,flow_designer_flow.name,workflow.name,sys_scope.scope --limit 20

# 2. Direct variables on the item
snow-cli-ro table list item_option_new \
  --query 'cat_item=<cat_item_sys_id>^active=true' \
  --fields sys_id,name,question_text,type,mandatory,order,reference,default_value \
  --order-by order --display-value both --limit 100

# 3. Attached Variable Sets (single-row and multi-row / MRVS)
snow-cli-ro table list io_set_item \
  --query 'sc_cat_item=<cat_item_sys_id>' \
  --fields variable_set,variable_set.title,variable_set.internal_name,variable_set.type,order \
  --order-by order --limit 50

# Variables inside a specific Variable Set
snow-cli-ro table list item_option_new \
  --query 'variable_set=<var_set_sys_id>^active=true' \
  --fields sys_id,name,question_text,type,mandatory,order --order-by order --limit 100

# 4. Catalog Client Scripts & UI Policies (check BOTH cat_item AND each variable_set!)
snow-cli-ro table list catalog_script_client \
  --query 'cat_item=<cat_item_sys_id>^active=true' \
  --fields sys_id,name,type,cat_variable,applies_catalog,applies_req_item,applies_target_record --limit 50

snow-cli-ro table list catalog_ui_policy \
  --query 'catalog_item=<cat_item_sys_id>^active=true' \
  --fields sys_id,short_description,order,catalog_conditions,applies_catalog,applies_req_item \
  --order-by order --limit 50

# 5. Inspect fulfillment Flow (if flow_designer_flow is populated)
snow-cli-ro flow show '<flow_name_or_sys_id>'
```

## 3. Trace a submitted Request (`REQ` → `RITM` → `SCTASK` + variable values)

```bash
# 1. Inspect the RITM
snow-cli-ro table get sc_req_item RITM0010001 \
  --fields sys_id,number,request.number,cat_item.name,state,stage,approval,assignment_group.name \
  --display-value both

# 2. Read all submitted single-row variable answers for that RITM
snow-cli-ro table list sc_item_option_mtom \
  --query 'request_item=<ritm_sys_id>' \
  --fields sc_item_option.item_option_new.name,sc_item_option.item_option_new.question_text,sc_item_option.value,sc_item_option.order \
  --order-by sc_item_option.order --limit 100

# 3. Read Multi-Row Variable Set (MRVS) answers for that RITM
snow-cli-ro table list sc_multi_row_question_answer \
  --query 'parent_id=<ritm_sys_id>' \
  --fields variable_set.internal_name,row_index,item_option_new.name,value \
  --order-by row_index --limit 100

# 4. List child Catalog Tasks (SCTASK)
snow-cli-ro table list sc_task \
  --query 'request_item=<ritm_sys_id>' \
  --fields sys_id,number,state,short_description,assignment_group.name,assigned_to.name \
  --display-value both --limit 30
```

## 4. Gotchas

- **Variable answers don't live as columns on `sc_req_item`:** Single-row answers live via `sc_item_option_mtom` → `sc_item_option`; multi-row variable set (MRVS) rows live in `sc_multi_row_question_answer` (`parent_id=<ritm_sys_id>`).
- **Check Variable Sets for scripts/policies too:** A `catalog_script_client` or `catalog_ui_policy` with `applies_to=set` links to `variable_set`, not `cat_item` — always check attached variable sets from `io_set_item`.
- **Target surface flags matter:** `applies_catalog` (order form), `applies_req_item` (RITM view), and `applies_target_record` (SCTASK/target record) control *where* a catalog script/policy runs.
- **Record Producers (`sc_cat_item_producer`):** Record Producers create target records (e.g. `incident`) instead of `sc_req_item`; their script lives in `sc_cat_item_producer.script` and their variables link via `question_answer` (`table_sys_id=<record_sys_id>`).
