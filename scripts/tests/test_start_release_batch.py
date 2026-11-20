import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
import unittest

from scripts.start_release_batch import start_next_batch


class StartReleaseBatchTests(unittest.TestCase):
    def setUp(self):
        temp = TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        self.repo = root / "source"
        remote = root / "remote.git"
        subprocess.run(["git", "init", "-q", "--bare", str(remote)], check=True)
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("remote", "add", "origin", str(remote))
        self.manifest = self.repo / "packaging/release-batch.json"
        self.manifest.parent.mkdir()
        self.manifest.write_text('{"base":"v2.6.0","fixes":[{"id":"previous-fix"}]}\n')
        self.git("add", "packaging/release-batch.json")
        self.git("commit", "-qm", "Released source")
        self.git("tag", "v2.6.1")
        self.git("checkout", "-q", "--detach")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def test_two_successive_releases_start_empty_patch_batches(self):
        for previous, following in [("2.6.1", "2.6.2"), ("2.6.2", "2.6.3")]:
            branch = start_next_batch(self.repo, previous)
            self.assertEqual(branch, f"release/auto-{following}")
            head = self.git("ls-remote", "origin", f"refs/heads/{branch}").split()[0]
            self.assertEqual(self.git("rev-parse", head + "^"), self.git("rev-parse", "v" + previous))
            self.assertEqual(json.loads(self.git("show", head + ":packaging/release-batch.json")),
                             {"base": "v" + previous, "fixes": []})
            self.git("commit", "-q", "--allow-empty", "-m", "The next reviewed fix")
            self.git("push", "-q", "origin", "HEAD:refs/heads/" + branch)
            progressed = self.git("rev-parse", "HEAD")
            self.git("tag", "v" + following)
            self.git("checkout", "-q", "--detach", "v" + previous)
            self.assertEqual(start_next_batch(self.repo, previous), branch)
            self.assertEqual(self.git("ls-remote", "origin", branch).split()[0], progressed)
            self.git("checkout", "-q", "--detach", "v" + following)

    def test_existing_branch_from_another_baseline_is_preserved(self):
        branch = "refs/heads/release/auto-2.6.2"
        unrelated = subprocess.check_output(
            ["git", "commit-tree", self.git("rev-parse", "HEAD^{tree}"), "-m", "Other history"],
            cwd=self.repo, text=True).strip()
        self.git("push", "-q", "origin", f"{unrelated}:{branch}")
        with self.assertRaises(subprocess.CalledProcessError):
            start_next_batch(self.repo, "2.6.1")
        self.assertEqual(self.git("ls-remote", "origin", branch).split()[0], unrelated)

    def test_mismatched_or_dirty_checkout_cannot_create_a_batch(self):
        (self.repo / "local-file").touch()
        with self.assertRaisesRegex(ValueError, "clean detached"):
            start_next_batch(self.repo, "2.6.1")
        self.git("commit", "-q", "--allow-empty", "-m", "Unreleased source")
        with self.assertRaisesRegex(ValueError, "published tag"):
            start_next_batch(self.repo, "2.6.1")
        self.assertEqual(self.git("ls-remote", "--heads", "origin"), "")
