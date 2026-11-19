"""Check the unsigned iOS app's package and Mach-O load commands."""
import argparse
from pathlib import Path
import plistlib
import struct
import zipfile

BUNDLE_ID = "io.github.kuddev.pebrel.ios"
APP = "Payload/Pebrel.app/"


def check(condition, reason):
    if not condition:
        raise ValueError(reason)


def audit(path, version):
    with zipfile.ZipFile(path) as ipa:
        names = ipa.namelist()
        check(len(names) == len(set(names)), "Duplicate zip entries")
        for name in names:
            parts = Path(name).parts
            check(name == "Payload/" or name.startswith(APP), "Unexpected top-level content")
            check(".." not in parts and not any(part.endswith((".appex", ".mobileprovision"))
                  or part == "_CodeSignature" for part in parts), "Signed or extended payload")
        info = plistlib.loads(ipa.read(APP+"Info.plist"))
        check(info["CFBundleIdentifier"] == BUNDLE_ID, "Bundle identifier")
        check(info["CFBundleShortVersionString"] == version and info["CFBundleVersion"] == "1", "Version")
        check(info["MinimumOSVersion"] == "17.0", "Minimum OS")
        check(info["CFBundleSupportedPlatforms"] == ["iPhoneOS"], "Device platform")
        for key in ("NSCameraUsageDescription", "NSLocalNetworkUsageDescription"):
            check(isinstance(info[key], str) and info[key].strip(), key)
        check("_pebrel-pair._tcp" in info["NSBonjourServices"], "Bonjour service")
        executable = info["CFBundleExecutable"]
        check(executable == "Pebrel", "Executable name")
        binary = ipa.read(APP + executable)
        magic, cpu, _, filetype, commands, size, _, _ = struct.unpack_from("<8I", binary)
        check((magic, cpu, filetype) == (0xfeedfacf, 0x100000c, 2), "Expected arm64 Mach-O executable")
        end = 32 + size
        check(end <= len(binary), "Truncated load commands")
        pos, builds = 32, []
        for _ in range(commands):
            cmd, length = struct.unpack_from("<II", binary, pos)
            check(length >= 8 and pos + length <= end, "Invalid load command")
            if cmd == 0x32:
                check(length >= 24, "Truncated LC_BUILD_VERSION")
                builds.append(struct.unpack_from("<II", binary, pos+8))
            pos += length
        check(pos == end and builds == [(2, 17 << 16)], "Expected iOS 17.0 device binary")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ipa", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    audit(args.ipa, args.version)
    print(f"Verified unsigned {BUNDLE_ID} {args.version}, arm64, iOS 17.0")
