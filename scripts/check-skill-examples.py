#!/usr/bin/env python3
"""Check that every `snow-cli` example in the skill bundle still parses.

Extracts example invocations from fenced code blocks and inline code spans in
the skill Markdown, fills placeholders such as `<sys_id>` with dummy values,
and runs each one against an offline profile with no network access and no
credentials. An example passes when the CLI gets past argument and input
validation: auth, transport and local file errors are expected offline. It
fails on exit 2, which covers clap rejections (renamed flags, removed
subcommands, broken placeholder syntax such as `<a\\|b>`) and the CLI's own
`VALIDATION_ERROR`s (bad `--order-by`, unsupported `--output`, and so on).

Examples run with the binary they name, so `snow-cli-ro` examples are checked
against the read-only command set. An example written as `snow-cli` fails when
`snow-cli-ro` accepts it too: the skill's `allowed-tools` only pre-approves
`snow-cli-ro`, so plain `snow-cli` is reserved for commands that need the full
binary (instance writes, `script run`, `seed`, `--read-only` demos).

Usage: scripts/check-skill-examples.py [--bin target/debug/snow-cli] [--skills skills/snow-cli]
"""

from __future__ import annotations

import argparse
import os
import re
import shlex
import socket
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

# Per-example wall clock limit. Parse errors and offline failures return in
# milliseconds; a command still running after this long got past parsing.
EXAMPLE_TIMEOUT_SECS = 3

# A non-loopback SN-Utils bridge address: `snu` commands parse their arguments,
# then fail fast on this address instead of starting a broker daemon and
# waiting for a browser. 192.0.2.0/24 is TEST-NET-1 and is never contacted.
UNREACHABLE_SNU_WS_ADDR = "192.0.2.1:9"

# Exit 2 failures that come from the offline environment rather than from the
# example: the fixture an example refers to does not exist here. Matched as
# message substrings; keep this list short and specific.
ENVIRONMENT_VALIDATION_ERRORS = ("Seed run `placeholder` was not found",)

# Examples that read a body from stdin (`import-set load <table>`) get this.
STDIN_BODY = "{}"

# Bare prose mentions such as "`snow-cli flow show`" name a command without
# arguments; check those with `--help` so a removed subcommand is still caught.
MAX_PROSE_TOKENS = 3

PLACEHOLDER = re.compile(r"<([^<>\s\\]+)>")
# Profile names the skill uses in examples (`--profile dev-session`) must exist,
# with the auth method those commands require, so that the example reaches
# argument validation instead of PROFILE_NOT_FOUND.
def render_offline_config(occupied_port: int) -> str:
    profiles = {
        "offline": {"auth_method": "api_key"},
        "destination": {"auth_method": "api_key"},
        "dev-session": {"auth_method": "native_session"},
        # OAuth2 authorization-code profiles bind to a port already held by this
        # script so loopback `auth login` examples validate arguments and fail
        # fast on EADDRINUSE without opening a real browser window.
        "sdk": {
            "auth_method": "oauth2",
            "client_id": "placeholder-client-id",
            "oauth_grant_type": "authorization_code",
            "oauth_redirect_port": occupied_port,
        },
        "dev-oauth": {
            "auth_method": "oauth2",
            "client_id": "placeholder-client-id",
            "oauth_grant_type": "authorization_code",
            "oauth_redirect_port": occupied_port,
        },
    }
    lines = ["default_profile = 'offline'"]
    for name, settings in profiles.items():
        lines.append(f"[profiles.{name}]")
        lines.append("instance = 'https://127.0.0.1:9'")
        for key, value in settings.items():
            rendered = f"'{value}'" if isinstance(value, str) else str(value)
            lines.append(f"{key} = {rendered}")
    return "\n".join(lines) + "\n"


@dataclass
class Example:
    location: str
    text: str


def extract(skills: Path) -> list[Example]:
    examples: list[Example] = []
    for path in sorted(skills.rglob("*.md")):
        rel = path.relative_to(skills)
        in_fence = False
        pending: Example | None = None
        for number, line in enumerate(path.read_text().splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("```"):
                in_fence = not in_fence
                continue
            if in_fence:
                if pending is not None:
                    pending.text += " " + stripped.rstrip("\\").strip()
                    if not stripped.endswith("\\"):
                        examples.append(pending)
                        pending = None
                    continue
                if stripped.startswith(("snow-cli ", "snow-cli-ro ")):
                    example = Example(f"{rel}:{number}", stripped.rstrip("\\").strip())
                    if stripped.endswith("\\"):
                        pending = example
                    else:
                        examples.append(example)
            else:
                for match in re.finditer(r"`((?:snow-cli|snow-cli-ro) [^`]+)`", line):
                    examples.append(Example(f"{rel}:{number}", match.group(1)))
    return examples


def fill(text: str) -> str:
    def replace(match: re.Match[str]) -> str:
        name = match.group(1)
        return "0" * 32 if "sys_id" in name else "placeholder"

    text = re.sub(r"\s+#\s.*$", "", text)  # trailing shell comment
    text = text.replace("[search]", "placeholder").replace("[field]", "placeholder")
    return PLACEHOLDER.sub(replace, text)


def is_generic(text: str) -> bool:
    """`snow-cli <group> <cmd>` style patterns name no real command."""
    return bool(re.search(r"^\S+\s+<", text) or re.search(r"^\S+\s+\S+\s+<\w+>", text))


def run(binary: Path, tokens: list[str], env: dict[str, str], cwd: Path) -> str | None:
    """Runs one filled example; returns a failure description or None."""
    try:
        result = subprocess.run(
            [str(binary.with_name(tokens[0])), *tokens[1:]],
            capture_output=True,
            text=True,
            cwd=cwd,
            env=env,
            input=STDIN_BODY,
            timeout=EXAMPLE_TIMEOUT_SECS,
        )
    except subprocess.TimeoutExpired:
        return None
    if result.returncode != 2:
        return None
    stderr = result.stderr.strip()
    if any(known in stderr for known in ENVIRONMENT_VALIDATION_ERRORS):
        return None
    if stderr.startswith("{"):
        return stderr.splitlines()[-1]
    return stderr.splitlines()[0] if stderr else "exit 2"


def check(binary: Path, example: Example, env: dict[str, str], cwd: Path) -> str | None:
    """Returns a failure description, or None when the example is valid."""
    if is_generic(example.text):
        return None
    try:
        tokens = shlex.split(fill(example.text))
    except ValueError as error:
        return f"shell syntax: {error}"
    bare_mention = len(tokens) <= MAX_PROSE_TOKENS and not any(t.startswith("-") for t in tokens)
    if bare_mention:
        tokens.append("--help")
    failure = run(binary, tokens, env, cwd)
    if failure or tokens[0] != "snow-cli":
        return failure
    if run(binary, ["snow-cli-ro", *tokens[1:]], env, cwd) is None:
        return "snow-cli-ro accepts this command; write it as `snow-cli-ro`"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--bin", type=Path, default=Path("target/debug/snow-cli"))
    parser.add_argument("--skills", type=Path, default=Path("skills/snow-cli"))
    args = parser.parse_args()

    binary = args.bin.resolve()
    for name in ("snow-cli", "snow-cli-ro"):
        if not binary.with_name(name).exists():
            print(f"missing binary {binary.with_name(name)}; run `cargo build` first", file=sys.stderr)
            return 2

    examples = extract(args.skills)
    failures: list[tuple[Example, str]] = []
    with (
        tempfile.TemporaryDirectory() as home,
        socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied,
    ):
        occupied.bind(("127.0.0.1", 0))
        occupied.listen(1)
        offline_config = render_offline_config(occupied.getsockname()[1])
        config = Path(home) / "config.toml"
        env = {
            "PATH": os.environ.get("PATH", ""),
            "HOME": home,
            "SNOW_CLI_CONFIG": str(config),
            "SNOW_CLI_OUTPUT": "json",
            "SNOW_CLI_SNU_WS_ADDR": UNREACHABLE_SNU_WS_ADDR,
            "SNOW_CLI_TEST_KEYCHAIN_STORE": str(Path(home) / "keychain.json"),
            "SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN": "1",
        }
        for example in examples:
            # Reset config before each example so profile-mutating examples
            # (such as `profile edit` or `profile default`) do not affect others.
            config.write_text(offline_config)
            failure = check(binary, example, env, Path(home))
            if failure:
                failures.append((example, failure))

    for example, failure in failures:
        print(f"{example.location}: {example.text}\n    {failure}")
    print(f"checked {len(examples)} examples, {len(failures)} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
