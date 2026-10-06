#!/usr/bin/env python3
"""Build the public russh transport for Android and retain resolved license texts."""
from pathlib import Path
import hashlib
import json
import os
import platform
import shutil
import subprocess

from cargo_notices import collect


ROOT = Path(__file__).resolve().parents[2]
CRATE = ROOT / "mobile/ssh"
OUTPUT = ROOT / "mobile/android/ssh/build"
TARGETS = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}


def notices() -> None:
    license_source = json.loads((CRATE / "licenses/SOURCE.json").read_text(encoding="utf-8"))
    apache = CRATE / "licenses" / license_source["file"]
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--manifest-path", str(CRATE / "Cargo.toml")
    ], text=True, encoding="utf-8"))
    destination = OUTPUT / "assets/licenses/Russh"
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    packages = collect([package for package in metadata["packages"] if package["id"] in resolved],
                       destination, apache, license_source["sha256"])
    if not any(p["name"] == "russh" and p["texts"] for p in packages):
        raise RuntimeError("Missing russh license")
    shutil.copy2(CRATE / "Cargo.lock", destination / "Cargo.lock")
    shutil.copy2(CRATE / "licenses/SOURCE.json", destination / "LICENSE-SOURCE.json")
    (destination / "DEPENDENCIES.json").write_text(json.dumps(packages, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    ndk = Path(os.environ["ANDROID_NDK_HOME"])
    host = {"Linux": "linux-x86_64", "Windows": "windows-x86_64"}.get(platform.system())
    if host is None:
        raise RuntimeError("Use the pinned Linux or Windows Android NDK builder")
    compiler = ndk / "toolchains/llvm/prebuilt" / host / "bin"
    suffix = ".exe" if platform.system() == "Windows" else ""
    archive_tool = compiler / f"llvm-ar{suffix}"
    if not archive_tool.is_file() or "Pkg.Revision = 27.2.12479018" not in (ndk / "source.properties").read_text(encoding="utf-8"):
        raise RuntimeError("Expected Android NDK 27.2.12479018")
    records = []
    for abi, target in TARGETS.items():
        environment = os.environ.copy()
        # 直接调用 clang，避免 Windows 的 .cmd 包装器在带空格的 SDK 路径中再次解释参数。
        clang = compiler / f"clang{suffix}"
        environment.update({
            f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER": str(clang),
            f"CC_{target.replace('-', '_')}": str(clang),
            f"CFLAGS_{target.replace('-', '_')}": f"--target={target}26",
            f"AR_{target.replace('-', '_')}": str(archive_tool),
            "RUSTFLAGS": f"-C link-arg=--target={target}26 -C link-arg=-Wl,-z,max-page-size=16384",
        })
        subprocess.run(["cargo", "build", "--locked", "--release", "--target", target,
                        "--manifest-path", str(CRATE / "Cargo.toml"), "--target-dir", str(CRATE / "target")],
                       env=environment, check=True)
        library = CRATE / "target" / target / "release/libpebrel_ssh.so"
        destination = OUTPUT / "jniLibs" / abi
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(library, destination / library.name)
        records.append({"abi": abi, "sha256": hashlib.sha256(library.read_bytes()).hexdigest()})
    notices()
    provenance = {"russh": "0.62.2", "rust": subprocess.check_output(["rustc", "--version"], text=True, encoding="utf-8").strip(),
                  "ndk": "27.2.12479018", "api": 26, "libraries": records,
                  "lock_sha256": hashlib.sha256((CRATE / "Cargo.lock").read_bytes()).hexdigest()}
    (OUTPUT / "assets/licenses/Russh/BUILD.json").write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
