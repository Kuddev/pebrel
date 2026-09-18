#!/usr/bin/env python3
"""Fail packaging if either deployable service is missing, stale or damaged."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zipfile


def verify(apk: Path, commit: str) -> None:
    with zipfile.ZipFile(apk) as archive:
        for arch, machine in (("x86_64", 62), ("aarch64", 183)):
            prefix = f"assets/native-relay/{arch}"
            metadata = json.loads(archive.read(f"{prefix}/manifest.json"))
            data = archive.read(f"{prefix}/pebrel-relay")
            if (len(data) < 64 or len(data) > 64 * 1024 * 1024 or
                    data[:6] != b"\x7fELF\x02\x01" or
                    struct.unpack_from("<H", data, 18)[0] != machine or
                    metadata.get("sha256") != hashlib.sha256(data).hexdigest() or
                    metadata.get("commit") != commit or metadata.get("arch") != arch or
                    metadata.get("protocol") != 2 or metadata.get("size") != len(data)):
                raise ValueError(f"Invalid or stale native relay: {arch}")
            print(f"Verified protocol-v2 {arch} service: {len(data)} bytes, source {commit}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("apk", type=Path)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    verify(args.apk, args.commit)
