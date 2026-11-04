#!/usr/bin/env python3
"""Build receipts and publication for the requested 2.4.0 release only."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zipfile

if __package__:
    from scripts import android_release, stable_release
    from scripts.preview_release import sha256, write_atomic
else:
    import android_release
    import stable_release
    from preview_release import sha256, write_atomic


ROOT = Path(__file__).resolve().parents[1]
VERSION = "2.4.0"
BRANCH = "release/2.4.0-20261102"
WORKFLOW = ".github/workflows/release-2.4.0.yml"
REQUEST = "packaging/release-2.4.0.json"
ARTIFACT = "pebrel-2.4.0-verified"
NOTES = "docs/release-notes/v2.4.0.md"


def run(*command: str | Path, timeout: int = 180) -> str:
    return subprocess.check_output(
        [str(part) for part in command], cwd=ROOT, text=True, encoding="utf-8", timeout=timeout
    )


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    write_atomic(path, json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def request() -> dict:
    data = json.loads((ROOT / REQUEST).read_text(encoding="utf-8"))
    if not isinstance(data, dict) or data.get("version") != VERSION:
        raise ValueError("This release request must specify 2.4.0")
    phase = data.get("phase")
    required = {"version", "phase"}
    if phase == "publish":
        required |= {"source_commit", "build_run_id"}
        if not re.fullmatch(r"[0-9a-f]{40}", str(data.get("source_commit", ""))):
            raise ValueError("Publication requires the exact built source commit")
        if type(data.get("build_run_id")) is not int or data["build_run_id"] <= 0:
            raise ValueError("Publication requires a completed build run")
    elif phase != "build":
        raise ValueError("Unknown release phase")
    if set(data) != required:
        raise ValueError("Unexpected or missing release request fields")
    return data


def source_commit() -> str:
    commit = run("git", "rev-parse", "HEAD").strip()
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or os.environ.get("GITHUB_SHA", commit) != commit:
        raise ValueError("Checkout differs from the selected build commit")
    return commit


def prepare() -> None:
    data = request()
    source_commit()
    manifest = tomllib.loads((ROOT / "nebula_app/Cargo.toml").read_text(encoding="utf-8"))
    if manifest["package"]["version"] != VERSION:
        raise ValueError("Cargo package version differs from the release request")
    stable_release.validate_notes(ROOT / NOTES, VERSION)
    stable_release.validate_changelog(ROOT / "CHANGELOG.md", VERSION)
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        for key in ("version", "phase", "source_commit", "build_run_id"):
            output.write(f"{key}={data.get(key, '')}\n")
    print(f"Prepared {VERSION} {data['phase']}; regression suites are not part of this requested run")


def collect_android(output: Path, evidence: Path) -> None:
    commit = source_commit()
    if request()["phase"] != "build":
        raise ValueError("Android collection belongs to the build phase")
    apk = ROOT / "mobile/android/app/build/outputs/apk/preview/app-preview.apk"
    metadata = json.loads(apk.with_name("output-metadata.json").read_text(encoding="utf-8"))
    build_tools = Path(os.environ["ANDROID_HOME"]) / "build-tools/35.0.0"
    badging = run(build_tools / "aapt", "dump", "badging", apk)
    signature = run(build_tools / "apksigner", "verify", "--verbose", "--print-certs", apk)
    code = android_release.validate_identity(metadata, badging, signature, VERSION)
    declared = re.search(
        r"(?m)^\s*versionCode\s*=\s*(\d+)\s*$",
        (ROOT / "mobile/android/app/build.gradle.kts").read_text(encoding="utf-8"),
    )
    if not declared or code != int(declared.group(1)):
        raise ValueError("APK version code differs from its source")
    run(build_tools / "zipalign", "-c", "-P", "16", "4", apk)
    sys.path.insert(0, str(ROOT / "mobile/tools"))
    from verify_ghostty_apk import verify as verify_native
    from verify_native_relay_apk import verify as verify_relay
    from package_manual_relay import package as package_manual

    native = verify_native(apk)
    verify_relay(apk, commit, require_licenses=True)
    output.mkdir(parents=True, exist_ok=True)
    target = output / android_release.asset_name(VERSION)
    if target.exists():
        raise ValueError("Refusing to replace an existing APK")
    shutil.copyfile(apk, target)
    if sha256(target) != native["sha256"]:
        raise ValueError("APK changed while collecting it")
    kit_name = android_release.manual_asset_name(VERSION)
    if not kit_name or (output / kit_name).exists():
        raise ValueError("Manual relay output is missing or already exists")
    with tempfile.TemporaryDirectory(prefix="relay-kit-", dir=output.parent) as temporary:
        staged = Path(temporary) / kit_name
        package_manual(apk, staged, commit)
        shutil.copyfile(staged, output / kit_name)
        if sha256(staged) != sha256(output / kit_name):
            raise ValueError("Manual relay archive changed during collection")
    write_json(evidence / "android-package.json", {
        "schema_version": 1, "version": VERSION, "source_commit": commit,
        "validation": "identity, signature, alignment, both native ABIs and relay payload verified",
        "tests": "not run for this requested build", "version_code": code,
        "application_id": android_release.APPLICATION_ID,
        "certificate_sha256": android_release.CERTIFICATE_SHA256,
        "apk_sha256": sha256(target), "relay_sha256": sha256(output / kit_name),
    })


def collect(output: Path, evidence: Path) -> None:
    commit = source_commit()
    if request()["phase"] != "build":
        raise ValueError("Collection belongs to the build phase")
    stable_release.mark_platform_previews(output, VERSION)
    assets = stable_release.validate_assets(output, VERSION)
    for path in assets:
        if path.suffix == ".zip":
            with zipfile.ZipFile(path) as archive:
                if archive.testzip() is not None:
                    raise ValueError(f"Damaged portable ZIP: {path.name}")
    android = json.loads((evidence / "android-package.json").read_text(encoding="utf-8"))
    if (android.get("version") != VERSION or android.get("source_commit") != commit
            or android.get("apk_sha256") != sha256(output / android_release.asset_name(VERSION))
            or android.get("relay_sha256") != sha256(output / android_release.manual_asset_name(VERSION))):
        raise ValueError("Android receipt differs from the collected assets")
    notes = stable_release.validate_notes(ROOT / NOTES, VERSION)
    stable_release.validate_changelog(ROOT / "CHANGELOG.md", VERSION)
    checksums = stable_release.checksum_text(assets)
    write_atomic(output / "SHA256SUMS", checksums)
    write_atomic(output / "RELEASE_NOTES.md", stable_release.render_notes(notes, checksums))
    write_json(evidence / "source.json", {
        "schema_version": 1, "version": VERSION, "source_commit": commit,
        "build_run_id": int(os.environ["GITHUB_RUN_ID"]),
        "validation": "fresh platform builds and package checks",
        "regression_suites": "not run at the maintainer's request",
        "assets": {p.name: {"size": p.stat().st_size, "sha256": sha256(p)} for p in assets},
    })
    print(f"Collected and verified {len(assets)} packages from {commit}")


def api(path: str, *, missing_ok: bool = False) -> dict | None:
    result = subprocess.run(
        ["gh", "api", path], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=180
    )
    if result.returncode:
        try:
            payload = json.loads(result.stdout)
        except json.JSONDecodeError:
            payload = {}
        if missing_ok and str(payload.get("status")) == "404":
            return None
        raise ValueError(f"GitHub API failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def tag_commit(repo: str) -> str | None:
    ref = api(f"repos/{repo}/git/ref/tags/v{VERSION}", missing_ok=True)
    if ref is None:
        return None
    obj = ref["object"]
    if obj["type"] == "tag":
        obj = api(f"repos/{repo}/git/tags/{obj['sha']}")["object"]
    if obj["type"] != "commit":
        raise ValueError("Release tag does not resolve to a commit")
    return obj["sha"]


def publish(output: Path, evidence: Path) -> None:
    data = request()
    if data["phase"] != "publish":
        raise ValueError("Publication requires a publish request")
    repo = os.environ["GITHUB_REPOSITORY"]
    if repo != "Kuddev/pebrel":
        raise ValueError("This scoped release belongs to Kuddev/pebrel")
    commit, build_run = data["source_commit"], data["build_run_id"]
    source_commit()
    build = api(f"repos/{repo}/actions/runs/{build_run}")
    expected = {"status": "completed", "conclusion": "success", "event": "push",
                "head_sha": commit, "head_branch": BRANCH}
    if any(build.get(key) != value for key, value in expected.items()):
        raise ValueError("Selected build is not a successful matching branch/source run")
    if build.get("repository", {}).get("full_name") != repo:
        raise ValueError("Build repository differs")
    workflow = api(f"repos/{repo}/actions/workflows/{build['workflow_id']}")
    if workflow.get("path") != WORKFLOW:
        raise ValueError("Selected build used another workflow")
    listing = api(f"repos/{repo}/actions/runs/{build_run}/artifacts?per_page=100")
    if listing["total_count"] > 100:
        raise ValueError("Unexpectedly large build artifact list")
    matching = [a for a in listing["artifacts"] if a["name"] == ARTIFACT]
    if (len(matching) != 1 or matching[0]["expired"]
            or matching[0].get("workflow_run", {}).get("head_sha") != commit):
        raise ValueError("Missing, expired or mismatched verified artifact")
    run("git", "merge-base", "--is-ancestor", commit, "HEAD")
    changed = set(run("git", "diff", "--name-only", commit, "HEAD").splitlines())
    if changed - {REQUEST, NOTES, "CHANGELOG.md"}:
        raise ValueError("Product or workflow changed after the selected build")
    handoff = output.parent
    if output.exists() or evidence.exists():
        raise ValueError("Publication download directory already exists")
    run("gh", "run", "download", str(build_run), "--repo", repo,
        "--name", ARTIFACT, "--dir", handoff, timeout=900)
    receipt = json.loads((evidence / "source.json").read_text(encoding="utf-8"))
    if (receipt.get("source_commit") != commit or receipt.get("version") != VERSION
            or receipt.get("build_run_id") != build_run):
        raise ValueError("Downloaded receipt differs from the selected build")
    assets = stable_release.validate_assets(output, VERSION)
    actual = {p.name: {"size": p.stat().st_size, "sha256": sha256(p)} for p in assets}
    if actual != receipt.get("assets"):
        raise ValueError("Downloaded package hashes or sizes differ from the receipt")
    checksums = stable_release.checksum_text(assets)
    if (output / "SHA256SUMS").read_text(encoding="utf-8") != checksums:
        raise ValueError("Downloaded SHA256SUMS differs")
    notes = stable_release.validate_notes(ROOT / NOTES, VERSION)
    if stable_release.PENDING_SHA256 in notes or stable_release.STABLE_SHA256_PLACEHOLDER in notes:
        raise ValueError("Final source notes must contain the actual build hashes")
    if notes != (output / "RELEASE_NOTES.md").read_text(encoding="utf-8"):
        raise ValueError("Final notes differ from the verified build")
    entry = stable_release.validate_changelog(ROOT / "CHANGELOG.md", VERSION)
    if any(line not in entry for line in checksums.splitlines()):
        raise ValueError("Final Changelog is missing actual build hashes")
    existing_commit = tag_commit(repo)
    if existing_commit is not None and existing_commit != commit:
        raise ValueError("Refusing to move an existing release tag")
    metadata = api(f"repos/{repo}/releases/tags/v{VERSION}", missing_ok=True)
    if metadata is None:
        run("gh", "release", "create", f"v{VERSION}",
            *(str(p) for p in assets), str(output / "SHA256SUMS"),
            "--repo", repo, "--target", commit, "--title", f"Pebrel {VERSION}",
            "--notes-file", output / "RELEASE_NOTES.md", "--latest", timeout=900)
        metadata = api(f"repos/{repo}/releases/tags/v{VERSION}")
    stable_release.verify_release(metadata, output, VERSION, commit, tag_commit(repo))
    write_json(evidence / "published-release.json", metadata)
    print(metadata["html_url"])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("prepare", "android", "collect", "publish"))
    parser.add_argument("--output", type=Path, default=Path("release-dist"))
    parser.add_argument("--evidence", type=Path, default=Path("release-evidence"))
    args = parser.parse_args()
    if args.phase == "prepare":
        prepare()
    else:
        operation = {"android": collect_android, "collect": collect, "publish": publish}[args.phase]
        operation(args.output.resolve(), args.evidence.resolve())


if __name__ == "__main__":
    main()
