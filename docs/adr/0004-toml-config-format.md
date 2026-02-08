# ADR-0004: TOML for Configuration Format

## Status

Accepted

## Context

The CLI needs a human-editable configuration file for storing instance profiles
and non-sensitive settings. The format must support comments (for documentation),
nested structures (for profiles), and be widely understood.

## Decision

Use TOML as the configuration format. Config file location: `~/.servicenow/config.toml`.

## Alternatives Considered

| Format | Pros                          | Cons                              |
|--------|-------------------------------|-----------------------------------|
| TOML   | Human-readable, comments, Rust-native (Cargo) | Less familiar outside Rust |
| YAML   | Widely known, comments        | Indentation-sensitive, gotchas    |
| JSON   | Universal                     | No comments, verbose              |

## Consequences

- **Easier:** Familiar to Rust developers (Cargo.toml), excellent `toml` crate
  support with serde, inline comments for self-documenting configs
- **Harder:** Users from YAML-heavy ecosystems (Kubernetes, Ansible) may need
  a brief adjustment
