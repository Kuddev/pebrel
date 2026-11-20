"""Decide whether a reviewed stable branch has enough new verified fixes to release."""
import argparse
import json
from pathlib import Path
import re
import subprocess

if __package__:
    from .stable_release import validate_version
else:
    from stable_release import validate_version


def batch_ready(repo: Path, latest: str, version: str) -> bool:
    validate_version(version)
    if latest == f"v{version}":
        return False
    if not latest.startswith("v"):
        raise ValueError("latest release must be a stable version tag")
    validate_version(latest[1:])
    if tuple(map(int, version.split("."))) <= tuple(map(int, latest[1:].split("."))):
        raise ValueError("batch version must follow the latest stable release")
    manifest = repo / "packaging/release-batch.json"
    if not manifest.exists():
        return False
    batch = json.loads(manifest.read_text(encoding="utf-8"))
    if batch["base"] != latest:
        raise ValueError("batch base differs from the latest stable release")
    subprocess.run(["git", "merge-base", "--is-ancestor", latest, "HEAD"], cwd=repo, check=True)
    commits = set(subprocess.check_output(
        ["git", "rev-list", f"{latest}..HEAD"], cwd=repo, text=True).splitlines())
    fixes = batch["fixes"]
    if not isinstance(fixes, list):
        raise ValueError("fixes must be a list")
    ids = set()
    for fix in fixes:
        key = fix["id"]
        if not isinstance(key, str) or not re.fullmatch(r"[a-z0-9][a-z0-9._-]*", key) or key in ids:
            raise ValueError("each verified defect needs a distinct stable identifier")
        if fix["commit"] not in commits or not isinstance(fix["verification"], str) or not fix["verification"].strip():
            raise ValueError(f"{key} needs a commit in this batch and its verification result")
        ids.add(key)
    print(f"{len(ids)} verified fixes since {latest}")
    return len(ids) >= 10


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--latest", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    ready = batch_ready(Path.cwd(), args.latest, args.version)
    with args.output.open("a", encoding="utf-8") as output:
        output.write(f"ready={str(ready).lower()}\nversion={args.version}\nbase={args.latest}\n")
