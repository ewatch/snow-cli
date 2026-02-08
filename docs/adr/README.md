# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for the snow-cli project.

ADRs document significant technical decisions made during development. They provide
context for why decisions were made, what alternatives were considered, and what
trade-offs were accepted.

## Format

Each ADR follows the naming convention: `NNNN-short-title.md` (e.g., `0001-use-rust.md`).

### Template

```markdown
# ADR-NNNN: Title

## Status

Accepted | Superseded | Deprecated

## Context

What is the issue that motivates this decision?

## Decision

What is the change that we are proposing or have agreed to implement?

## Alternatives Considered

What other options were evaluated?

## Consequences

What becomes easier or harder as a result of this decision?
```

## Index

| ADR  | Title                          | Status   |
|------|--------------------------------|----------|
| 0001 | Use Rust as implementation language | Accepted |
| 0002 | Noun-verb command structure    | Accepted |
| 0003 | OS keychain for credential storage | Accepted |
| 0004 | TOML for configuration format  | Accepted |
| 0005 | Structured JSON error output   | Accepted |
