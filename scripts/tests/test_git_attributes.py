import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "nebula_terminal/tests/ref"


class GitAttributeTests(unittest.TestCase):
    def test_embedded_shell_scripts_survive_autocrlf_checkout(self):
        paths = [
            "nebula_terminal/src/tty/connection.sh",
            "nebula_app/src/platform/pi_session.sh",
            *(
                path.relative_to(ROOT).as_posix()
                for path in sorted((ROOT / "nebula_app/res/shell").iterdir())
                if path.is_file()
            ),
        ]
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary) / "repository"
            checkout = Path(temporary) / "checkout"
            repository.mkdir()
            git = ["git", "-C", str(repository)]
            subprocess.run([*git, "init", "--quiet"], check=True)
            subprocess.run([*git, "config", "core.autocrlf", "true"], check=True)
            (repository / ".gitattributes").write_bytes((ROOT / ".gitattributes").read_bytes())
            originals = {}
            for path in paths:
                original = (ROOT / path).read_bytes().replace(b"\r\n", b"\n")
                self.assertIn(b"\n", original)
                originals[path] = original
                target = repository / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(original)
            # Prove autocrlf actually rewrites an unprotected shell script.
            (repository / "control.sh").write_bytes(b"function example() {\n    :\n}\n")
            subprocess.run([*git, "add", "--", ".gitattributes", "control.sh", *paths], check=True)
            subprocess.run(
                [*git, "checkout-index", "--all", f"--prefix={checkout.as_posix()}/"],
                check=True,
            )
            self.assertIn(b"\r\n", (checkout / "control.sh").read_bytes())
            for path, original in originals.items():
                with self.subTest(path=path):
                    actual = (checkout / path).read_bytes()
                    self.assertEqual(actual, original)
                    self.assertNotIn(b"\r\n", actual)

    def test_git_preserves_raw_terminal_streams(self):
        paths = sorted(str(path.relative_to(ROOT)).replace("\\", "/") for path in FIXTURES.rglob("*.recording"))
        self.assertTrue(paths)
        result = subprocess.check_output(
            ["git", "-C", str(ROOT), "check-attr", "-z", "text", "--", *paths]
        ).decode("utf-8").split("\0")[:-1]
        for path, attribute, value in zip(result[0::3], result[1::3], result[2::3]):
            with self.subTest(path=path):
                self.assertEqual((attribute, value), ("text", "unset"))

    def test_known_crlf_streams_keep_carriage_returns(self):
        for name in (
            "clear_underline", "colored_reset", "delete_lines", "row_reset",
            "saved_cursor", "saved_cursor_alt", "selective_erasure", "sgr",
        ):
            with self.subTest(name=name):
                self.assertIn(b"\r\n", (FIXTURES / name / "nebula.recording").read_bytes())

    def test_completion_snapshots_use_lf_on_every_checkout(self):
        paths = sorted(
            str(path.relative_to(ROOT)).replace("\\", "/")
            for path in (ROOT / "extra/completions").rglob("*")
            if path.is_file() and path.name != "README.md"
        )
        self.assertTrue(paths)
        result = subprocess.check_output(
            ["git", "-C", str(ROOT), "check-attr", "-z", "eol", "--", *paths]
        ).decode("utf-8").split("\0")[:-1]
        for path, attribute, value in zip(result[0::3], result[1::3], result[2::3]):
            with self.subTest(path=path):
                self.assertEqual((attribute, value), ("eol", "lf"))

    def test_legacy_hook_fixture_bytes_survive_autocrlf_checkout(self):
        # These files represent released payloads with exact ownership hashes.
        # Exercise Git's Windows checkout behavior even on a Unix CI runner.
        paths = [
            f"scripts/tests/fixtures/ai-hooks-v1.5.0/{name}"
            for name in ("opencode.js", "pi.ts")
        ]
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary) / "repository"
            checkout = Path(temporary) / "checkout"
            repository.mkdir()
            git = ["git", "-C", str(repository)]
            subprocess.run([*git, "init", "--quiet"], check=True)
            subprocess.run([*git, "config", "core.autocrlf", "true"], check=True)
            (repository / ".gitattributes").write_bytes((ROOT / ".gitattributes").read_bytes())
            originals = {}
            for path in paths:
                original = subprocess.check_output(["git", "-C", str(ROOT), "show", f"HEAD:{path}"])
                originals[path] = original
                target = repository / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(original)
            subprocess.run([*git, "add", "--", ".gitattributes", *paths], check=True)
            subprocess.run(
                [*git, "checkout-index", "--all", f"--prefix={checkout.as_posix()}/"],
                check=True,
            )
            for path, original in originals.items():
                with self.subTest(path=path):
                    self.assertEqual((checkout / path).read_bytes(), original)
