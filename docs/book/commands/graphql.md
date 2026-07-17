# `graphql`

Use `graphql` to submit a document to the optional Now GraphQL endpoint. It is
useful when an instance exposes a GraphQL schema that fits the data you need.

```bash
snow-cli graphql '{ incident { number } }'
```

The command has an implicit query action and sends the document to
`/api/now/graphql`. The document must match the GraphQL schema on the target
instance.

## Provide a document

Provide exactly one document source: a positional document, `--query`, or
`--query-file`. If none is provided, the command reads the document from stdin.

```bash
snow-cli graphql --query '{ incident { number } }'
snow-cli graphql --query-file incident.graphql
cat incident.graphql | snow-cli graphql
```

Use `--variables` to pass a JSON object for variables in the document:

```bash
snow-cli graphql \
  --query 'query Incident($number: String!) { incident(number: $number) { number } }' \
  --variables '{"number":"INC0010001"}'
```

## Before you use it

Now GraphQL must be enabled by an administrator on the target instance.
`snow-cli` does not discover schemas or enable the feature for you.

GraphQL is not available in `snow-cli-ro` or with `--read-only`, because a
submitted document can contain mutations. For raw REST endpoints, use the
[`api` command reference](./api.md) instead.

## Related pages

- [Command reference](../commands.md)
- [`api` command reference](./api.md)
