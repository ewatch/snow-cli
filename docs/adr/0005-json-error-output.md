# ADR-0005: Structured JSON Error Output

## Status

Accepted

## Context

The CLI is designed to be consumed by both humans and LLM-based agents. Error
handling must be machine-parseable so that agents can programmatically detect
failures, extract error codes, and decide on retry strategies.

## Decision

All errors are written to **stderr** as structured JSON:

```json
{
  "error": {
    "code": "AUTH_TOKEN_EXPIRED",
    "message": "OAuth token expired and refresh failed",
    "status": 401,
    "detail": "Token refresh returned 403: insufficient scope",
    "instance": "https://mycompany.service-now.com"
  }
}
```

Successful output goes to **stdout** (JSON or CSV). This separation allows
agents to pipe stdout for data processing while monitoring stderr for errors.

The process exit code is non-zero on any error.

## Alternatives Considered

| Approach           | Pros                      | Cons                          |
|--------------------|---------------------------|-------------------------------|
| JSON errors        | Machine-parseable         | Less readable for humans      |
| Human-readable     | Easy to read in terminal  | Hard for agents to parse      |
| Configurable       | Flexible                  | Adds complexity               |

## Consequences

- **Easier:** Agents can parse errors reliably, error codes enable programmatic
  handling, consistent structure across all error types
- **Harder:** Raw terminal users see JSON instead of friendly messages (mitigated
  by verbosity flags that add human-readable log lines via tracing)
