# ADR-0006: Profile Resolution and Command UX Simplification

## Status

Accepted

## Context

Users reported confusion in four areas:

1. The active profile behavior felt inconsistent. `config use-profile` sets a default profile,
   but command execution still behaved as if profile `default` was always selected unless
   `--profile` was passed explicitly.
2. Profile lifecycle commands were incomplete (`delete-profile` was missing).
3. The `incident` command duplicated functionality already covered by generic `table` commands
   and remained unimplemented.
4. The `codesearch` command syntax and options were harder to discover than necessary
   (`search --term <term>` and boolean option ergonomics).

## Decision

1. Make `--profile` optional and resolve active profile from configuration by default:
   - use explicit `--profile <name>` when provided
   - otherwise use `default_profile` from `~/.servicenow/config.toml`
2. Add `config delete-profile <name>` with safeguards:
   - deleting the current default requires explicit confirmation
   - deleting the current default requires choosing a replacement default profile
   - keychain credentials are cleaned up on profile deletion (best effort)
3. Remove the top-level `incident` subcommand from CLI wiring.
4. Simplify code search UX:
   - `codesearch search <query>` (positional query)
   - `--source-table` with alias `--table`
   - `--current-scope` flag instead of requiring explicit boolean string values
5. Surface the active profile for interactive users by printing a small green profile hint
   to stderr on each run (TTY only), with opt-out via `SNOW_CLI_PROFILE_HINT=0`.

## Alternatives Considered

1. Keep `--profile` defaulting to literal `default`
   - Rejected because it ignores the configured default profile and causes surprising behavior.
2. Keep `incident` as an alias to `table` operations
   - Rejected for now to keep command surface minimal and avoid duplicate maintenance paths.
3. Keep `codesearch search --term <term>`
   - Rejected because positional query is simpler and aligns with common CLI search patterns.
4. Print profile hint on stdout
   - Rejected because it would break machine-readable output pipelines.

## Consequences

### Positive

- Profile behavior now matches user expectations and config semantics.
- Config management includes full create/update/list/use/delete lifecycle.
- CLI surface is clearer by removing incomplete duplication.
- Code search is easier to use for common cases.
- Interactive users gain quick visibility into the active profile without impacting parsers.

### Trade-offs

- This is a CLI behavior change; users relying on implicit `default` must ensure
  `default_profile` is configured correctly.
- Removing `incident` may require users to migrate existing scripts to `table` commands.
- Profile hint adds stderr noise in interactive sessions (mitigated by TTY check and env opt-out).
