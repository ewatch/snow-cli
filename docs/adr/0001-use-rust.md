# ADR-0001: Use Rust as Implementation Language

## Status

Accepted

## Context

The CLI needs to run on all major operating systems (macOS, Linux, Windows) with
minimal installation friction. It must be suitable for use by both humans and
automated agents (LLMs, CI/CD pipelines).

Key requirements:
- Cross-platform single binary distribution (no runtime dependency)
- Strong type safety for reliable API interactions
- Good performance for large data transfers
- Mature ecosystem for HTTP, TLS, and CLI tooling

## Decision

Use Rust (latest stable) as the implementation language.

## Alternatives Considered

| Language       | Pros                                    | Cons                                  |
|----------------|-----------------------------------------|---------------------------------------|
| **Go**         | Simple, fast compile, single binary     | Less expressive type system, GC pauses |
| **Rust**       | Zero-cost abstractions, no GC, single binary | Steeper learning curve, slower compile |
| **TypeScript** | Large ecosystem, rapid development      | Requires Node.js runtime              |
| **Python**     | Rapid prototyping, familiar             | Requires Python runtime, slower       |

## Consequences

- **Easier:** Distribution (single binary), reliability (compiler catches bugs),
  performance (no GC, zero-cost abstractions)
- **Harder:** Initial development velocity (borrow checker learning curve),
  finding contributors familiar with Rust
