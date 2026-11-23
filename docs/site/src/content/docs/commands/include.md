# `include`

Use `include` to read script includes by name: scope, the class each one
extends, an outline of its members with line numbers, and only the source you
ask for. It reads `sys_script_include` through the Table API and is available
in `snow-cli-ro`.

```bash
snow-cli include show <name>... [options]
```

All `include` subcommands also accept the global flags from the [command overview](/commands/).

## `include show <name>...`

Each `<name>` (up to 5) is a script include `name`, an `api_name` such as
`global.AbstractAjaxProcessor` (any value containing `.`), or a `sys_id`.

Important options:

- `--chain`: also show the classes each name extends, handler first, up to 5 levels. Shows outlines without source
- `--source-of <member>[,<member>]`: include only these members' source, looked up in every listed class
- `--grep <regex>`: print only matching source lines with line numbers, across every listed class
- `--context <n>`: lines of context around each `--grep` match (default 3; requires `--grep`)
- `--script`: include the full source of every class
- `--no-script`: omit the source of a single include

Examples:

```bash
# One call: the extends chain, outlines, and two method bodies
snow-cli include show ChangeRequestStateHandler --chain --source-of canMoveTo,_resetModel
# Two chains at once (the handler and the state model it instantiates)
snow-cli include show ChangeRequestStateHandler ChangeRequestStateModel_normal --chain
snow-cli include show ChangeRequestStateHandler --chain --grep 'canMoveTo|_resetModel' --context 10
snow-cli-ro include show global.AbstractAjaxProcessor
```

With `--chain` or several names, the output is an envelope:

```json
{"includes":[
  {"name":"ChangeRequestStateHandlerSNC","api_name":"global.ChangeRequestStateHandlerSNC","sys_id":"…","scope":"global","active":true,"access":"public","client_callable":false,"extends":null,"script_lines":473,
   "outline":["38 DEFAULT_MODEL_CLASS=\"ChangeRequestStateModel_normal\"","152-187 canMoveTo(toState)","309-325 _resetModel()"],
   "source_of":[{"member":"canMoveTo","lines":"152-187","source":"canMoveTo: function(toState) {…"}]},
  {"name":"ChangeRequestStateModel_normal","extends":"ChangeRequestStateModelSNC_normal","extends_source":"parsed",
   "outline":["27-69 assess{nextState=[authorize],draft,authorize,scheduled,canceled}"]}],
 "references":["GSLog","ChangeUtils","ChangeRequestStateModel_emergency"]}
```

A single name without `--chain` prints one object and, unless `--grep`,
`--source-of`, or `--no-script` is given, its full `script`.

Notes:

- `outline` lists the members of the object assigned to `X.prototype`
  (directly or through `Object.extendsObject(Base, {...})`), with their line
  range: `name(params)` for functions, `name{keys}` for objects (short arrays
  such as `nextState=[authorize]` inline), and `name=literal` for short
  literals. The conventional `type` member is omitted. The outline is parsed
  from the source and can be incomplete for unusual code.
- `references` lists other classes the sources name as `new X(` or `X.y`,
  minus JavaScript built-ins, `Glide*` APIs, and the listed classes. They are
  not verified or fetched: pass the ones you need as extra names.
- `members_not_found` (only when non-empty) lists `--source-of` names that no
  listed class declares.
- `sys_script_include` has no `extends` column. `extends` is parsed from
  `Object.extendsObject(<Base>, {...})`, ignoring comments, and is marked
  `"extends_source":"parsed"`. `Class.create()` alone gives `"extends": null`.
- `grep` has `match_count` and `lines`: `N:text` for a match, `N-text` for
  context, and `--` between non-adjacent blocks. `--grep` and `--source-of`
  each stop at 400 lines per command and then report `"truncated": true`.
- Every class costs one request; a parent already listed is not fetched again.
  A parent name in several scopes resolves to the child's scope, then
  `global`. A parent that cannot be found ends the chain and sets
  `"extends_found": false` on the child.
- A name that matches nothing fails with `RECORD_NOT_FOUND` (exit 4). A name
  in several scopes fails with `VALIDATION_ERROR` (exit 2) and lists the
  candidates' `api_name` and `sys_id`; pass the `api_name` instead.
- CSV output has one row per script include, with lists as JSON text.

To print one field of any record without JSON escaping, use
`table get <table> <name> --fields <field> --raw`; see the
[`table` command reference](/commands/table/).
