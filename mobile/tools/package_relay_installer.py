#!/usr/bin/env python3
"""Publish a fixed-version bootstrap and raw native assets, never a latest alias."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil


def package(source: Path, output: Path, commit: str, tag: str) -> None:
    if not re.fullmatch(r"[a-f0-9]{40}", commit):
        raise ValueError("An exact source commit is required")
    if not re.fullmatch(r"relay-[A-Za-z0-9._-]+", tag):
        raise ValueError("A separate, immutable relay release tag is required")
    template = Path(__file__).resolve().parents[1] / "relay-native/install-online.sh"
    script = template.read_text(encoding="utf-8").replace("@RELEASE_TAG@", tag)
    output.mkdir(parents=True, exist_ok=True)
    checksums = {}
    for arch in ("x86_64", "aarch64"):
        binary = source / arch / "pebrel-relay"
        manifest = json.loads((source / arch / "manifest.json").read_text(encoding="utf-8"))
        data = binary.read_bytes()
        sha = hashlib.sha256(data).hexdigest()
        if (manifest != {"protocol": 2, "arch": arch, "commit": commit,
                         "size": len(data), "sha256": sha}
                or not data.startswith(b"\x7fELF\x02\x01")):
            raise ValueError(f"Unverified {arch} artifact")
        name = f"pebrel-relay-linux-{arch}"
        shutil.copyfile(binary, output / name)
        checksums[name] = sha
        script = script.replace(f"@{arch.upper()}_SHA256@", sha)
    if re.search(r"@[A-Z0-9_]+@", script):
        raise ValueError("Unresolved installer placeholder")
    (output / "pebrel-relay.sh").write_text(script, encoding="utf-8", newline="\n")
    checksums["pebrel-relay.sh"] = hashlib.sha256((output / "pebrel-relay.sh").read_bytes()).hexdigest()
    (output / "build.json").write_text(json.dumps({
        "source": commit, "tag": tag, "protocol": 2, "sha256": checksums,
    }, indent=2) + "\n", encoding="utf-8")
    checksums["build.json"] = hashlib.sha256((output / "build.json").read_bytes()).hexdigest()
    (output / "SHA256SUMS").write_text("".join(
        f"{sha}  {name}\n" for name, sha in sorted(checksums.items())
    ), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    package(args.source, args.output, args.commit, args.tag)
