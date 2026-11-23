# Data Model, Schema & Overrides

Use when answering: "What tables exist for X?", "What columns, references, choices, or child-table overrides apply to table `T`?", or before writing queries, exports, or imports.

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables & CLI commands

| Command / Table | What it gives you |
|---|---|
| `snow-cli-ro table find <text>` | Tables matching name/label with `extends` and `scope` |
| `snow-cli-ro table schema <table> --extended` | Effective columns (including inherited) with `table`, `type`, `reference`, `mandatory`/`required`, `read_only`, `max_length`, `default` |
| `snow-cli-ro table schema <table> --own-only` | Only columns defined directly on `<table>` |
| `snow-cli-ro table choices <table> [field]` | Active choice values → labels with defining `table` and `dependent_value` |
| `snow-cli-ro table stats <table> --group-by <f>` | Row distribution without downloading records |
| `sys_db_object` | Table metadata: `name,label,super_class.name,is_extendable,access,sys_scope.scope` |
| `sys_dictionary` | Raw column definitions + table collection row (`element=''`) |
| `sys_dictionary_override` | Per-child overrides of inherited columns (default, mandatory, read-only, ref qual, attributes) |
| `sys_number` | Auto-numbering prefixes & padding (`category,prefix,maximum_digits`) |

## 2. Step-by-step exploration

```bash
# 1. Discover tables and inheritance
snow-cli-ro table find change --limit 30
snow-cli-ro table list sys_db_object \
  --query 'name=incident^ORsuper_class.name=incident' \
  --fields name,label,super_class.name,is_extendable,sys_scope.scope

# 2. Inspect effective schema (inherited + own) and verify exact field names
snow-cli-ro table schema incident --extended
snow-cli-ro table schema incident --own-only

# 3. Check Dictionary Overrides on child tables (critical for task/cmdb_ci subclasses!)
snow-cli-ro table list sys_dictionary_override \
  --query 'name=incident' \
  --fields element,default_value_override,default_value,mandatory_override,mandatory,read_only_override,read_only,reference_qual_override,reference_qual,attributes_override,attributes \
  --limit 100

# 4. Inspect choice lists (including dependent choices such as subcategory -> category)
snow-cli-ro table choices incident state
snow-cli-ro table choices incident subcategory

# 5. Check class distribution on an extendable base table (e.g. task or cmdb_ci)
snow-cli-ro table stats task --query 'active=true' --group-by sys_class_name
```

## 3. Inspecting table-level settings & attributes

The row in `sys_dictionary` where `element` is empty (`internal_type=collection`) stores table-level settings like auditing (`audit=true`), text indexing, and table attributes:

```bash
snow-cli-ro table list sys_dictionary \
  --query 'nameINincident,task^internal_type=collection' \
  --fields name,audit,text_index,attributes,read_only
```

To inspect reference qualifiers or dynamic defaults on a specific column (when not overridden in `sys_dictionary_override`):

```bash
snow-cli-ro table list sys_dictionary \
  --query 'nameINincident,task^element=assignment_group' \
  --fields name,element,internal_type,reference,use_reference_qualifier,reference_qual,default_value,attributes,dependent
```

## 4. Verifying queries before trusting empty or wide results

ServiceNow silently ignores unknown or unreadable field names in queries. Use `--check-fields error` when scripting or validating unfamiliar schemas:

```bash
snow-cli-ro table list incident \
  --query 'active=true^assignment_group.name=Service Desk' \
  --fields number,short_description,state,assignment_group.name \
  --check-fields error --limit 10
```

## 5. Gotchas

- **Dictionary Overrides are invisible in `sys_dictionary`:** If `incident.state` has a different default value, mandatory flag, or reference qualifier than `task.state`, it lives in `sys_dictionary_override` — not as a second `sys_dictionary` row. Always check both.
- **`table choices` picks the most specific table:** If `incident` defines even one choice for `state`, it overrides `task`'s `state` choices for `incident`. Check `dependent_value` for cascading choices like `category` → `subcategory`.
- **Table hierarchy vs `sys_class_name`:** Querying `task` returns `incident`, `change_request`, `problem`, etc., with `sys_class_name` naming the concrete subclass — but fields defined only on `incident` are not directly readable on `task` without `ref_incident.<field>` dot-walking.
- **Reference fields vs display values:** Raw queries compare against `sys_id` (`assignment_group=<sys_id>`) unless you dot-walk (`assignment_group.name=Network`). Use `--display-value both` when you need both the code/sys_id and label.
- **Number prefixes live in `sys_number`:** Useful when someone pastes an unfamiliar prefix like `SCTASK0012345` or `HR00042` and you need to map it to a table (`table list sys_number --query 'prefix=SCTASK' --fields category,prefix`).
