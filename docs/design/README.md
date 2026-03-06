# Design Documents

This directory contains design documents that describe the architecture and
technical design of major components in snow-cli.

## Index

| Document                          | Description                                |
|-----------------------------------|--------------------------------------------|
| [authentication.md](authentication.md) | Authentication architecture and trait design |
| [data-import-export.md](data-import-export.md) | Data export, import, and test-data seeding plan |
| [http-client.md](http-client.md)       | HTTP client, pagination, and error handling  |

## Guidelines

Design documents should:

1. Describe the **problem** being solved
2. Show the **high-level architecture** (module boundaries, data flow)
3. Define **public interfaces** (traits, structs, function signatures)
4. Call out **edge cases** and **error handling** strategies
5. Include **examples** of usage where helpful

Use the filename pattern `<component>.md`.
