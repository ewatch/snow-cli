# Review a Flow Designer flow

Use this recipe to have a coding agent (or yourself) explain what a Flow
Designer flow does, grounded in the instance's own records rather than the
visual canvas. Every step stays inside the read-only surface.

## 1. Ground the review in the flow table

Read the schema first so later queries reference real columns:

```bash
snow-cli --read-only table schema sys_hub_flow
```

## 2. List the active flows

Pull a bounded set of active flows to find the one you care about:

```bash
snow-cli --read-only table list sys_hub_flow \
  --query 'active=true' \
  --fields name,active,sys_updated_on \
  --limit 10
```

Narrow to a specific flow by name with an encoded query:

```bash
snow-cli --read-only table list sys_hub_flow \
  --query 'name=Business Calendar Demo' \
  --fields sys_id,name,description,active,sys_scope,sys_updated_on
```

## 3. Search the instance code it touches

Flows often call Script Includes or client scripts. Find references to a symbol
across instance code:

```bash
snow-cli --read-only codesearch search GlideAjax --limit 10
```

Limit the search to a source table when you already know where to look:

```bash
snow-cli --read-only codesearch search GlideAjax --source-table sys_script_include
```

## 4. Review recent changes to the flow

Read the most recent Customer Update records, newest first:

```bash
snow-cli --read-only table list sys_update_xml \
  --query 'ORDERBYDESCsys_updated_on' \
  --fields name,type,action,sys_updated_on,update_set \
  --limit 10
```

## What to look for

- **`active` and `sys_scope`** tell you whether the flow runs and which
  application owns it.
- **`description`** on the `sys_hub_flow` record is often the author's own
  summary of intent.
- The **Customer Updates** show when the flow last changed and in which update
  set, connecting behavior to change history.

## Related pages

- [`table`](/commands/table/) — schema, list, and get
- [`codesearch`](/commands/codesearch/) — search instance code
- [Secure read-only usage](/secure-readonly-usage/)
