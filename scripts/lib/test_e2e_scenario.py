#!/usr/bin/env python3
"""Focused tests for E2E scenario capture resolution."""

import json
import pathlib
import subprocess
import tempfile
import textwrap
import unittest


LIB = pathlib.Path(__file__).with_name("e2e_scenario.py")


class ResolveStepTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.root = pathlib.Path(self.temp_dir.name)
        self.scenario = self.root / "scenario.toml"
        self.scenario.write_text(
            textwrap.dedent(
                """
                name = "capture-resolution-test"

                [[setup]]
                args = ["setup", "records/{{sys_id}}"]

                [command]
                args = ["get", "records/{{sys_id}}"]

                [[cleanup]]
                shell = "remove '{{sys_id}}'"
                """
            ),
            encoding="utf-8",
        )

    def resolve(self, phase, captured):
        return subprocess.run(
            [
                "python3",
                str(LIB),
                "resolve-step",
                str(self.scenario),
                phase,
                "0",
                json.dumps(captured),
            ],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_successfully_substitutes_captures_for_every_phase(self):
        expected = {
            "setup": ("argv", ["setup", "records/abc123"]),
            "command": ("argv", ["get", "records/abc123"]),
            "cleanup": ("shell", "remove 'abc123'"),
        }

        for phase, (field, value) in expected.items():
            with self.subTest(phase=phase):
                result = self.resolve(phase, {"sys_id": "abc123"})

                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)[field], value)
                self.assertEqual(result.stderr, "")

    def test_missing_capture_fails_for_every_phase(self):
        for phase in ("setup", "command", "cleanup"):
            with self.subTest(phase=phase):
                result = self.resolve(phase, {})

                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, "")
                error = json.loads(result.stderr)
                location = phase if phase == "command" else f"{phase}[0]"
                self.assertEqual(
                    error["error"],
                    f"{location} has unresolved capture placeholder {{{{sys_id}}}}",
                )
                self.assertNotIn("abc123", result.stderr)

    def test_null_capture_fails_for_every_phase(self):
        for phase in ("setup", "command", "cleanup"):
            with self.subTest(phase=phase):
                result = self.resolve(phase, {"sys_id": None})

                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, "")
                self.assertIn(
                    "unresolved capture placeholder {{sys_id}}",
                    json.loads(result.stderr)["error"],
                )

    def test_unresolved_error_does_not_expose_capture_values(self):
        result = self.resolve("command", {"sys_id": "sensitive-{{nested}}"})

        self.assertEqual(result.returncode, 2)
        self.assertEqual(
            json.loads(result.stderr)["error"],
            "command has unresolved capture placeholder {{nested}}",
        )
        self.assertNotIn("sensitive", result.stderr)

    def test_literal_placeholder_cannot_false_green_before_process_invocation(self):
        invoked = self.root / "invoked"
        process = self.root / "would-pass"
        process.write_text(
            '#!/usr/bin/env bash\ntouch "$1"\nexit 0\n',
            encoding="utf-8",
        )
        process.chmod(0o755)

        for phase in ("setup", "command", "cleanup"):
            with self.subTest(phase=phase):
                invoked.unlink(missing_ok=True)
                result = subprocess.run(
                    [
                        "bash",
                        "-c",
                        'set -e; step=$(python3 "$1" resolve-step "$2" "$3" 0 "$4"); "$5" "$6"',
                        "runner",
                        str(LIB),
                        str(self.scenario),
                        phase,
                        json.dumps({"sys_id": "{{sys_id}}"}),
                        str(process),
                        str(invoked),
                    ],
                    check=False,
                    capture_output=True,
                    text=True,
                )

                self.assertEqual(result.returncode, 2)
                self.assertFalse(invoked.exists())
                self.assertIn(
                    "unresolved capture placeholder {{sys_id}}",
                    json.loads(result.stderr)["error"],
                )


if __name__ == "__main__":
    unittest.main()
