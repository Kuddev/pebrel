#!/usr/bin/env python3
"""Generate an offline Scoop manifest from a published GitHub Release response."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re

if __package__:
    from scripts.stable_release import expected_asset_names, validate_version
else:
    from stable_release import expected_asset_names, validate_version


REPOSITORY = "https://github.com/Kuddev/pebrel"


def manifest_from_release(release: dict) -> dict:
    if release.get("draft") is not False or release.get("prerelease") is not False:
        raise ValueError("Scoop requires a published stable release")
    tag = release.get("tag_name", "")
    if not isinstance(tag, str) or not tag.startswith("v"):
        raise ValueError("Release must have a v-prefixed stable tag")
    version = tag[1:]
    validate_version(version)
    required = expected_asset_names(version)
    architecture = {}
    autoupdate = {}
    assets = release.get("assets", [])
    if not isinstance(assets, list):
        raise ValueError("Release assets must be an array")
    for scoop_arch, asset_arch in (("64bit", "x64"), ("arm64", "arm64")):
        name = f"Pebrel-v{version}-windows-{asset_arch}.zip"
        if name not in required:
            continue
        matches = [asset for asset in assets if isinstance(asset, dict) and asset.get("name") == name]
        if len(matches) != 1:
            raise ValueError(f"Expected exactly one published asset: {name}")
        asset = matches[0]
        url = f"{REPOSITORY}/releases/download/{tag}/{name}"
        digest = asset.get("digest", "")
        if asset.get("browser_download_url") != url:
            raise ValueError(f"Unexpected download URL for {name}")
        if asset.get("state") != "uploaded" or not isinstance(asset.get("size"), int) or asset["size"] <= 0:
            raise ValueError(f"Incomplete release asset: {name}")
        if not isinstance(digest, str) or not re.fullmatch(r"sha256:[a-fA-F0-9]{64}", digest):
            raise ValueError(f"Missing SHA256 digest for {name}")
        architecture[scoop_arch] = {"url": url, "hash": digest[7:].lower()}
        # Scoop 的自动更新器下载最终 URL 并重算哈希；这里不沿用旧版本摘要。
        autoupdate[scoop_arch] = {
            "url": f"{REPOSITORY}/releases/download/v$version/Pebrel-v$version-windows-{asset_arch}.zip"
        }
    return {
        "version": version,
        "description": "GPU-accelerated terminal with native UI and AI-agent integration",
        "homepage": REPOSITORY,
        "license": "GPL-3.0-or-later",
        "architecture": architecture,
        "pre_install": (
            "[IO.File]::WriteAllText((Join-Path $dir 'pebrel-distribution'), "
            '"scoop`n", [Text.UTF8Encoding]::new($false))'
        ),
        "bin": "pebrel.exe",
        "shortcuts": [["pebrel.exe", "Pebrel", "--gpui"]],
        "checkver": "github",
        "autoupdate": {"architecture": autoupdate},
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-json", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    release = json.loads(args.release_json.read_text(encoding="utf-8-sig"))
    if not isinstance(release, dict):
        parser.error("Release response must be an object")
    try:
        manifest = manifest_from_release(release)
    except ValueError as error:
        parser.error(str(error))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(manifest, ensure_ascii=False, indent=4) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
