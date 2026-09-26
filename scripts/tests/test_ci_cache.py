"""Exercise the cache identity code used by the composite action itself."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch


ACTION = Path(__file__).resolve().parents[2] / ".github/actions/rust-cache/action.yml"


class CacheIdentityTests(unittest.TestCase):
    @staticmethod
    def action_text() -> str:
        return ACTION.read_text(encoding="utf-8")

    def identity(self, *, compiler="rustc 1.97.1", sdk="15.0", **changes):
        source = ACTION.read_text(encoding="utf-8")
        body = re.search(r"python - <<'PY'\n(.*?)\n        PY", source, re.S)
        self.assertIsNotNone(body, "cache identity must remain executable from this action")
        code = textwrap.dedent(body.group(1))
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "outputs"
            environment = {
                "CACHE_OS": "Linux", "CACHE_ARCH": "X64", "CACHE_WORKLOAD": "ci-product",
                "CACHE_MANIFESTS": "pinned-manifests", "GITHUB_OUTPUT": str(output),
                "CACHE_REVISION": "source-revision",
                "CARGO_HOME": str(Path(temporary) / "cargo"), **changes,
            }

            def command(arguments, **kwargs):
                if arguments == ["rustc", "-vV"]:
                    return compiler
                self.assertEqual(arguments, ["xcrun", "--sdk", "macosx", "--show-sdk-version"])
                return sdk

            with patch.dict(os.environ, environment, clear=True), patch("subprocess.check_output", side_effect=command):
                exec(compile(code, str(ACTION), "exec"), {})
            return dict(line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines())

    def test_compiler_flags_and_workload_isolate_compiled_targets(self):
        baseline = self.identity()["key"]
        for change in (
            {"compiler": "rustc 1.98.0"}, {"RUSTFLAGS": "-C opt-level=1"},
            {"CARGO_PROFILE_RELEASE_LTO": "false"}, {"CACHE_WORKLOAD": "release"},
            {"CACHE_ARCH": "ARM64"}, {"ImageOS": "a-different-runner-image"},
        ):
            with self.subTest(change=change):
                self.assertNotEqual(baseline, self.identity(**change)["key"])

    def test_macos_sdk_and_deployment_floor_are_part_of_identity(self):
        baseline = self.identity(CACHE_OS="macOS", sdk="15.0")["key"]
        self.assertNotEqual(baseline, self.identity(CACHE_OS="macOS", sdk="16.0")["key"])
        self.assertNotEqual(baseline, self.identity(CACHE_OS="macOS", MACOSX_DEPLOYMENT_TARGET="13.0")["key"])

    def test_dependency_change_can_restore_compatible_previous_targets(self):
        before = self.identity(CACHE_MANIFESTS="before")
        after = self.identity(CACHE_MANIFESTS="after")
        self.assertNotEqual(before["key"], after["key"])
        self.assertEqual(before["restore-key"], after["restore-key"])
        self.assertTrue(after["key"].startswith(after["restore-key"]))

    def test_source_only_revision_and_package_labels_do_not_bust_dependencies(self):
        before = self.identity(CACHE_REVISION="one", PREVIEW_ID="first")
        after = self.identity(CACHE_REVISION="two", PREVIEW_ID="second")
        self.assertNotEqual(before["key"], after["key"])
        self.assertEqual(before["manifest-key"], after["manifest-key"])
        self.assertEqual(before["restore-key"], after["restore-key"])
        self.assertEqual(before["key"], self.identity(CACHE_REVISION="one", PREVIEW_ID="third")["key"])

    def test_summary_exposes_real_target_cache_outputs(self):
        source = self.action_text()
        self.assertIn(
            "value: ${{ steps.target.outputs.cache-matched-key }}",
            source,
        )
        self.assertIn(
            "value: ${{ steps.cache-summary.outputs.legacy-fallback-invoked }}",
            source,
        )
        self.assertIn(
            "value: ${{ steps.cache-summary.outputs.legacy-fallback-outcome }}",
            source,
        )
        self.assertIn(
            "TARGET_CACHE_HIT: ${{ steps.target.outputs.cache-hit }}",
            source,
        )
        self.assertIn(
            "TARGET_CACHE_MATCHED_KEY: ${{ steps.target.outputs.cache-matched-key }}",
            source,
        )
        self.assertIn("SAVE_KEY: ${{ steps.identity.outputs.key }}", source)
        self.assertIn("CACHE_WORKLOAD: ${{ inputs.key }}", source)
        self.assertIn("RUNNER_OS_VALUE: ${{ runner.os }}", source)
        self.assertIn("RUNNER_ARCH_VALUE: ${{ runner.arch }}", source)

        summary = source.split("- name: Summarize compiled target cache", 1)[1]
        summary = summary.split("\n    - name:", 1)[0]
        run_body = summary.split("      run: |", 1)[1]
        self.assertNotIn("${{", run_body)
        for field in (
            "Target cache exact hit",
            "Target cache matched key",
            "Save key",
            "Legacy fallback invoked",
            "Legacy fallback outcome",
        ):
            self.assertIn(field, run_body)

    def test_legacy_fallback_is_distinct_from_target_cache_hit(self):
        source = self.action_text()
        legacy = source.split("- name: Recover an existing combined cache during migration", 1)[1]
        self.assertIn("id: legacy-fallback", legacy)
        self.assertIn(
            "if: steps.target.outputs.cache-matched-key == '' && inputs.legacy-key != ''",
            legacy,
        )
        self.assertIn("LEGACY_OUTCOME: ${{ steps.legacy-fallback.outcome }}", source)
        self.assertIn("legacy-fallback-invoked=", source)
        self.assertIn("Legacy fallback status is separate and is not counted as target exact hit", source)
        self.assertNotIn("LEGACY_KEY:", source)
        self.assertNotIn("ACTIONS_RUNTIME_TOKEN", source)
        self.assertNotIn("GITHUB_TOKEN", source)

    @unittest.skipUnless(os.name != "nt" and shutil.which("bash"), "POSIX summary shell")
    def test_summary_executes_exact_partial_and_legacy_cache_cases(self):
        source = self.action_text().split("- name: Summarize compiled target cache", 1)[1]
        script = textwrap.dedent(source.split("      run: |\n", 1)[1].split("\n    - name:", 1)[0])
        cases = (
            ("true", "current-key", "false", "skipped", "false"),
            ("false", "older-compatible-key", "true", "skipped", "false"),
            ("", "", "true", "success", "true"),
            ("", "", "true", "failure", "true"),
        )
        for hit, matched, configured, outcome, invoked in cases:
            with self.subTest(hit=hit, matched=matched, outcome=outcome), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                marker = root / "must-not-be-created"
                workload = f"$(touch {marker})"
                environment = {
                    "PATH": os.environ["PATH"],
                    "GITHUB_STEP_SUMMARY": str(root / "summary"),
                    "GITHUB_OUTPUT": str(root / "outputs"),
                    "TARGET_CACHE_HIT": hit, "TARGET_CACHE_MATCHED_KEY": matched,
                    "SAVE_KEY": "new-save-key", "LEGACY_CONFIGURED": configured,
                    "LEGACY_OUTCOME": outcome, "CACHE_WORKLOAD": workload,
                    "RUNNER_OS_VALUE": "Linux", "RUNNER_ARCH_VALUE": "X64",
                }
                result = subprocess.run([shutil.which("bash"), "-c", script], env=environment,
                                        text=True, capture_output=True, check=False)
                self.assertEqual(result.returncode, 0, result.stderr)
                summary = (root / "summary").read_text()
                outputs = (root / "outputs").read_text()
                self.assertIn(f"Target cache exact hit: `{hit or 'false'}`", summary)
                self.assertIn(f"Target cache matched key: `{matched or '<none>'}`", summary)
                self.assertIn(f"legacy-fallback-invoked={invoked}\n", outputs)
                self.assertIn(f"legacy-fallback-outcome={outcome}\n", outputs)
                self.assertIn(workload, summary)
                self.assertFalse(marker.exists(), "summary values must never execute as shell code")

    @unittest.skipUnless(os.name == "nt", "Windows cache migration")
    def test_windows_migration_preserves_compiler_and_workload_boundaries(self):
        before = self.identity()
        after = self.identity(ImageOS="win22")
        self.assertEqual(before["restore-key"], after["migration-key"])
        for change in ({"compiler": "rustc 1.98.0"}, {"CACHE_WORKLOAD": "release"},
                       {"RUSTFLAGS": "-C opt-level=1"}):
            self.assertNotEqual(after["migration-key"], self.identity(**change)["migration-key"])


if __name__ == "__main__":
    unittest.main()
