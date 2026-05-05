# Introduction

`snow-cli` is a cross-platform command-line interface for working with ServiceNow instances.

It is designed for:

- developers who need quick access to ServiceNow APIs,
- automation scripts and CI jobs,
- coding agents and LLM workflows,
- teams that prefer machine-readable JSON, JSON Lines, TOON, or CSV output.

The binary is written in Rust and is intended to ship as a single executable with no runtime dependencies.

## What you can do

With `snow-cli`, you can:

- manage connection profiles,
- log in using supported authentication methods,
- query, create, update, and delete Table API records,
- inspect table schemas,
- call raw REST API endpoints,
- run background scripts,
- search ServiceNow code,
- move data through export/import and seed workflows,
- generate shell completions.

## Command style

Commands use a noun-verb structure:

```bash
snow-cli <noun> <verb> [options]
```

Examples:

```bash
snow-cli profile add dev --instance https://example.service-now.com --auth-method basic --username admin
snow-cli auth login
snow-cli table list incident --query 'active=true' --limit 20
```
