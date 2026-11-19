"""Build VT for the Linux host tests or both arm64 iOS SDKs."""
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
from build_ghostty import PINS, collect_notices, source_archive, zig_host


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    pins = json.loads(PINS.read_text())
    mac = platform.system() == "Darwin"
    key, archive = ("zig_macos_arm64", "zig-macos.tar.xz") if mac else zig_host(platform.system(), platform.machine())
    src = source_archive(pins["source_url"], pins["source_sha256"], out / "download/ghostty.tar.gz", out / "source")
    tool = source_archive(pins[key + "_url"], pins[key + "_sha256"], out / "download" / archive, out / "tool")
    zig = tool / "zig"
    if subprocess.check_output([zig, "version"], text=True).strip() != pins["zig_version"]:
        raise RuntimeError("Unexpected Zig version")
    env = dict(os.environ, GIT_CEILING_DIRECTORIES=str(src.parent), ZIG_GLOBAL_CACHE_DIR=str(out / "cache"))
    slices = {"device": "aarch64-ios.17.0", "simulator": "aarch64-ios.17.0-simulator"} if mac else {"host": "native"}
    libraries = []
    for name, target in slices.items():
        prefix = out / name
        subprocess.run([zig, "build", "-Demit-lib-vt", "-Demit-xcframework=false", "-Doptimize=ReleaseFast",
                        "-Dtarget=" + target, "-Dcpu=baseline", "-j2", "--prefix", prefix,
                        "--cache-dir", out / ("local-" + name)], check=True, cwd=src, env=env)
        library = prefix / "lib/libghostty-vt.a"
        if library.stat().st_size == 0:
            raise RuntimeError(f"Empty static library: {library}")
        libraries.extend(["-library", library, "-headers", prefix / "include"])
    collect_notices(out, src, tool, out / "cache/p")
    if mac:
        framework = out / "GhosttyVT.xcframework"
        if framework.exists():
            shutil.rmtree(framework)
        subprocess.run(["xcodebuild", "-create-xcframework", *libraries, "-output", framework], check=True)


if __name__ == "__main__":
    main()
