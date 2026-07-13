#!/usr/bin/env bash
#
# Secret-field guard (ratchet).
#
# A struct that stores a secret (password, token, API key, ...) in a raw
# `String` will leak it through a derived `Debug`, `tracing`, or a panic. The
# project wraps such fields in `crate::models::secret::Secret`, which redacts
# by construction.
#
# This guard fails CI when a *new* secret-looking field is declared as a bare
# `String`/`Option<String>`. Pre-existing fields are grandfathered in
# `scripts/secret-fields-baseline.txt` so the codebase can be migrated
# incrementally without the check going red — that file is the to-do list.
#
# Resolve a new finding by, in order of preference:
#   1. wrapping the field:  `password: Secret<String>`
#   2. annotating a genuine non-secret with a trailing `// secret-guard: allow`
#      comment (e.g. a token *type*, a fn parameter, a test mock)
#   3. (only for a legitimately pre-existing field) re-baselining with:
#        scripts/check-secret-fields.sh --update
#
set -euo pipefail

cd "$(dirname "$0")/.."

BASELINE="scripts/secret-fields-baseline.txt"

# Field names that indicate a secret.
name='(password|passwd|secret|token|api_?key|credential)'
# Suffixes that look secret-ish but are not (token_type, *_url, ...).
safe_suffix='_(type|types|url|uri|name|path|host|endpoint|id|kind|mode|format|count|at|scope|link|field|key_path)'

# Raw findings: `path:line:  field: Type,`. POSIX ERE so it runs identically on
# the CI runner and on BSD/macOS.
raw_matches() {
  grep -rnE "^[[:space:]]*(pub[[:space:]]+)?[a-z_]*${name}[a-z_]*[[:space:]]*:[[:space:]]*(String|Option<String>)" src/ \
    | grep -vE "[a-z_]*${name}[a-z_]*${safe_suffix}[[:space:]]*:" \
    | grep -v 'Secret<' \
    | grep -v 'src/models/secret.rs' \
    | grep -v 'secret-guard: allow' \
    || true
}

# Normalize to `path | field: Type` (drop the line number) so the baseline is
# stable across unrelated edits that shift line numbers.
normalize() {
  sed -E 's/^([^:]+):[0-9]+:[[:space:]]*(.*[^[:space:]])[[:space:]]*$/\1 | \2/' | sort -u
}

current="$(raw_matches | normalize)"

if [[ "${1:-}" == "--update" ]]; then
  {
    echo "# Secret-named fields still stored as a raw String, grandfathered by"
    echo "# scripts/check-secret-fields.sh. Migrate each to Secret<...> and delete"
    echo "# its line here. New entries not listed here fail CI."
    echo "# Regenerate with: scripts/check-secret-fields.sh --update"
    printf '%s\n' "$current"
  } >"$BASELINE"
  echo "Wrote baseline: $BASELINE"
  exit 0
fi

baseline="$(grep -vE '^[[:space:]]*(#|$)' "$BASELINE" 2>/dev/null | sort -u || true)"

# Findings present now but not grandfathered.
new_findings="$(comm -23 <(printf '%s\n' "$current") <(printf '%s\n' "$baseline") | grep -v '^$' || true)"

if [[ -n "$new_findings" ]]; then
  echo "error: new secret-bearing field(s) stored as a raw String:"
  echo
  printf '  %s\n' "$new_findings"
  echo
  echo "Wrap the field in Secret<...>, or annotate the line '// secret-guard: allow'"
  echo "if it is genuinely not a secret. See scripts/check-secret-fields.sh."
  exit 1
fi

# Nudge (non-fatal) to prune baseline entries that have since been wrapped.
stale="$(comm -13 <(printf '%s\n' "$current") <(printf '%s\n' "$baseline") | grep -v '^$' || true)"
if [[ -n "$stale" ]]; then
  echo "note: $(printf '%s\n' "$stale" | grep -c .) baseline entr(y/ies) no longer present; consider running --update to prune."
fi

echo "secret-field guard: OK ($(printf '%s\n' "$baseline" | grep -c . || true) grandfathered, 0 new)"
