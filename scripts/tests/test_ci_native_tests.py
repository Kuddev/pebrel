from __future__ import annotations

import subprocess
import re
from pathlib import Path
import tomllib
import unittest
from unittest.mock import patch

from scripts.ci_native_tests import main, native_commands


class NativeSuiteTests(unittest.TestCase):
    def test_required_lint_plans_runners_before_native_jobs(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text(encoding="utf-8")
        lint = workflow.split("\n  lint:\n", 1)[1].split("\n  native-tests:\n", 1)[0]
        self.assertIn("name: lint", lint)
        self.assertIn("Test CI contracts before matrix planning", lint)
        for contract in (
            "scripts.tests.test_ci_plan",
            "scripts.tests.test_ci_native_tests",
            "scripts.tests.test_stable_release",
            "scripts.tests.test_ci_cache",
            "scripts.tests.test_platform_cfg",
            "scripts.tests.test_pr_size_workflow",
        ):
            self.assertIn(contract, lint)
        self.assertIn("python scripts/ci_plan.py", lint)
        self.assertIn('--event-path "$GITHUB_EVENT_PATH"', lint)
        self.assertLess(
            lint.index("cargo fmt"),
            lint.index("Test CI contracts before matrix planning"),
        )
        self.assertLess(
            lint.index("Test CI contracts before matrix planning"),
            lint.index("python scripts/ci_plan.py"),
        )
        self.assertLess(
            lint.index("python3 scripts/check_platform_cfg.py"),
            lint.index("python scripts/ci_plan.py"),
        )
        platform_step = lint.split("Check platform cfg budget before native jobs", 1)[1].split("      - name:", 1)[0]
        self.assertNotIn("continue-on-error", platform_step)
        self.assertNotIn("--update", platform_step)
        for job, output in (("native-tests", "native_matrix"),
                            ("macos-release-check", "release_matrix")):
            body = workflow.split(f"\n  {job}:\n", 1)[1]
            body = re.split(r"\n  [a-z][a-z-]*:\n", body, maxsplit=1)[0]
            self.assertIn("needs: lint", body)
            self.assertIn(f"fromJSON(needs.lint.outputs.{output})", body)
            self.assertNotIn("pull_request.draft", body)
            self.assertNotIn("matrix.tier", body)
        self.assertIn("cargo check --locked --workspace --release", workflow)
        # Native validation never needs to retain checkout credentials.
        checkouts = re.findall(r"- uses: actions/checkout@[^\n]+\n(.*?)(?=      - |\Z)",
                               workflow, re.S)
        self.assertTrue(checkouts)
        for checkout in checkouts:
            self.assertIn("persist-credentials: false", checkout)

    def test_packages_run_after_merge_or_manual_dispatch_not_for_prs(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/preview-packages.yml").read_text(encoding="utf-8")
        events = workflow.split("\non:\n", 1)[1].split("\nconcurrency:", 1)[0]
        triggers = set(re.findall(r"^  ([a-z_]+):", events, re.M))
        self.assertEqual(triggers, {"push", "workflow_dispatch"})
        push = re.search(r"^  push:(.*?)(?=^  [a-z_]+:|\Z)", events, re.M | re.S)
        self.assertIsNotNone(push)
        self.assertIn("branches: [main]", push.group(1))
        self.assertIn("paths:", push.group(1))
        # Packaging still needs explicit dispatch to create a public release.
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.publish == true", workflow)
        stable = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        stable_events = stable.split("\non:\n", 1)[1].split("\nconcurrency:", 1)[0]
        self.assertEqual(set(re.findall(r"^  ([a-z_]+):", stable_events, re.M)),
                         {"push", "workflow_dispatch"})
        self.assertIn('tags: ["v*.*.*"]', stable_events)

    def test_every_pr_and_merge_group_runs_without_path_exclusions(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text(encoding="utf-8")
        events = workflow.split("\non:\n", 1)[1].split("\nconcurrency:", 1)[0]
        for event in ("pull_request", "merge_group"):
            declaration = re.search(rf"^  {event}:(.*?)(?=^  [a-z_]+:|\Z)", events, re.M | re.S)
            self.assertIsNotNone(declaration)
            self.assertNotIn("paths", declaration.group(1))
            self.assertNotIn("branches", declaration.group(1))
            if event == "pull_request":
                # Drafts already run the full matrix; readiness alone changes no source.
                self.assertIn("types: [opened, synchronize, reopened]", declaration.group(1))
                self.assertNotIn("ready_for_review", declaration.group(1))
            else:
                self.assertNotIn("types", declaration.group(1))
        self.assertIn("branches: [main]", events)
        self.assertNotIn("branches-ignore", events)
        self.assertNotIn("pull_request_target", workflow)
        self.assertNotIn("contents: write", workflow)
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request'", workflow)

    def test_arm_job_requires_native_execution_and_matching_console_runtime(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text(encoding="utf-8")
        self.assertIn("windows-11-arm", workflow)
        self.assertIn("host: aarch64-pc-windows-msvc", workflow)
        self.assertIn("OSArchitecture -ne 'Arm64'", workflow)
        self.assertIn("-Architecture $architecture", workflow)
        self.assertLess(workflow.index("Prepare pinned Windows console runtime"),
                        workflow.index("Test complete workspace"))
        preview = (root / ".github/workflows/preview-packages.yml").read_text(encoding="utf-8")
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
        for runner in ("cargo", "nextest"):
            with self.subTest(runner=runner):
                checks = [command for command in native_commands(runner) if command[:2] == ["cargo", "check"]]
                self.assertEqual(len(checks), 1)
                command = checks[0]
                self.assertEqual(command[command.index("--features") + 1], "gpui-shell")
                self.assertEqual(command[command.index("--bin") + 1], "pebrel")

    def test_nextest_runs_all_native_targets_and_preserves_doctests(self):
        commands = native_commands("nextest")
        tests = [command for command in commands if command[:3] == ["cargo", "nextest", "run"]]
        docs = [command for command in commands if command[:2] == ["cargo", "test"]]
        self.assertEqual(len(tests), 1)
        self.assertEqual(len(docs), 1)
        self.assertIn("--doc", docs[0])
        self.assertNotIn("--doc", tests[0])
        for command, profile_flag in ((tests[0], "--cargo-profile"), (docs[0], "--profile")):
            self.assertIn("--locked", command)
            self.assertIn("--workspace", command)
            self.assertIn("--timings", command)
            self.assertEqual(command[command.index("--features") + 1], "nebula/gpui-test-support")
            self.assertEqual(command[command.index(profile_flag) + 1], "ci")
            self.assertEqual(command[command.index("--config") + 1], ".github/ci-profile.toml")
            for excluded in ("--exclude", "--lib", "--skip", "--filterset", "--partition", "--no-run"):
                self.assertNotIn(excluded, command)
        self.assertIn("--no-fail-fast", tests[0])
        self.assertEqual(tests[0][tests[0].index("--retries") + 1], "0")
        self.assertLess(commands.index(tests[0]), commands.index(docs[0]))

    def test_native_workflow_installs_nextest_without_changing_release_callers(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text(encoding="utf-8")
        native = workflow.split("\n  native-tests:\n", 1)[1].split("\n  macos-release-check:\n", 1)[0]
        install = native.split("- name: Install the pinned native test runner", 1)[1].split("      - uses:", 1)[0]
        self.assertIn("uses: taiki-e/install-action@v2", install)
        self.assertIn("tool: cargo-nextest@0.9.146", install)
        self.assertNotIn("if:", install)
        self.assertIn("run: python scripts/ci_native_tests.py --runner nextest", native)
        release = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn("run: python scripts/ci_native_tests.py\n", release)

    def test_nextest_preserves_only_the_existing_shared_theme_fixture_mutex(self):
        root = Path(__file__).resolve().parents[2]
        config = tomllib.loads((root / ".config/nextest.toml").read_text(encoding="utf-8"))
        self.assertEqual(config["test-groups"], {"theme-studio": {"max-threads": 1}})
        self.assertEqual(config["profile"]["default"], {
            "overrides": [{
                "filter": "test(gpui_shell::settings_pane::theme_studio_tests::)",
                "test-group": "theme-studio",
            }],
        })

    def test_native_caches_are_default_branch_snapshots_not_per_pr_uploads(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/linux-lua.yml").read_text(encoding="utf-8")
        for job in ("native-tests", "macos-release-check"):
            body = workflow.split(f"\n  {job}:\n", 1)[1]
            body = re.split(r"\n  [a-z][a-z-]*:\n", body, maxsplit=1)[0]
            self.assertIn("revision: dependencies-v1", body)
            self.assertIn("save-if: ${{ github.ref == format('refs/heads/{0}', github.event.repository.default_branch) }}", body)
            save = body.split("- name: Save compiled workload", 1)[1].split("      - name:", 1)[0]
            self.assertIn("success() && steps.rust-cache.outputs.save-enabled == 'true'", save)
            self.assertNotIn("always()", save)
            self.assertIn("steps.rust-cache.outputs.cache-hit != 'true'", save)

    def test_fast_test_profile_preserves_runtime_checks_and_resets_named_overrides(self):
        root = Path(__file__).resolve().parents[2]
        config = tomllib.loads((root / ".github/ci-profile.toml").read_text(encoding="utf-8"))
        workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
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
        for runner in ("cargo", "nextest"):
            with self.subTest(runner=runner), patch("scripts.ci_native_tests.subprocess.run") as run:
                self.assertEqual(main(["--runner", runner]), 0)
                self.assertEqual([call.args[0] for call in run.call_args_list], native_commands(runner))
                self.assertTrue(all(call.kwargs["check"] for call in run.call_args_list))

    def test_failure_stops_the_suite_and_is_not_reported_as_success(self):
        for runner in ("cargo", "nextest"):
            for failed in range(len(native_commands(runner))):
                with self.subTest(runner=runner, failed=failed), patch("scripts.ci_native_tests.subprocess.run") as run:
                    run.side_effect = [None] * failed + [subprocess.CalledProcessError(7, "test")]
                    with self.assertRaises(subprocess.CalledProcessError):
                        main(["--runner", runner])
                    self.assertEqual(run.call_count, failed + 1)

    def test_unknown_runner_is_rejected_before_running_commands(self):
        with self.assertRaises(ValueError):
            native_commands("typo")


if __name__ == "__main__":
    unittest.main()
