# Explore a custom application

Use this recipe to map a scoped custom application end-to-end: discover the
scope, inventory its artifacts, read its code, and follow its recent changes.
The sequence keeps discovery, implementation, and change history connected, and
stays read-only throughout.

## 1. Discover the custom-app scopes

List application scopes classified as custom apps:

```bash
snow-cli --read-only scope list --kind custom-app
```

Note the returned `scope` value (for example `x_2135095_au_demo`) — the later
steps use it.

## 2. Inventory the application's artifacts

Get a normalized count of every artifact type in the scope:

```bash
snow-cli --read-only scope inventory x_2135095_au_demo
```

This tells you how many tables, business rules, script includes, flows, and
scripted REST resources the app defines — a bounded, app-specific next step.

## 3. Read the application's code

Search instance code for a symbol you saw in the inventory. Restrict by source
table to focus on a specific artifact type:

```bash
snow-cli --read-only codesearch search ES12Demo --source-table sys_script_include
```

## 4. Follow the application's recent changes

Read Customer Updates for the same application scope, newest first:

```bash
snow-cli --read-only table list sys_update_xml \
  --query 'application.scope=x_2135095_au_demo^ORDERBYDESCsys_updated_on' \
  --fields name,type,action,sys_updated_on,sys_updated_by,update_set \
  --limit 10
```

## What to look for

- **`scope inventory`** gives you the shape of the app before you read any code,
  so you know whether to look for flows, script includes, or REST APIs.
- **Source-table filters** keep `codesearch` focused on one artifact type.
- The **`application.scope=` query** ties every change back to the app you are
  investigating, ordered by recency.

## Related pages

- [`scope`](/commands/scope/) — list, inspect, and inventory scopes
- [`codesearch`](/commands/codesearch/) — search instance code
- [`table`](/commands/table/) — query Customer Updates and other records
