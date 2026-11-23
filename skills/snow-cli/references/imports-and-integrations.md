# Imports & Integrations

Use when answering: "How does data get imported into table `T`?", "Why did an Import Set / Transform Map fail or duplicate rows?", or "Which inbound/outbound REST APIs or MID/ECC integrations touch `T`?"

All inspection commands below are read-only and work with `snow-cli-ro`. Only `import-set load` and `api post/put/delete` mutate the instance.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `sys_transform_map` | Transform Maps (staging → target) | `sys_id,name,source_table,target_table,active,order,run_business_rules,enforce_mandatory_fields,copy_empty_fields,sys_scope` |
| `sys_transform_entry` | Field mappings & coalesce keys | `map,source_field,target_field,coalesce,coalesce_empty_fields,coalesce_case_sensitive,use_source_script,choice_action` |
| `sys_transform_script` | Transform event scripts (`onStart`, `onBefore`, `onAfter`, `onComplete`) | `map,when,order,active,script` |
| `sys_data_source` | Data Sources feeding staging tables | `name,type,format,import_set_table_name,file_retrieval_method,sys_scope` |
| `sys_import_set` | Import Set batches | `sys_id,number,table_name,state,mode,data_source.name,sys_created_on` |
| `sys_import_set_run` | Transform run history & counts | `sys_id,set.number,sys_transform_map.name,state,total,inserts,updates,ignored,errors,skipped,sys_created_on` |
| `<staging_table>` (extends `sys_import_set_row`) | Individual staged rows & errors | `sys_import_set,sys_import_row,sys_import_state,sys_import_state_comment,sys_target_table,sys_target_sys_id` |
| `sys_ws_definition` | Scripted REST APIs (inbound) | `sys_id,name,service_id,namespace,base_uri,active,sys_scope` |
| `sys_ws_operation` | Scripted REST Resources / routes | `sys_id,name,web_service_definition.name,http_method,operation_uri,active,requires_authentication,requires_acl_authorization` |
| `sys_rest_message` / `sys_rest_message_fn` | Outbound REST Messages & HTTP methods | `name,rest_endpoint` / `rest_message.name,function_name,http_method,rest_endpoint` |
| `sys_outbound_http_log` | Outbound HTTP request/response logs | `method,url,response_status,response_time,source_table,source_record,sys_created_on` |
| `ecc_queue` | MID Server / async integration queue | `sys_id,agent,topic,name,queue,state,source,error_string,sys_created_on` |

## 2. Inspect Import Sets & Transform Maps for a target table

```bash
# 1. Find Transform Maps targeting a table (e.g. sys_user or cmdb_ci)
snow-cli-ro table list sys_transform_map \
  --query 'target_table=sys_user^active=true' \
  --fields sys_id,name,source_table,target_table,order,run_business_rules,enforce_mandatory_fields \
  --order-by order --limit 50

# 2. Inspect field mappings and coalesce keys for a map
snow-cli-ro table list sys_transform_entry \
  --query 'map=<map_sys_id>' \
  --fields source_field,target_field,coalesce,use_source_script,choice_action --limit 100

# 3. Inspect transform scripts (onBefore/onAfter/etc.)
snow-cli-ro table list sys_transform_script \
  --query 'map=<map_sys_id>^active=true' \
  --fields sys_id,when,order,active --order-by when,order

snow-cli-ro table get sys_transform_script <script_sys_id> --fields script --raw

# 4. Inspect staging table columns (must match payload keys, NOT target table columns!)
snow-cli-ro table schema <staging_table> --own-only

# 5. Inspect recent transform runs and failed staging rows
snow-cli-ro table list sys_import_set_run \
  --query 'sys_transform_map=<map_sys_id>' \
  --fields set.number,state,total,inserts,updates,ignored,errors,skipped,sys_created_on \
  --order-by -sys_created_on --limit 10

snow-cli-ro table list <staging_table> \
  --query 'sys_import_set.number=<ISET_number>^sys_import_state=error' \
  --fields sys_import_row,sys_import_state,sys_import_state_comment,sys_target_sys_id --limit 20
```

## 3. Inspect Scripted REST APIs, Outbound REST & ECC Queue

```bash
# Inbound Scripted REST APIs and their operations
snow-cli-ro table list sys_ws_definition \
  --query 'active=true' \
  --fields sys_id,name,service_id,base_uri,sys_scope.scope --limit 50

snow-cli-ro table list sys_ws_operation \
  --query 'web_service_definition=<ws_def_sys_id>^active=true' \
  --fields sys_id,name,http_method,operation_uri,requires_authentication,requires_acl_authorization

snow-cli-ro table get sys_ws_operation <op_sys_id> --fields operation_script --raw

# Outbound REST definitions & recent failed calls
snow-cli-ro table list sys_rest_message_fn \
  --fields sys_id,rest_message.name,function_name,http_method,rest_endpoint --limit 50

snow-cli-ro table list sys_outbound_http_log \
  --query 'response_status>=400' \
  --fields method,url,response_status,response_time,source_table,source_record,sys_created_on \
  --order-by -sys_created_on --limit 20

# ECC Queue errors (MID Server / probes / integrations)
snow-cli-ro table list ecc_queue \
  --query 'state=error' \
  --fields sys_id,agent,topic,name,queue,state,error_string,sys_created_on \
  --order-by -sys_created_on --limit 20
```

## 4. Loading a row via `import-set load` (write op — only when asked)

Before running `snow-cli import-set load <staging_table>`, always verify the staging table's `sys_transform_entry` source field names and coalesce keys first:

```bash
snow-cli import-set load imp_user \
  --data '{"user_id":"jdoe","email":"jdoe@example.com","first_name":"John","last_name":"Doe"}'
```

- `/api/now/import/<table>` stages the row **and runs the transform map synchronously** in one call.
- Row-level errors exit `1` (`IMPORT_SET_ROW_ERRORS`) while still printing the detailed report on stdout. **Do not blindly rerun** after a failure — partial changes are not rolled back; read the stdout report first.

## 5. Gotchas

- **Staging keys ≠ target keys:** Payload keys for `import-set load` must match the staging table / `sys_transform_entry.source_field` names (e.g. `imp_user` expects `user_id`/`email` and coalesces on `email`, not `user_name`).
- **`run_business_rules=false`:** If a Transform Map has `run_business_rules=false`, target-table Business Rules and engines do not run during import!
- **Coalesce duplicates:** If `coalesce=true` is missing on the expected key (or `coalesce_empty_fields=false` with blank source values), transforms insert duplicate target records instead of updating.
- **Reserved word `NULL`:** All-caps `"NULL"` in an import payload explicitly clears the target field in ServiceNow transform maps.
- **CSV unsupported on `import-set load`:** Pass `--output json` if CSV is configured as default.
