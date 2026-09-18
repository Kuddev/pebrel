#!/usr/bin/env python3
"""Package a same-build native relay; Android never downloads an unpinned binary."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct


def package(binary: Path, output: Path, arch: str, commit: str) -> None:
    data = binary.read_bytes()
    machine = {"x86_64": 62, "aarch64": 183}[arch]
    if (len(data) < 64 or len(data) > 64 * 1024 * 1024 or data[:6] != b"\x7fELF\x02\x01"
            or struct.unpack_from("<H", data, 18)[0] != machine):
        raise ValueError("Wrong relay ELF architecture or size")
    if len(commit) != 40 or any(c not in "0123456789abcdef" for c in commit):
        raise ValueError("Expected exact source commit")
    destination = output / arch
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(binary, destination / "pebrel-relay")
    (destination / "manifest.json").write_text(json.dumps({
        "protocol": 2, "arch": arch, "commit": commit,
        "size": len(data), "sha256": hashlib.sha256(data).hexdigest(),
    }, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--arch", choices=["x86_64", "aarch64"], required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    package(args.binary, args.output, args.arch, args.commit)
