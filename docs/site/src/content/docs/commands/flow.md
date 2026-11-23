# `flow`

Use `flow` to read a Flow Designer flow or subflow as a compact tree: its
trigger, top-level structure, and every logic, action, and subflow step in
execution order. It reads the Flow Designer tables through the Table API and
is available in `snow-cli-ro`.

```bash
snow-cli flow show <flow> [options]
```

All `flow` subcommands also accept the global flags from the [command overview](/commands/).

## `flow show <flow>`

`<flow>` is a `sys_hub_flow` `sys_id`, `internal_name`, or `name`.

Examples:

```bash
snow-cli flow show 'Change - Normal - Authorize'
snow-cli flow show change__normal__authorize --output toon
snow-cli-ro flow show 2f33fe56731310108ef62d2b04f6a749
```

Output (about 2 KB as JSON, 1.4 KB as TOON for this flow):

```json
{"flow":{"sys_id":"2f33…","name":"Change - Normal - Authorize","internal_name":"change__normal__authorize","type":"flow","status":"published","active":true,"scope":"global","description":"…"},
 "triggers":[{"name":"Created or Updated","type":"record_create_or_update","table":"change_request","condition":"chg_model=…^state=-3^on_hold=false"}],
 "structure":{"mode":"parallel","top_level":["Do the following in Parallel"],"branches":2},
 "steps":[{"path":"1","kind":"logic","type":"Do the following in Parallel","note":""},
          {"path":"1.1","kind":"logic","type":"Parallel Branch","note":""},
          {"path":"1.1.1","kind":"action","type":"Check Change for User Approval","note":"Check if assignment group manager has already approved"},
          {"path":"1.1.3","kind":"logic","type":"If","note":"Skipped"}, …],
 "action_types":["Apply Change Approval Policy","Check Change for User Approval", …],
 "source":"flow"}
```

Notes:

- `path` is the step's position in the tree (`1.2.3` is the third step of the
  second branch of the first top-level step). `kind` is `logic`, `action`, or
  `subflow`; `type` is the logic, action type, or subflow name; `note` is the
  condition label of an If/Else step or the step's comment.
- Steps are ordered by their `order` value. A moved step keeps an order such
  as `11➛12`; the number after the arrow is used.
- `structure.mode` is `parallel` when a top-level step is a parallel block,
  with `branches` counting its branches, otherwise `sequential`.
- The command reads `sys_hub_flow`, `sys_hub_trigger_instance`,
  `sys_hub_flow_logic`, `sys_hub_action_instance`,
  `sys_hub_sub_flow_instance`, and `sys_variable_value` (labels): six
  requests. When the flow has no steps of its own it uses the published
  snapshot's rows (`"source":"snapshot"`), and then the `*_v2` tables
  (`"source":"v2"`, four more requests). The `_v2` tables store labels and
  trigger conditions compressed, so `note`, `table`, and `condition` are empty
  there.
- If the label read fails, the tree is still printed with a `warnings` entry.
- Step inputs, data pills, and scripts are not shown; read them with
  `table list` on the tables above.
- A name that matches nothing fails with `RECORD_NOT_FOUND` (exit 4); a name
  shared by several flows fails with `VALIDATION_ERROR` (exit 2) listing the
  candidates.
