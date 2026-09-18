"""Bundle a freshly built GPUI desktop with native mobile pairing instructions."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import zipfile


def safe_name(name: str) -> str:
    name = name.replace("\\", "/")
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or ":" in name:
        raise ValueError(f"Unsafe package path: {name}")
    return name


def package(desktop: Path, output: Path, commit: str) -> Path:
    root = Path(__file__).resolve().parents[2]
    if not re.fullmatch(r"[a-f0-9]{40}", commit):
        raise ValueError("An exact source commit is required")
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    if actual != commit:
        raise ValueError("Requested commit differs from the checked-out source")
    output.mkdir(parents=True, exist_ok=True)
    destination = output / f"Pebrel-Windows-x64-mobile-preview-{commit[:7]}.zip"
    with desktop.open("rb") as source_file:
        desktop_digest = hashlib.file_digest(source_file, "sha256").hexdigest()
    record = {
        "source_commit": commit,
        "desktop_package_sha256": desktop_digest,
        "mode": "package_only",
        "tests": "See the matching CI run for targeted check results; real-device and visual acceptance remain manual",
        "files": {},
    }
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED) as target:
        def add(name: str, data: bytes) -> None:
            name = safe_name(name)
            if name in record["files"]:
                raise ValueError(f"Duplicate package entry: {name}")
            target.writestr(name, data)
            record["files"][name] = hashlib.sha256(data).hexdigest()

        with zipfile.ZipFile(desktop) as source:
            for entry in source.infolist():
                if not entry.is_dir():
                    add(entry.filename, source.read(entry))
        for required in ("pebrel.exe", "runtime/pebrel-hook.exe", "runtime/conpty.dll", "runtime/OpenConsole.exe"):
            if required not in record["files"]:
                raise ValueError(f"Missing desktop runtime: {required}")
        # Native Settings replaces the external Node/browser pairing launcher.
        for name in ("Start-Pebrel-Preview.cmd", "START-NATIVE-LAN.md", "SETTINGS-PREVIEW-0.4.9.md"):
            data = (root / "mobile/desktop" / name).read_bytes()
            if name.endswith(".cmd"):
                data = data.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
            add(name, data)
        add("SOURCE_COMMIT", (commit + "\n").encode())
        metadata = json.dumps(record, ensure_ascii=False, indent=2) + "\n"
        target.writestr("BUILD.json", metadata)
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    (output / "SHA256SUMS").write_text(f"{digest}  {destination.name}\n", encoding="utf-8")
    (output / "SOURCE_COMMIT").write_text(commit + "\n", encoding="utf-8")
    (output / "BUILD.json").write_text(metadata, encoding="utf-8")
    print(json.dumps({"package": str(destination), "sha256": digest, "source_commit": commit}))
    return destination


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    arguments = parser.parse_args()
    package(arguments.desktop, arguments.output, arguments.commit)
