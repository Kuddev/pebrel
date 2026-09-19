#!/usr/bin/env python3
"""Build the public russh transport for Android and retain resolved license texts."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[2]
CRATE = ROOT / "mobile/ssh"
OUTPUT = ROOT / "mobile/android/ssh/build"
TARGETS = {"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}


def notices() -> None:
    license_source = json.loads((CRATE / "licenses/SOURCE.json").read_text(encoding="utf-8"))
    apache = CRATE / "licenses" / license_source["file"]
    if hashlib.sha256(apache.read_bytes()).hexdigest() != license_source["sha256"]:
        raise RuntimeError("Pinned Apache license text changed")
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--manifest-path", str(CRATE / "Cargo.toml")
    ], text=True))
    destination = OUTPUT / "assets/licenses/Russh"
    destination.mkdir(parents=True, exist_ok=True)
    packages = []
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    for package in metadata["packages"]:
        if package["id"] not in resolved or package["source"] is None:
            continue
        source = Path(package["manifest_path"]).parent
        target = destination / f"{package['name']}-{package['version']}"
        texts = []
        for notice in sorted(source.rglob("*")):
            if not notice.is_file() or not notice.name.lower().startswith(("license", "licence", "copying", "notice")):
                continue
            relative = notice.relative_to(source)
            output = target / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(notice, output)
            texts.append(str(relative))
        if package.get("license_file"):
            declared = source / package["license_file"]
            if declared.is_file() and str(declared.relative_to(source)) not in texts:
                target.mkdir(parents=True, exist_ok=True)
                shutil.copy2(declared, target / declared.name)
                texts.append(declared.name)
        selected = None
        if not texts:
            # These published crates omit the workspace's standard license file.
            # Choose the declared Apache option, retaining authors and origin.
            if "Apache-2.0" not in (package["license"] or ""):
                raise RuntimeError(f"Missing license text for {package['name']}")
            target.mkdir(parents=True, exist_ok=True)
            shutil.copy2(apache, target / "Apache-2.0.txt")
            shutil.copy2(source / "Cargo.toml", target / "upstream-Cargo.toml")
            texts = ["Apache-2.0.txt", "upstream-Cargo.toml"]
            selected = "Apache-2.0"
        packages.append({"name": package["name"], "version": package["version"],
                         "license": package["license"], "selected_license": selected,
                         "authors": package["authors"], "repository": package["repository"], "texts": texts})
    if not any(p["name"] == "russh" and p["texts"] for p in packages):
        raise RuntimeError("Missing russh license")
    shutil.copy2(CRATE / "Cargo.lock", destination / "Cargo.lock")
    shutil.copy2(CRATE / "licenses/SOURCE.json", destination / "LICENSE-SOURCE.json")
    (destination / "DEPENDENCIES.json").write_text(json.dumps(packages, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    ndk = Path(os.environ["ANDROID_NDK_HOME"])
    compiler = ndk / "toolchains/llvm/prebuilt/linux-x86_64/bin"
    if not (compiler / "llvm-ar").is_file():
        raise RuntimeError("Use the pinned Linux Android NDK builder")
    records = []
    for abi, target in TARGETS.items():
        environment = os.environ.copy()
        environment.update({
            f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER": str(compiler / f"{target}26-clang"),
            f"CC_{target.replace('-', '_')}": str(compiler / f"{target}26-clang"),
            f"AR_{target.replace('-', '_')}": str(compiler / "llvm-ar"),
            "RUSTFLAGS": "-C link-arg=-Wl,-z,max-page-size=16384",
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
    provenance = {"russh": "0.62.2", "rust": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                  "ndk": "27.2.12479018", "api": 26, "libraries": records,
                  "lock_sha256": hashlib.sha256((CRATE / "Cargo.lock").read_bytes()).hexdigest()}
    (OUTPUT / "assets/licenses/Russh/BUILD.json").write_text(json.dumps(provenance, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
