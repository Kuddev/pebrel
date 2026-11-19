"""Open an empty patch batch from a published stable tag in a detached CI checkout."""

import argparse
import json
from pathlib import Path
import subprocess

if __package__:
    from .stable_release import validate_version
else:
    from stable_release import validate_version


def start_next_batch(repo: Path, version: str) -> str:
    validate_version(version)
    major, minor, patch = map(int, version.split("."))
    branch = f"release/auto-{major}.{minor}.{patch + 1}"
    ref = f"refs/heads/{branch}"
    tag = f"v{version}"

    def git(*args: str) -> str:
        return subprocess.check_output(["git", *args], cwd=repo, text=True).strip()

    published = git("rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
    if git("rev-parse", "HEAD") != published:
        raise ValueError("the checkout must match the published tag")

    def already_open() -> bool:
        if not git("ls-remote", "--heads", "origin", ref):
            return False
        git("fetch", "--quiet", "--no-tags", "origin", ref)
        git("merge-base", "--is-ancestor", published, "FETCH_HEAD")
        return True

    if already_open():
        return branch
    if git("status", "--porcelain") or git("branch", "--show-current"):
        raise ValueError("creating a batch requires a clean detached checkout")
    manifest = Path("packaging/release-batch.json")
    (repo / manifest).write_text(json.dumps({"base": tag, "fixes": []}, indent=2) + "\n",
                                 encoding="utf-8")
    git("add", "--", str(manifest))
    git("commit", "-m", f"Start the verified patch batch after {tag}", "--", str(manifest))
    try:
        git("push", "origin", f"HEAD:{ref}")
    except subprocess.CalledProcessError:
        # A concurrent maintainer may already have started the new branch.
        if not already_open():
            raise
    return branch


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--published-version", required=True)
    args = parser.parse_args()
    print(start_next_batch(Path.cwd(), args.published_version))
