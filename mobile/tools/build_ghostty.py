#!/usr/bin/env python3
"""Build the pinned public VT core, with no desktop UI or private binary input.

Linux x86_64 is the reproducible builder used by CI; it produces both Android
ABIs. Upstream Zig dependency content hashes remain authoritative for transitive
sources. All downloads, extraction, caches and products stay under --output.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
PINS = ROOT / "mobile/android/ghostty/UPSTREAM.json"


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_archive(url: str, expected: str, archive: Path, extracted: Path) -> Path:
    archive.parent.mkdir(parents=True, exist_ok=True)
    if not archive.exists() or digest(archive) != expected:
        temporary = archive.with_suffix(archive.suffix + ".partial")
        with urllib.request.urlopen(url, timeout=120) as response, temporary.open("wb") as output:
            shutil.copyfileobj(response, output)
        if digest(temporary) != expected:
            temporary.unlink()
            raise RuntimeError(f"Source checksum mismatch: {url}")
        temporary.replace(archive)
    # A hash-specific extraction prevents stale products from another revision.
    destination = extracted / expected
    marker = destination / ".source-complete"
    if not marker.exists():
        destination.mkdir(parents=True, exist_ok=True)
        with tarfile.open(archive) as tar:
            tar.extractall(destination, filter="data")
        marker.write_text(expected + "\n", encoding="utf-8")
    directories = [entry for entry in destination.iterdir() if entry.is_dir()]
    if len(directories) != 1:
        raise RuntimeError(f"Expected one source root in {archive}")
    return directories[0]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "mobile/android/ghostty/build/upstream")
    parser.add_argument("--abi", choices=("arm64-v8a", "x86_64", "all"), default="all")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("The pinned builder runs on Linux x86_64 (CI or WSL).")
    pins = json.loads(PINS.read_text(encoding="utf-8"))
    output = args.output.resolve()
    ndk_value = os.environ.get("ANDROID_NDK_HOME")
    if not ndk_value:
        sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
        if not sdk:
            raise SystemExit("Set ANDROID_NDK_HOME or ANDROID_HOME for the pinned NDK.")
        ndk_value = str(Path(sdk) / "ndk" / pins["ndk_version"])
    ndk = Path(ndk_value).resolve()
    properties = (ndk / "source.properties").read_text(encoding="utf-8")
    if f"Pkg.Revision = {pins['ndk_version']}" not in properties:
        raise SystemExit(f"Expected Android NDK {pins['ndk_version']} at {ndk}")
    sources = source_archive(pins["source_url"], pins["source_sha256"], output / "downloads/ghostty.tar.gz", output / "sources")
    zig_root = source_archive(pins["zig_url"], pins["zig_sha256"], output / "downloads/zig.tar.xz", output / "tools")
    zig = zig_root / "zig"
    if subprocess.check_output([zig, "version"], text=True).strip() != pins["zig_version"]:
        raise SystemExit("Unexpected Zig version")
    environment = os.environ.copy()
    environment.update(ANDROID_NDK_HOME=str(ndk), ZIG_GLOBAL_CACHE_DIR=str(output / "cache/global"))
    targets = {"arm64-v8a": "aarch64", "x86_64": "x86_64"}
    for abi, architecture in targets.items():
        if args.abi not in (abi, "all"):
            continue
        prefix = output / abi
        command = [str(zig), "build", "-Demit-lib-vt", "-Doptimize=ReleaseFast",
                   f"-Dtarget={architecture}-linux-android.{pins['android_api']}", "-Dcpu=baseline",
                   "--prefix", str(prefix), "--cache-dir", str(output / "cache" / abi)]
        print(f"Building Ghostty {pins['revision']} for {abi}", flush=True)
        subprocess.run(command, cwd=sources, env=environment, check=True)
        library = prefix / "lib/libghostty-vt.a"
        if not library.is_file() or library.stat().st_size == 0:
            raise RuntimeError(f"Missing static library: {library}")
        (prefix / "SOURCE.json").write_text(json.dumps({**pins, "abi": abi,
            "library_sha256": digest(library)}, indent=2) + "\n", encoding="utf-8")
        notices = prefix / "licenses"
        notices.mkdir(exist_ok=True)
        shutil.copy2(sources / "LICENSE", notices / "Ghostty-MIT.txt")
        shutil.copy2(sources / "build.zig.zon", notices / "upstream-dependencies.zig.zon")
        shutil.copy2(zig_root / "LICENSE", notices / "Zig-MIT.txt")
        packages = output / "cache/global/p"
        if packages.is_dir():
            for package in sorted(packages.iterdir()):
                if not package.is_dir():
                    continue
                for name in ("LICENSE", "LICENSE.md", "LICENSE.txt", "LICENSE-MIT", "LICENSE-APACHE", "COPYING", "NOTICE"):
                    notice = package / name
                    if notice.is_file():
                        target = notices / package.name / name
                        target.parent.mkdir(exist_ok=True)
                        shutil.copy2(notice, target)
        print(f"Verified {abi}: {digest(library)}", flush=True)


if __name__ == "__main__":
    main()
