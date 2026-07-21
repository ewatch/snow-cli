#!/usr/bin/env python3
"""TOML scenario parsing/resolution helper for scripts/e2e-run.

Stdlib only (tomllib, json) so it runs on a bare Python 3.11+ without
installing anything. scripts/e2e-run shells out to this for TOML parsing,
$VAR env substitution, and {{capture}} placeholder substitution, then does
process execution and control flow itself.

Subcommands:
  parse <file.toml>
      Print the whole scenario as JSON, with $VAR / ${VAR} occurrences in
      every string value substituted from the current environment.

  step-count <file.toml> <setup|cleanup>
      Print the number of steps in that phase (0 if absent).

  resolve-step <file.toml> <setup|cleanup|command> <index> [captured_json]
      Print one step as JSON, ready to execute:
        {"kind": "args"|"shell", "argv": [...], "shell": "...",
         "capture": {...}, "allow_failure": bool, "description": "..."}
      $VAR substitution is applied first, then {{name}} placeholders are
      replaced using captured_json (a JSON object of previously captured
      values). Resolution exits with status 2 before process invocation if a
      placeholder is missing, null, or otherwise remains unresolved. `index`
      is ignored for phase "command".
"""

import json
import os
import re
import sys
import tomllib


CAPTURE_PLACEHOLDER = re.compile(r"\{\{([^{}]+)\}\}")


def load(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


def expand_env(value):
    if isinstance(value, str):
        return os.path.expandvars(value)
    if isinstance(value, list):
        return [expand_env(v) for v in value]
    if isinstance(value, dict):
        return {k: expand_env(v) for k, v in value.items()}
    return value


def substitute_captures(value, captured):
    if isinstance(value, str):
        def replace(match):
            captured_value = captured.get(match.group(1))
            if captured_value is None:
                return match.group(0)
            return str(captured_value)

        return CAPTURE_PLACEHOLDER.sub(replace, value)
    if isinstance(value, list):
        return [substitute_captures(v, captured) for v in value]
    return value


def unresolved_capture_names(value):
    if isinstance(value, str):
        return set(CAPTURE_PLACEHOLDER.findall(value))
    if isinstance(value, list):
        return set().union(*(unresolved_capture_names(v) for v in value))
    return set()


def reject_unresolved_captures(value, phase, index):
    names = sorted(unresolved_capture_names(value))
    if not names:
        return

    location = phase if phase == "command" else f"{phase}[{index}]"
    placeholders = ", ".join(f"{{{{{name}}}}}" for name in names)
    noun = "placeholder" if len(names) == 1 else "placeholders"
    error = f"{location} has unresolved capture {noun} {placeholders}"
    print(json.dumps({"error": error}), file=sys.stderr)
    raise SystemExit(2)


def cmd_parse(path):
    json.dump(expand_env(load(path)), sys.stdout)


def cmd_step_count(path, phase):
    data = load(path)
    print(len(data.get(phase, [])))


def cmd_resolve_step(path, phase, index, captured_json):
    data = expand_env(load(path))
    captured = json.loads(captured_json) if captured_json else {}

    if phase == "command":
        step = data.get("command")
        if step is None:
            print(json.dumps({"error": "scenario has no [command] table"}), file=sys.stderr)
            sys.exit(2)
    else:
        steps = data.get(phase, [])
        idx = int(index)
        if idx >= len(steps):
            print(json.dumps({"error": f"no {phase} step at index {idx}"}), file=sys.stderr)
            sys.exit(2)
        step = steps[idx]

    resolved = {
        "description": step.get("description", ""),
        "allow_failure": bool(step.get("allow_failure", phase == "cleanup")),
        "capture": step.get("capture", {}),
    }
    if step.get("shell") is not None:
        resolved["kind"] = "shell"
        resolved["shell"] = substitute_captures(step["shell"], captured)
        resolved["argv"] = []
        reject_unresolved_captures(resolved["shell"], phase, index)
    else:
        resolved["kind"] = "args"
        resolved["shell"] = ""
        resolved["argv"] = substitute_captures(step.get("args", []), captured)
        reject_unresolved_captures(resolved["argv"], phase, index)

    json.dump(resolved, sys.stdout)


def main():
    if len(sys.argv) < 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)

    op, path = sys.argv[1], sys.argv[2]
    if op == "parse":
        cmd_parse(path)
    elif op == "step-count":
        cmd_step_count(path, sys.argv[3])
    elif op == "resolve-step":
        phase = sys.argv[3]
        index = sys.argv[4] if len(sys.argv) > 4 else "0"
        captured_json = sys.argv[5] if len(sys.argv) > 5 else "{}"
        cmd_resolve_step(path, phase, index, captured_json)
    else:
        print(f"unknown op: {op}", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
