"""Collect and validate source-bound notices for the Linux relay binaries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib

from cargo_notices import collect


TARGETS = {"x86_64": "x86_64-unknown-linux-musl", "aarch64": "aarch64-unknown-linux-musl"}
MAX_NOTICE_BYTES = 16 * 1024 * 1024


def dependency_packages(tree: str, lock: dict, cargo_home: Path, staging: Path) -> list[dict]:
    packages = []
    for line in sorted(set(tree.splitlines())):
        match = re.fullmatch(r"([A-Za-z0-9_-]+) v([^ ]+)(?: \((.+)\))?", line)
        if not match:
            raise ValueError("Unexpected Cargo tree package identity")
        name, version, location = match.groups()
        if location == "proc-macro":
            location = None
        if name == "pebrel-mobile-link" and location:
            continue
        entries = [p for p in lock["package"] if p["name"] == name and p["version"] == version]
        if location or len(entries) != 1 or not entries[0].get("source", "").startswith("registry+"):
            raise ValueError("Relay notice collection requires an unambiguous registry dependency")
        entry = entries[0]
        candidates = sorted((cargo_home / "registry/cache").glob(f"*/{name}-{version}.crate"))
        archive = None
        for candidate in candidates:
            with candidate.open("rb") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() == entry["checksum"]:
                    archive = candidate
                    break
        if archive is None:
            raise ValueError(f"Missing checksum-matched Cargo archive for {name} {version}")
        source = staging / f"{name}-{version}"
        source.mkdir(parents=True)
        prefix = f"{name}-{version}/"
        with tarfile.open(archive) as crate:
            manifest_entry = crate.getmember(prefix + "Cargo.toml")
            if not manifest_entry.isfile():
                raise ValueError("Cargo manifest is not a regular archive entry")
            manifest = bounded_read(crate.extractfile(manifest_entry))
            metadata = tomllib.loads(manifest.decode("utf-8"))["package"]
            if metadata["name"] != name or metadata["version"] != version:
                raise ValueError("Cargo archive identity differs from the lockfile")
            (source / "Cargo.toml").write_bytes(manifest)
            for member in crate.getmembers():
                if not member.name.startswith(prefix) or not member.isfile():
                    continue
                relative = member.name[len(prefix):]
                if not (Path(relative).name.lower().startswith(("license", "licence", "copying", "notice"))
                        or relative == metadata.get("license-file")):
                    continue
                if "\\" in relative or any(part in ("", ".", "..") for part in relative.split("/")):
                    raise ValueError("Invalid license path in Cargo archive")
                output = source / relative
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_bytes(bounded_read(crate.extractfile(member)))
        packages.append({"name": name, "version": version, "source": entry["source"],
                         "manifest_path": str(source / "Cargo.toml"), "license": metadata.get("license"),
                         "license_file": metadata.get("license-file"), "authors": metadata.get("authors", []),
                         "repository": metadata.get("repository")})
    return packages


def _file_names(packages: list[dict]) -> set[str]:
    if not packages:
        raise ValueError("Missing native dependency notices")
    names = {"Cargo.lock", "LICENSE-SOURCE.json"}
    for package in packages:
        if not package.get("texts"):
            raise ValueError("Dependency notice has no license text")
        for text in package["texts"]:
            name = f"{package['name']}-{package['version']}/{text}"
            if "\\" in name or name.startswith("/") or any(part in ("", ".", "..") for part in name.split("/")):
                raise ValueError("Invalid dependency notice path")
            names.add(name)
    return names


def bounded_read(stream) -> bytes:
    data = stream.read(MAX_NOTICE_BYTES + 1)
    if len(data) > MAX_NOTICE_BYTES:
        raise ValueError("Dependency notice exceeds the size limit")
    return data


def directory_reader(directory: Path):
    root = directory.resolve(strict=True)

    def read(name: str) -> bytes:
        path = root / name
        if not path.resolve(strict=True).is_relative_to(root):
            raise ValueError("Dependency notice escapes its bundle")
        with path.open("rb") as stream:
            return bounded_read(stream)
    return read


def read_bundle(read, commit: str, arch: str) -> dict[str, bytes]:
    raw = read("manifest.json")
    manifest = json.loads(raw)
    if (manifest.get("schema_version") != 1 or manifest.get("commit") != commit or
            manifest.get("target") != TARGETS[arch]):
        raise ValueError("Dependency notices differ from relay source or target")
    names = _file_names(manifest["packages"])
    if set(manifest["files"]) != names:
        raise ValueError("Dependency notice inventory is incomplete")
    result, total = {"manifest.json": raw}, len(raw)
    for name in sorted(names):
        data = read(name)
        total += len(data)
        if total > MAX_NOTICE_BYTES or hashlib.sha256(data).hexdigest() != manifest["files"][name]:
            raise ValueError("Dependency notice is damaged or oversized")
        result[name] = data
    return result


def package(source: Path, output: Path, commit: str, arch: str) -> None:
    source = source.resolve(strict=True)
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Expected exact source commit")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True, encoding="utf-8").strip()
    if head != commit:
        raise ValueError("Notice source checkout differs from the relay binary")
    subprocess.run(["git", "diff", "--exit-code", commit, "--", "Cargo.toml", "Cargo.lock", "mobile/link", "mobile/ssh/licenses"],
                   cwd=source, stdout=subprocess.DEVNULL, check=True)
    target = TARGETS[arch]
    # 按真实构建选择依赖；workspace metadata 会额外激活其它成员的功能并下载无关源码。
    tree = subprocess.check_output([
        "cargo", "tree", "--color", "never", "--locked", "--offline", "--manifest-path", str(source / "mobile/link/Cargo.toml"),
        "-p", "pebrel-mobile-link", "--features", "relay", "--target", target,
        "--edges", "normal,build", "--prefix", "none", "--no-dedupe", "--format", "{p}",
    ], cwd=source, text=True, encoding="utf-8")
    lock = tomllib.loads((source / "Cargo.lock").read_text(encoding="utf-8"))
    cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))).expanduser()
    if not cargo_home.is_absolute():
        cargo_home = source / cargo_home
    license_source = source / "mobile/ssh/licenses/SOURCE.json"
    fallback = json.loads(license_source.read_text(encoding="utf-8"))
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="relay-notice-source-", dir=output.parent) as temporary:
        staging = Path(temporary).resolve()
        if not staging.is_relative_to(output.parent.resolve()):
            raise ValueError("Notice staging escaped its output directory")
        packages = collect(dependency_packages(tree, lock, cargo_home, staging), output,
                           license_source.parent / fallback["file"], fallback["sha256"])
    (output / "Cargo.lock").write_bytes((source / "Cargo.lock").read_bytes())
    (output / "LICENSE-SOURCE.json").write_bytes(license_source.read_bytes())
    files = {name: hashlib.sha256((output / name).read_bytes()).hexdigest() for name in sorted(_file_names(packages))}
    (output / "manifest.json").write_text(json.dumps({
        "schema_version": 1, "commit": commit, "target": target, "packages": packages, "files": files,
    }, indent=2) + "\n", encoding="utf-8", newline="\n")
    read_bundle(directory_reader(output), commit, arch)
    print(f"Relay notices: {len(packages)} resolved dependencies for {target}, source {commit}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--arch", choices=TARGETS, required=True)
    args = parser.parse_args()
    package(args.source, args.output, args.commit, args.arch)
