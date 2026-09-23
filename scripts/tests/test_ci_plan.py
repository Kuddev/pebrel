from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "ci_plan.py"


class CiPlanCliTests(unittest.TestCase):
    def run_cli(self, event_name: str, payload: object, output: Path) -> subprocess.CompletedProcess[str]:
        event_path = output.parent / "event.json"
        event_path.write_text(json.dumps(payload), encoding="utf-8")
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--event-name",
                event_name,
                "--event-path",
                str(event_path),
                "--output",
                str(output),
            ],
            text=True,
            capture_output=True,
            check=False,
        )

    @staticmethod
    def outputs(path: Path) -> dict[str, object]:
        lines = path.read_text(encoding="utf-8").splitlines()
        return {name: json.loads(value) for name, value in (line.split("=", 1) for line in lines)}

    def test_draft_pr_does_not_materialize_scarce_runner_rows(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "github-output"
            result = self.run_cli("pull_request", {"pull_request": {"draft": True}}, output)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                output.read_text(encoding="utf-8"),
                'native_matrix=[{"os":"ubuntu-24.04"},{"os":"windows-2022"},{"os":"macos-26"}]\n'
                'release_matrix=[{"os":"macos-26"}]\n',
            )
            plan = self.outputs(output)
            self.assertNotIn({"os": "windows-11-arm"}, plan["native_matrix"])
            self.assertNotIn({"os": "macos-26-intel"}, plan["native_matrix"])

    def test_ready_pr_has_all_native_and_release_checks(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "github-output"
            result = self.run_cli("pull_request", {"pull_request": {"draft": False}}, output)

            self.assertEqual(result.returncode, 0, result.stderr)
            plan = self.outputs(output)
            self.assertEqual(
                [row["os"] for row in plan["native_matrix"]],
                ["ubuntu-24.04", "windows-2022", "macos-26", "windows-11-arm", "macos-26-intel"],
            )
            self.assertEqual(
                [row["os"] for row in plan["release_matrix"]],
                ["macos-26", "macos-26-intel"],
            )

    def test_success_appends_outputs_without_replacing_existing_lines(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "github-output"
            output.write_text("existing=preserved\n", encoding="utf-8")
            result = self.run_cli("push", {}, output)

            self.assertEqual(result.returncode, 0, result.stderr)
            lines = output.read_text(encoding="utf-8").splitlines()
            self.assertEqual(lines[0], "existing=preserved")
            self.assertTrue(lines[1].startswith("native_matrix=[{"))
            self.assertTrue(lines[2].startswith("release_matrix=[{"))

    def test_merge_group_and_non_pr_events_have_full_coverage(self) -> None:
        events = ("merge_group", "push", "workflow_dispatch", "workflow_call", "schedule")
        with tempfile.TemporaryDirectory() as directory:
            for event_name in events:
                with self.subTest(event_name=event_name):
                    output = Path(directory) / f"{event_name}.out"
                    result = self.run_cli(event_name, {}, output)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    plan = self.outputs(output)
                    self.assertEqual(
                        [row["os"] for row in plan["native_matrix"]],
                        ["ubuntu-24.04", "windows-2022", "macos-26", "windows-11-arm", "macos-26-intel"],
                    )
                    self.assertEqual(
                        [row["os"] for row in plan["release_matrix"]],
                        ["macos-26", "macos-26-intel"],
                    )

    def test_invalid_events_fail_without_matrix_outputs(self) -> None:
        cases = (
            ("pull_request", {}, "missing pull request"),
            ("pull_request", {"pull_request": []}, "wrong pull request type"),
            ("pull_request", {"pull_request": {}}, "missing draft"),
            ("pull_request", {"pull_request": {"draft": "false"}}, "wrong draft type"),
            ("pull_request", {"pull_request": {"draft": 0}}, "integer draft type"),
            ("unknown", {}, "unknown event"),
            ("push", [], "non-object payload"),
        )
        with tempfile.TemporaryDirectory() as directory:
            for event_name, payload, label in cases:
                with self.subTest(label=label):
                    output = Path(directory) / f"{label.replace(' ', '-')}.out"
                    output.write_text("existing=preserved\n", encoding="utf-8")
                    result = self.run_cli(event_name, payload, output)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("ci_plan:", result.stderr)
                    self.assertEqual(output.read_text(encoding="utf-8"), "existing=preserved\n")

    def test_malformed_json_fails_without_creating_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            event_path = root / "broken.json"
            output = root / "github-output"
            event_path.write_text('{"pull_request":', encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--event-name",
                    "pull_request",
                    "--event-path",
                    str(event_path),
                    "--output",
                    str(output),
                ],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())

    def test_non_standard_json_constant_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            event_path = root / "non-standard.json"
            output = root / "github-output"
            event_path.write_text('{"value":NaN}', encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--event-name",
                    "push",
                    "--event-path",
                    str(event_path),
                    "--output",
                    str(output),
                ],
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
