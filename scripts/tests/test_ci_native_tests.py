from __future__ import annotations

import subprocess
import re
from pathlib import Path
import tomllib
import unittest
from unittest.mock import patch

from scripts.ci_native_tests import main, native_commands


class NativeSuiteTests(unittest.TestCase):
    def test_every_pr_and_merge_group_runs_without_path_exclusions(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text()
        events = workflow.split("\non:\n", 1)[1].split("\nconcurrency:", 1)[0]
        for event in ("pull_request", "merge_group"):
            declaration = re.search(rf"^  {event}:(.*?)(?=^  [a-z_]+:|\Z)", events, re.M | re.S)
            self.assertIsNotNone(declaration)
            self.assertNotIn("paths", declaration.group(1))
            self.assertNotIn("branches", declaration.group(1))
            self.assertNotIn("types", declaration.group(1))
        self.assertIn("branches: [main]", events)
        self.assertNotIn("branches-ignore", events)
        self.assertNotIn("pull_request_target", workflow)
        self.assertNotIn("contents: write", workflow)
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request'", workflow)

    def test_arm_job_requires_native_execution_and_matching_console_runtime(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text()
        self.assertIn("windows-11-arm", workflow)
        self.assertIn("host: aarch64-pc-windows-msvc", workflow)
        self.assertIn("OSArchitecture -ne 'Arm64'", workflow)
        self.assertIn("-Architecture $architecture", workflow)
        self.assertLess(workflow.index("Prepare pinned Windows console runtime"),
                        workflow.index("Test complete workspace"))
        preview = (root / ".github/workflows/preview-packages.yml").read_text()
        windows = preview.split("\n  windows:\n", 1)[1].split("\n  aggregate:", 1)[0]
        self.assertNotIn("prepare-windows-runtime.ps1", preview.split("\njobs:", 1)[1].split("\n  windows:", 1)[0])
        self.assertLess(windows.index("prepare-windows-runtime.ps1"),
                        windows.index("cargo test --locked --workspace"))
        self.assertIn("python scripts/conformance/windows_standard_user.py scripts/conformance/run.py", windows)

    def test_full_workspace_and_interactions_share_one_unfiltered_invocation(self):
        rust = [command for command in native_commands() if command[:2] == ["cargo", "test"]]
        self.assertEqual(len(rust), 1)
        command = rust[0]
        self.assertEqual(command[1], "test")
        self.assertIn("--locked", command)
        self.assertIn("--workspace", command)
        self.assertEqual(command[command.index("--features") + 1], "nebula/gpui-test-support")
        self.assertNotIn("--exclude", command)
        self.assertNotIn("--lib", command)
        self.assertNotIn("--skip", command)
        self.assertNotIn("--", command)

    def test_actual_product_feature_graph_is_also_checked(self):
        checks = [command for command in native_commands() if command[:2] == ["cargo", "check"]]
        self.assertEqual(len(checks), 1)
        command = checks[0]
        self.assertEqual(command[command.index("--features") + 1], "gpui-shell")
        self.assertEqual(command[command.index("--bin") + 1], "pebrel")

    def test_fast_test_profile_preserves_runtime_checks_and_resets_named_overrides(self):
        root = Path(__file__).resolve().parents[2]
        config = tomllib.loads((root / ".github/ci-profile.toml").read_text())
        workspace = tomllib.loads((root / "Cargo.toml").read_text())
        profile = config["profile"]["ci"]
        self.assertEqual(profile["inherits"], "dev")
        self.assertTrue(profile["debug-assertions"])
        self.assertTrue(profile["overflow-checks"])
        for package in workspace["profile"]["dev"]["package"]:
            self.assertEqual(profile["package"][package]["opt-level"], 0)
        self.assertNotIn("release", config["profile"])

    def test_both_python_test_roots_are_discovered(self):
        suites = [command for command in native_commands() if "unittest" in command]
        self.assertEqual(
            {command[command.index("-s") + 1] for command in suites},
            {"scripts/tests", "scripts/conformance/tests"},
        )
        self.assertTrue(all("discover" in command for command in suites))

    def test_success_runs_every_command_and_checks_exit_codes(self):
        with patch("scripts.ci_native_tests.subprocess.run") as run:
            self.assertEqual(main(), 0)
        self.assertEqual(run.call_count, len(native_commands()))
        self.assertTrue(all(call.kwargs["check"] for call in run.call_args_list))

    def test_failure_stops_the_suite_and_is_not_reported_as_success(self):
        with patch("scripts.ci_native_tests.subprocess.run") as run:
            run.side_effect = subprocess.CalledProcessError(7, "test")
            with self.assertRaises(subprocess.CalledProcessError):
                main()
        self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
