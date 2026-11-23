# UI, Forms, Service Portal & Next Experience

Use when answering: "Why is a field shown/hidden on a form view?", "Which Service Portal widget renders this page?", or "What Next Experience / UI Builder route handles this URL?"

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Surface | Table | What it stores | Key fields to project |
|---|---|---|---|
| Classic Form | `sys_ui_form` | Form layouts per table & view | `sys_id,name,view.name,sys_scope` |
| Classic Form | `sys_ui_form_section` | Ordered sections on a form | `sys_ui_form,sys_ui_section,position` |
| Classic Form | `sys_ui_section` | Section definitions | `sys_id,name,caption,view.name` |
| Classic Form | `sys_ui_element` | Fields/formatters inside a section | `sys_ui_section,element,type,position` |
| Classic List | `sys_ui_list` / `sys_ui_list_element` | List columns per table & view | `name,view.name,relationship` / `list_id,element,position` |
| Service Portal | `sp_portal` | Portal roots (`url_suffix`, theme, home page) | `title,url_suffix,homepage.id,sc_catalog` |
| Service Portal | `sp_page` | Portal pages by `?id=<page_id>` | `sys_id,id,title,public,sys_scope` |
| Service Portal | `sp_instance` | Widget instances placed on a page | `sys_id,sp_widget.id,sp_widget.name,sp_column,order,title` |
| Service Portal | `sp_widget` | Widget HTML/CSS/client/server code | `sys_id,id,name,data_table,public,sys_scope` |
| Next Experience | `sys_ux_page_registry` | Workspace / UX experience roots | `sys_id,title,path,root_macroponent,active` |
| Next Experience | `sys_ux_app_route` | Routes inside a UX experience | `sys_id,name,route_type,screen_type,app_config,fields` |
| Next Experience | `sys_ux_macroponent` | UI Builder page/component definitions | `sys_id,name,extends,category,sys_scope` |

## 2. Inspect a classic Form layout

```bash
# 1. Find sections for a table (empty view = Default view)
snow-cli-ro table list sys_ui_section \
  --query 'name=incident^viewISEMPTY' \
  --fields sys_id,name,caption,view.name --limit 30

# 2. List ordered fields inside a section
snow-cli-ro table list sys_ui_element \
  --query 'sys_ui_section=<section_sys_id>' \
  --fields position,element,type --order-by position --limit 100

# 3. Check view-specific Client Scripts / UI Policies that hide or lock fields
snow-cli-ro table list sys_ui_policy \
  --query 'tableINincident,task^active=true' \
  --fields sys_id,short_description,view,global,conditions,order --order-by order --limit 50
```

## 3. Trace a Service Portal page → widgets → code

When given a URL like `/sp?id=sc_cat_item`:

```bash
# 1. Find the portal by url_suffix and the page by id
snow-cli-ro table list sp_portal --query 'url_suffix=sp' --fields sys_id,title,url_suffix,homepage.id
snow-cli-ro table list sp_page --query 'id=sc_cat_item' --fields sys_id,id,title,public,sys_scope.scope

# 2. Find all widget instances on that page (dot-walk through sp_column -> sp_row -> sp_container -> sp_page)
snow-cli-ro table list sp_instance \
  --query 'sp_column.sp_row.sp_container.sp_page=<page_sys_id>' \
  --fields sys_id,sp_widget.sys_id,sp_widget.id,sp_widget.name,order,title --order-by order --limit 50

# 3. Inspect a widget's server script, client controller, or HTML template verbatim
snow-cli-ro table get sp_widget <widget_sys_id> --fields script --raw
snow-cli-ro table get sp_widget <widget_sys_id> --fields client_script --raw
snow-cli-ro table get sp_widget <widget_sys_id> --fields template --raw

# 4. Search across all Service Portal widgets at once
snow-cli-ro codesearch search 'c.server.get' --source-table sp_widget --limit 25
```

## 4. Trace a Next Experience / Workspace route

When given a URL like `/now/sow/record/incident/<sys_id>`:

```bash
# 1. Find the experience by path prefix (e.g. "now/sow")
snow-cli-ro table list sys_ux_page_registry \
  --query 'pathLIKEsow^active=true' \
  --fields sys_id,title,path,admin_panel,root_macroponent --limit 20

# 2. List routes for that experience's app_config
snow-cli-ro table list sys_ux_app_route \
  --query 'app_config=<admin_panel_sys_id>' \
  --fields sys_id,name,route_type,screen_type,fields --limit 50

# 3. Find UI Builder screens / macroponents
snow-cli-ro table list sys_ux_macroponent \
  --query 'nameLIKErecord^category=page' \
  --fields sys_id,name,sys_scope.scope --limit 30
```

## 5. Gotchas

- **Default view = empty `view` field:** On `sys_ui_section`, `sys_ui_form`, and `sys_ui_list`, the Default view has `viewISEMPTY` (not `view.name=Default`).
- **UI Policies & Client Scripts can be view-scoped:** Check `global=false` + `view` on `sys_ui_policy` and `sys_script_client` when behavior differs between Default view and Workspace (`sow` / `workspace`) view.
- **Embedded widgets:** Service Portal widgets frequently embed child widgets via `<widget id="...">` or `spUtil.get(...)` — check the widget `template` and `script` if the visible UI isn't directly in `sp_instance`.
- **Workspace Declarative Actions:** Next Experience buttons/actions live in `sys_declarative_action_assignment`, not only classic `sys_ui_action`.
- **Field invisible on form ≠ missing from layout:** A field present in `sys_ui_element` can still be hidden by a `read` ACL, a UI Policy action (`visible=false`), or a `g_form.setDisplay/setVisible` Client Script.
