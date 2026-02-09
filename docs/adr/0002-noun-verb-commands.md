# ADR-0002: Noun-Verb Command Structure

## Status

Accepted

## Context

The CLI needs a command structure that works well for both human users and LLM-based
coding agents. Discoverability and predictability are critical — agents need to
construct commands programmatically, and humans need to explore available operations
via `--help`.

## Decision

Use a **noun-verb** pattern: `snow-cli <noun> <verb> [options]`.

Examples:
- `snow-cli table list incident --limit 10`
- `snow-cli table get sys_user abc123`
- `snow-cli attachment upload incident INC0010001 --file report.pdf`
- `snow-cli config set-profile dev`

## Alternatives Considered

| Pattern        | Example                              | Assessment                          |
|----------------|--------------------------------------|-------------------------------------|
| Noun-verb      | `snow-cli table list incident`       | Natural English, discoverable       |
| Resource-verb  | `snow-cli list table incident`       | Verb-first, less discoverable       |
| Flat           | `snow-cli list-incidents`            | No hierarchy, poor discoverability  |

## Consequences

- **Easier:** `--help` at each noun level shows all available verbs; agents can
  enumerate operations per resource; natural language mapping
- **Harder:** Some operations span multiple nouns (mitigated by the `api` escape
  hatch for raw REST calls)
