import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
import unittest

from scripts.release_batch import batch_ready


class ReleaseBatchTests(unittest.TestCase):
    def setUp(self):
        directory = TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name)
        self.manifest = self.repo / "packaging/release-batch.json"
        self.manifest.parent.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("commit", "-q", "--allow-empty", "-m", "Last stable")
        self.git("tag", "v2.6.0")
        self.git("commit", "-q", "--allow-empty", "-m", "Reviewed backport")
        self.commit = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def fixes(self, count):
        return [{"id": f"fix-{n}", "commit": self.commit, "verification": f"Regression {n} passed"}
                for n in range(count)]

    def record(self, fixes, base="v2.6.0"):
        self.manifest.write_text(json.dumps({"base": base, "fixes": fixes}), encoding="utf-8")

    def ready(self):
        return batch_ready(self.repo, "v2.6.0", "2.6.1")

    def test_ninth_and_tenth_verified_defects(self):
        self.assertFalse(self.ready())
        self.record(self.fixes(9))
        self.assertFalse(self.ready())
        self.record(self.fixes(10))
        self.assertTrue(self.ready())
        self.assertFalse(batch_ready(self.repo, "v2.6.1", "2.6.1"))

    def test_duplicate_id_old_commit_and_absent_verification(self):
        for replacement in [{"id": "fix-0"}, {"commit": self.git("rev-parse", "v2.6.0")},
                            {"verification": " "}]:
            with self.subTest(replacement=replacement):
                fixes = self.fixes(10)
                fixes[-1].update(replacement)
                self.record(fixes)
                with self.assertRaises(ValueError):
                    self.ready()

    def test_latest_release_and_candidate_must_share_the_reviewed_baseline(self):
        self.record(self.fixes(10), base="v2.5.9")
        with self.assertRaisesRegex(ValueError, "batch base"):
            self.ready()
        self.record(self.fixes(10))
        self.git("checkout", "--orphan", "unrelated")
        self.git("commit", "-q", "--allow-empty", "-m", "Other baseline")
        with self.assertRaises(subprocess.CalledProcessError):
            self.ready()
