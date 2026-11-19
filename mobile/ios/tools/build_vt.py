"""Build VT for the Linux host tests or both arm64 iOS SDKs."""
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
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
    prefix = out / ("apple" if mac else "host")
    options = ["-Demit-xcframework=true"] if mac else ["-Demit-xcframework=false", "-Dtarget=native", "-Dcpu=baseline"]
    subprocess.run([zig, "build", "-Demit-lib-vt", "-Doptimize=ReleaseFast", *options, "-j2",
                    "--prefix", prefix, "--cache-dir", out / "local"], check=True, cwd=src, env=env)
    collect_notices(out, src, tool, out / "cache/p")
    if mac:
        verify_apple_slices(prefix / "lib/ghostty-vt.xcframework")
    elif (prefix / "lib/libghostty-vt.a").stat().st_size == 0:
        raise RuntimeError("Empty host VT archive")


def verify_apple_slices(framework):
    # Upstream skips iOS when SDK discovery fails. Check the two required slices
    # explicitly before caching or linking its universal Apple framework.
    info = plistlib.loads((framework / "Info.plist").read_bytes())
    found = set()
    for library in info["AvailableLibraries"]:
        if library["SupportedPlatform"] != "ios" or "arm64" not in library["SupportedArchitectures"]:
            continue
        archive = framework / library["LibraryIdentifier"] / library["LibraryPath"]
        if archive.stat().st_size > 0:
            found.add(library.get("SupportedPlatformVariant", "device"))
    if found != {"device", "simulator"}:
        raise RuntimeError(f"Expected arm64 iOS device and simulator VT slices, found {found}")


if __name__ == "__main__":
    main()
