from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.ci_plan import PLATFORMS, PlanError, _source_platforms, plan_matrices

SCRIPT = Path(__file__).resolve().parents[1] / "ci_plan.py"


class CiPlanCliTests(unittest.TestCase):
    def run_cli(self, event_name: str, payload: object, output: Path) -> subprocess.CompletedProcess[str]:
        repo = output.parent / "repo"
        pr = payload.get("pull_request") if isinstance(payload, dict) else None
        if isinstance(pr, dict) and isinstance(pr.get("draft"), bool) and "base" not in pr and "head" not in pr:
            if not (repo / ".git").exists():
                repo.mkdir()
                self.git(repo, "init", "-q")
                (repo / "Cargo.toml").write_text("base\n", encoding="utf-8")
                self.git(repo, "add", ".")
                self.git(repo, "commit", "-qm", "Create dependency fixture")
                (repo / "Cargo.toml").write_text("head\n", encoding="utf-8")
                self.git(repo, "commit", "-qam", "Update dependency fixture")
            pr["base"] = {"sha": self.git(repo, "rev-parse", "HEAD^").strip()}
            pr["head"] = {"sha": self.git(repo, "rev-parse", "HEAD").strip()}
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
            cwd=repo if repo.exists() else output.parent,
        )

    @staticmethod
    def git(repo: Path, *args: str) -> str:
        return subprocess.check_output(
            ["git", "-c", "user.name=CI fixture", "-c", "user.email=ci@example.invalid",
             "-c", "commit.gpgsign=false", "-c", "core.hooksPath=", *args],
            cwd=repo, text=True, encoding="utf-8",
        )

    @staticmethod
    def outputs(path: Path) -> dict[str, object]:
        lines = path.read_text(encoding="utf-8").splitlines()
        return {name: json.loads(value) for name, value in (line.split("=", 1) for line in lines)}

    def test_dependency_changes_have_the_same_full_coverage_in_draft_and_ready_prs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "github-output"
            result = self.run_cli("pull_request", {"pull_request": {"draft": True}}, output)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(
                output.read_text(encoding="utf-8").startswith(
                'native_matrix=[{"os":"ubuntu-24.04"},{"os":"windows-2022"},{"os":"macos-26"},'
                '{"os":"windows-11-arm"},{"os":"macos-26-intel"}]\n'
                'release_matrix=[{"os":"macos-26"},{"os":"macos-26-intel"}]\n'),
            )
            plan = self.outputs(output)
            ready_output = Path(directory) / "ready-output"
            ready = self.run_cli("pull_request", {"pull_request": {"draft": False}}, ready_output)
            self.assertEqual(ready.returncode, 0, ready.stderr)
            self.assertEqual(plan, self.outputs(ready_output))
            self.assertEqual(len(plan["check_matrix"]), 7)
            self.assertTrue(all(row["scheduled"] == "true" for row in plan["check_matrix"]))

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
            ("pull_request", {"pull_request": {"draft": False, "base": {}}}, "missing commit sha"),
            ("pull_request", {"pull_request": {"draft": False, "base": {"sha": "-bad"}, "head": {"sha": "a" * 40}}}, "invalid commit sha"),
            ("pull_request", {"pull_request": {"draft": False, "base": {"sha": "a" * 40}, "head": {"sha": "b" * 40}}}, "unavailable commit"),
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

    def test_renaming_a_windows_file_keeps_windows_validation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / "repo"
            repo.mkdir()
            self.git(repo, "init", "-q")
            old = repo / "nebula_terminal/src/tty/windows.rs"
            old.parent.mkdir(parents=True)
            old.write_text("// platform fixture\n", encoding="utf-8")
            self.git(repo, "add", ".")
            self.git(repo, "commit", "-qm", "Create Windows fixture")
            base = self.git(repo, "rev-parse", "HEAD").strip()
            self.git(repo, "mv", "nebula_terminal/src/tty/windows.rs", "nebula_terminal/src/common.rs")
            self.git(repo, "commit", "-qm", "Move platform fixture")
            payload = {"pull_request": {"draft": False, "base": {"sha": base},
                                        "head": {"sha": self.git(repo, "rev-parse", "HEAD").strip()}}}
            output = root / "github-output"
            result = self.run_cli("pull_request", payload, output)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual([row["os"] for row in self.outputs(output)["native_matrix"]],
                             ["ubuntu-24.04", "windows-2022", "windows-11-arm"])

    def test_removing_a_platform_condition_in_a_generic_file_keeps_both_hosts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo = root / "repo"
            repo.mkdir()
            self.git(repo, "init", "-q")
            source = repo / "nebula_app/src/welcome.rs"
            source.parent.mkdir(parents=True)
            source.write_text("#[cfg(windows)]\nfn welcome() {}\n", encoding="utf-8")
            self.git(repo, "add", ".")
            self.git(repo, "commit", "-qm", "Create conditional welcome fixture")
            base = self.git(repo, "rev-parse", "HEAD").strip()
            source.write_text("fn welcome() {}\n", encoding="utf-8")
            self.git(repo, "commit", "-qam", "Remove platform condition from fixture")
            payload = {"pull_request": {"draft": False, "base": {"sha": base},
                                        "head": {"sha": self.git(repo, "rev-parse", "HEAD").strip()}}}
            output = root / "github-output"
            result = self.run_cli("pull_request", payload, output)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual([row["os"] for row in self.outputs(output)["native_matrix"]],
                             ["ubuntu-24.04", "windows-2022", "windows-11-arm"])

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


class PrPathPolicyTests(unittest.TestCase):
    def test_platform_functions_inside_shared_paths_keep_native_coverage(self) -> None:
        cases = (
            ('#[cfg(windows)]\nfn welcome() {}', {"windows-2022", "windows-11-arm"}),
            ('#[cfg(all(feature = "ui", target_os = "macos"))]\nfn notify() {}', {"macos-26", "macos-26-intel"}),
            ('if cfg!(unix) { true } else { false }', {"macos-26", "macos-26-intel"}),
            ('use windows_sys::Win32;', {"windows-2022", "windows-11-arm"}),
            ('#[cfg(target_arch = "x86_64")]\nfn scan() {}', {p.runner for p in PLATFORMS}),
            ('#[cfg(feature = "gpui-test-support")]\nfn test() {}', set()),
        )
        for source, expected in cases:
            with self.subTest(source=source):
                self.assertEqual(_source_platforms(source), expected)

    def test_representative_prs_select_only_the_required_native_hosts(self) -> None:
        full = {p.runner for p in PLATFORMS}
        cases = (
            (["README.md", "docs/user-guide.md", "architecture/notes/scripts/ci/example.md"], set()),
            (["nebula_app/src/gpui_shell/settings_pane/colors.rs"], {"ubuntu-24.04"}),
            (["nebula_terminal/src/term/mod.rs", "nebula_app/src/backup_remote.rs"], {"ubuntu-24.04"}),
            (["nebula_terminal/src/tty/windows/mod.rs"], {"ubuntu-24.04", "windows-2022", "windows-11-arm"}),
            (["scripts/prepare-windows-runtime.ps1"], {"ubuntu-24.04", "windows-2022", "windows-11-arm"}),
            (["nebula_app/src/platform/macos/notifications.rs"], {"ubuntu-24.04", "macos-26", "macos-26-intel"}),
            (["nebula_app/src/notify_macos.rs"], {"ubuntu-24.04", "macos-26", "macos-26-intel"}),
            (["nebula_terminal/src/tty/unix.rs"], {"ubuntu-24.04", "macos-26", "macos-26-intel"}),
            (["nebula_app/src/platform/process.rs"], full),
            (["Cargo.lock"], full),
            (["nebula_terminal/Cargo.toml"], full),
            ([".github/workflows/linux-lua.yml"], full),
            (["scripts/ci_plan.py"], full),
            (["scripts/package-macos.sh"], full),
            (["nebula_terminal/src/scan_x86_64.rs"], full),
            (["architecture/dependencies.toml"], full),
            (["new_native_module/source.rs"], full),
        )
        for paths, expected in cases:
            for draft in (False, True):
                with self.subTest(paths=paths, draft=draft):
                    native, release = plan_matrices("pull_request", {"pull_request": {"draft": draft}}, paths)
                    self.assertEqual({row["os"] for row in native}, expected)
                    self.assertEqual({row["os"] for row in release}, expected & {"macos-26", "macos-26-intel"})

    def test_invalid_paths_do_not_silently_skip_validation(self) -> None:
        for path in ("", "/absolute.rs", "../outside.rs", "nebula_app/../unknown.rs", "windows\\path.rs"):
            with self.subTest(path=path), self.assertRaises(PlanError):
                plan_matrices("pull_request", {"pull_request": {"draft": False}}, [path])



if __name__ == "__main__":
    unittest.main()
