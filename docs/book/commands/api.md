# `api`

Use `api` when you want to call a ServiceNow endpoint directly instead of using a higher-level command.

```bash
snow-cli api <verb> [options]
```

All `api` subcommands also accept the global flags from the [command overview](../commands.md).

## Supported verbs

- `api get <path>`
- `api post <path>`
- `api put <path>`
- `api delete <path>`

`<path>` should be a ServiceNow-relative path such as:

```text
/api/now/table/incident?sysparm_limit=1
/api/x_myapp/status
```

## Common options

- `-H, --header 'Key: Value'`: add a custom header; repeat the flag to send multiple headers
- `--data <body>`: request body for `post` and `put`

Examples:

```bash
snow-cli api get /api/now/table/incident?sysparm_limit=1
snow-cli api post /api/x_myapp/action --data '{"dry_run":true}'
snow-cli api put /api/x_myapp/config --data '{"enabled":true}'
snow-cli api delete /api/x_myapp/config/abc123
snow-cli api get /api/x_myapp/status -H 'X-Trace-Id: abc123'
```

## `api get <path>`

Send a GET request.

```bash
snow-cli api get <path> [-H 'Key: Value']
```

Use this for read-only endpoints.

## `api post <path>`

Send a POST request.

```bash
snow-cli api post <path> [--data '{"key":"value"}']
```

If `--data` is omitted and stdin is piped in, the CLI reads the request body from stdin.

Example:

```bash
echo '{"dry_run":true}' | snow-cli api post /api/x_myapp/action
```

## `api put <path>`

Send a PUT request.

```bash
snow-cli api put <path> [--data '{"key":"value"}']
```

Like `post`, this can also read the body from stdin.

## `api delete <path>`

Send a DELETE request.

```bash
snow-cli api delete <path>
```

Use `-H` if the endpoint needs additional headers.

### Shell quoting for URLs with query parameters

URLs containing `?` or `&` must be quoted in most shells (zsh, bash) to prevent glob expansion:

```bash
# Correct — quoted
snow-cli api get '/api/now/table/incident?sysparm_limit=1'
snow-cli api get "/api/now/table/incident?sysparm_limit=1"

# Wrong — will fail in zsh with "no matches found"
snow-cli api get /api/now/table/incident?sysparm_limit=1
```

## Response handling

`snow-cli` prints the raw response body to stdout.

- If the response is valid JSON and you use JSON-like output, the CLI pretty-prints it.
- If the response is not valid JSON, the body is printed as-is.
- `--output csv` prints the raw body unchanged.

This makes `api` the most flexible command when you need an endpoint that does not yet have a dedicated command.

## Related pages

- [`table` command reference](./table.md)
- [`script` command reference](./script.md)
