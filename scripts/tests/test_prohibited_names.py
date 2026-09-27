from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from scripts import check_prohibited_names


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts" / "check_prohibited_names.py"
FIXTURES = ROOT / "scripts" / "tests" / "fixtures" / "prohibited_names"


def run_git(repository: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repository, check=True, stdout=subprocess.DEVNULL)


class ProhibitedNamesTests(unittest.TestCase):
    def check_staged(self, repository: Path):
        return subprocess.run(
            [sys.executable, str(CHECKER), "staged"], cwd=repository,
            text=True, encoding="utf-8", capture_output=True,
            env={**os.environ, "PYTHONUTF8": "1"},
        )

    def test_invalid_utf8_path_cannot_turn_into_an_empty_diff(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            # Git trees can contain these bytes even when the host filesystem
            # cannot. Never require a checkout or create an invalid OS filename.
            def write_object(kind, contents):
                return subprocess.check_output(
                    ["git", "hash-object", "-w", "-t", kind, "--stdin"],
                    input=contents, cwd=repository,
                ).decode("ascii").strip()

            empty_tree = write_object("tree", b"")
            base = subprocess.check_output(
                ["git", "commit-tree", empty_tree], input=b"baseline\n", cwd=repository,
            ).decode("ascii").strip()
            blob = write_object("blob", b"Ghostty\n")
            tree = write_object("tree", b"100644 bad-\xff.rs\0" + bytes.fromhex(blob))
            head = subprocess.check_output(
                ["git", "commit-tree", tree, "-p", base],
                input=b"invalid path fixture\n", cwd=repository,
            ).decode("ascii").strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository, text=True, encoding="utf-8", capture_output=True,
                env={**os.environ, "PYTHONUTF8": "1"},
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("utf-8", result.stderr.lower())

    def test_invalid_utf8_text_and_message_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            path = repository / "source.rs"
            path.write_bytes(b"safe \xff\n")
            run_git(repository, "add", "source.rs")
            staged = self.check_staged(repository)
            message = subprocess.run(
                [sys.executable, str(CHECKER), "message", str(path)], cwd=repository,
                text=True, encoding="utf-8", capture_output=True,
                env={**os.environ, "PYTHONUTF8": "1"},
            )
            for result in (staged, message):
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("utf-8", result.stderr.lower())

    def test_valid_unicode_and_control_character_paths_are_scanned(self) -> None:
        names = ["路径.rs"]
        if os.name != "nt":
            names.append("control\tline\n.rs")
        for name in names:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                run_git(repository, "init", "-q")
                path = repository / name
                path.write_text("safe\n", encoding="utf-8")
                run_git(repository, "add", "--", name)
                self.assertEqual(self.check_staged(repository).returncode, 0)
                path.write_text("Ghostty\n", encoding="utf-8")
                run_git(repository, "add", "--", name)
                result = self.check_staged(repository)
                self.assertEqual(result.returncode, 1)
                self.assertIn("Ghostty", result.stderr)

    def test_names_do_not_match_inside_unrelated_words(self) -> None:
        self.assertFalse(check_prohibited_names.prohibited("spotty network; tty72; netcatty_adapter"))
        self.assertTrue(check_prohibited_names.prohibited("Use Otty or TTY-7"))

    def test_ordinary_competitor_comparison_is_rejected(self) -> None:
        text = (FIXTURES / "ordinary-comparison.md").read_text(encoding="utf-8")
        self.assertTrue(check_prohibited_names.prohibited_source_line("comparison.md", text))

    def test_inline_legal_attribution_is_allowed_but_comparison_is_not(self) -> None:
        text = (FIXTURES / "legal-notice.txt").read_text(encoding="utf-8")
        self.assertFalse(check_prohibited_names.prohibited_source_line("source.rs", text))
        self.assertTrue(
            check_prohibited_names.prohibited_source_line("README.md", "Compare with Alacritty.")
        )
        self.assertTrue(
            check_prohibited_names.prohibited_source_line(
                "source.rs", "Copyright (c) Alacritty contributors; compare with Ghostty."
            )
        )

    def test_protocol_identifiers_are_allowed(self) -> None:
        text = (FIXTURES / "protocol.rs").read_text(encoding="utf-8")
        self.assertFalse(check_prohibited_names.prohibited_source_line("src/protocol.rs", text))

    def test_github_cli_text_is_not_a_competitor_name(self) -> None:
        command = (FIXTURES / "github-cli.sh").read_text(encoding="utf-8")
        documentation = (FIXTURES / "github-cli-docs.sh").read_text(encoding="utf-8")
        self.assertFalse(check_prohibited_names.prohibited_source_line("check.sh", command))
        self.assertFalse(check_prohibited_names.prohibited_source_line("check.sh", documentation))

    def test_real_cargo_git_dependency_is_allowed_but_readme_reference_is_not(self) -> None:
        dependency = 'gpui = { git = "https://github.com/ghostty/ghostty", rev = "deadbeef" }'
        self.assertFalse(check_prohibited_names.prohibited_source_line("Cargo.toml", dependency))
        self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", dependency))

    def test_staged_hook_configuration_is_not_exempt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            (repository / ".pre-commit-config.yaml").write_text(
                "# Compare with Ghostty.\nrepos: []\n", encoding="utf-8"
            )
            run_git(repository, "add", ".pre-commit-config.yaml")
            result = subprocess.run(
                [sys.executable, str(CHECKER), "staged"], cwd=repository,
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn(".pre-commit-config.yaml", result.stderr)

    def test_theme_format_message_does_not_exempt_an_appended_comparison(self) -> None:
        message = (
            "unsupported theme format; use Pebrel JSON, Windows Terminal JSON, "
            "Kitty, Ghostty, WezTerm or Alacritty"
        )
        self.assertFalse(check_prohibited_names.prohibited_source_line("src/theme.rs", message))
        self.assertTrue(check_prohibited_names.prohibited_source_line(
            "src/theme.rs", message + "; copied from Ghostty"
        ))

    def test_range_scans_added_text_and_new_commit_messages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "README.md").write_text("safe\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            (repository / "comparison.md").write_text("Compare with Ghostty.\n", encoding="utf-8")
            run_git(repository, "add", "comparison.md")
            run_git(repository, "commit", "-qm", "ordinary comparison")
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository,
                text=True,
                capture_output=True,
                env={**os.environ, "PYTHONUTF8": "1"},
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Ghostty", result.stderr)

    def test_range_scans_a_commit_message_even_when_text_is_legal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "README.md").write_text("safe\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            (repository / "README.md").write_text("still safe\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "Compare with Ghostty")
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("commit-message", result.stderr)

    def test_range_uses_merge_base_and_keeps_added_then_deleted_text(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "README.md").write_text("Historical Ghostty attribution\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            run_git(repository, "branch", "main")
            run_git(repository, "switch", "main")
            (repository / "README.md").write_text("safe on main\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "advance main")

            run_git(repository, "switch", "-c", "feature", base)
            (repository / "README.md").write_text("safe on feature\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "advance feature")
            clean_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            clean = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", "main", "--head", clean_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertEqual(clean.returncode, 0, clean.stderr)

            (repository / "temporary.md").write_text("Ghostty\n", encoding="utf-8")
            run_git(repository, "add", "temporary.md")
            run_git(repository, "commit", "-qm", "temporary comparison")
            (repository / "temporary.md").unlink()
            run_git(repository, "add", "-u")
            run_git(repository, "commit", "-qm", "remove temporary comparison")
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("temporary.md", result.stderr)

            unrelated_tree = subprocess.check_output(["git", "write-tree"], cwd=repository, text=True).strip()
            unrelated = subprocess.check_output(
                ["git", "commit-tree", unrelated_tree, "-m", "unrelated"],
                cwd=repository,
                text=True,
            ).strip()
            unrelated_result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", unrelated, "--head", clean_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(unrelated_result.returncode, 0)
            self.assertIn("cannot scan prohibited-name range", unrelated_result.stderr)

    def test_range_scans_merge_resolution_without_replaying_second_parent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "shared.md").write_text("base\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "baseline")
            run_git(repository, "branch", "main")

            run_git(repository, "switch", "-c", "feature")
            (repository / "shared.md").write_text("feature\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "feature change")
            feature_parent = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            run_git(repository, "switch", "main")
            (repository / "shared.md").write_text("main Ghostty\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "main change")
            main_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            run_git(repository, "switch", "feature")
            merge = subprocess.run(
                ["git", "merge", "--no-ff", "main", "-m", "merge main"],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(merge.returncode, 0, merge.stdout + merge.stderr)
            (repository / "shared.md").write_text("resolved safe\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "resolve safe merge")
            safe_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            safe_result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", main_head, "--head", safe_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertEqual(safe_result.returncode, 0, safe_result.stderr)

            run_git(repository, "switch", "-c", "feature-bad", feature_parent)
            merge_bad = subprocess.run(
                ["git", "merge", "--no-ff", "main", "-m", "merge main with bad resolution"],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(merge_bad.returncode, 0, merge_bad.stdout + merge_bad.stderr)
            (repository / "shared.md").write_text("resolved Ghostty\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "resolve bad merge")
            bad_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            bad_result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", main_head, "--head", bad_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(bad_result.returncode, 0)
            self.assertIn("shared.md", bad_result.stderr)

    def test_range_fails_closed_for_an_unknown_base(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", "missing", "--head", "HEAD"],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("cannot scan prohibited-name range", result.stderr)


if __name__ == "__main__":
    unittest.main()
