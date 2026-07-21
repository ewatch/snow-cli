# `graphql`

Use `graphql` to submit a document to the optional Now GraphQL endpoint. It is
useful when an instance exposes a GraphQL schema that fits the data you need.

```bash
snow-cli graphql '{ GlideRecord_Query { incident(queryConditions: "active=true", pagination: {limit: 2}) { _results { number { value } short_description { value } } } } }'
```

The command has an implicit query action and sends the document to
`/api/now/graphql`. The document must match the GraphQL schema on the target
instance.

## Provide a document

Provide exactly one document source: a positional document, `--query`, or
`--query-file`. If none is provided, the command reads the document from stdin.

```bash
snow-cli graphql --query '{ GlideRecord_Query { incident(queryConditions: "active=true", pagination: {limit: 2}) { _results { number { value } short_description { value } } } } }'
snow-cli graphql --query-file incident.graphql
cat incident.graphql | snow-cli graphql
```

Use `--variables` to pass a JSON object for variables in the document:

```bash
snow-cli graphql \
  --query 'query Incidents($qc: String) { GlideRecord_Query { incident(queryConditions: $qc, pagination: {limit: 1}) { _results { number { value } state { value displayValue } } } } }' \
  --variables '{"qc":"active=true"}'
```

## Before you use it

Now GraphQL must be enabled by an administrator on the target instance.
`snow-cli` does not discover schemas or enable the feature for you.

On a standard ServiceNow instance, tables are not root fields of `QueryType`.
Instead, use the namespaces ServiceNow ships out of the box:

- `GlideRecord_Query`: query a table by name (as shown above), returning
  `_results` rows where each field is an object with `value` (and, for choice
  fields, `displayValue`) — not a bare scalar.
- `GlideAggregate_Query`: run aggregate queries (counts, group-by) against a
  table, mirroring `GlideAggregate` semantics.

Introspection (`__schema`, `__type`) is disabled on ServiceNow instances by
default, so you generally cannot discover the schema by querying it. Rely on
ServiceNow's documented GraphQL namespaces (`GlideRecord_Query` /
`GlideAggregate_Query`) rather than schema discovery when building documents.

GraphQL is not available in `snow-cli-ro` or with `--read-only`, because a
submitted document can contain mutations. For raw REST endpoints, use the
[`api` command reference](/commands/api/) instead.

## Related pages

- [Command reference](/commands/)
- [`api` command reference](/commands/api/)
