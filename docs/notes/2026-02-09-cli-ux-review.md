# CLI UX Review Notes (2026-02-09)

This note captures practical lessons from a profile-management and command-UX review.

## What we learned

- Defaults must be real defaults: if config has `default_profile`, runtime should honor it.
- `--profile` should override, not replace, default-resolution behavior.
- Any user-facing status hint must go to stderr, never stdout, to preserve JSON/CSV pipelines.
- Profile lifecycle needs symmetry: create/update/list/use/delete.
- Duplicate command surfaces (like an `incident` shortcut over `table`) create confusion when
  behavior diverges or implementation drifts.
- Boolean CLI options are easier when they express intent (`--current-scope`) rather than
  value assignment (`--search-all-scopes false`).

## Follow-up opportunities

- Add `config current` for scripts that only need active profile + instance.
- Add `config doctor` to validate profile completeness by auth method.
- Consider introducing command aliases (`code search`) while preserving backward compatibility.
- Add migration notes in release docs for removed/renamed commands.
