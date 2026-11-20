#!/usr/bin/env python3
"""Create an offline manual kit from a verified APK, without fetching executables."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import tarfile
import zipfile

from verify_native_relay_apk import verify
from relay_notices import bounded_read, directory_reader, read_bundle


def package(apk: Path, output: Path, commit: str, licenses: Path | None = None) -> None:
    verify(apk, commit)
    if output.exists():
        raise ValueError("Refusing to overwrite an existing manual kit")
    root = Path(__file__).resolve().parents[1]
    members = {}
    with zipfile.ZipFile(apk) as archive:
        for arch in ("x86_64", "aarch64"):
            prefix = f"assets/native-relay/{arch}"
            binary = archive.read(f"{prefix}/pebrel-relay")
            manifest = archive.read(f"{prefix}/manifest.json")
            record = json.loads(manifest)
            assert record["commit"] == commit
            assert hashlib.sha256(binary).hexdigest() == record["sha256"]
            members[f"{arch}/pebrel-relay"] = binary
            members[f"{arch}/manifest.json"] = manifest
            members[f"{arch}/SHA256SUMS"] = (record["sha256"] + "  pebrel-relay\n").encode()
            def read(name: str) -> bytes:
                try:
                    stream = archive.open(f"{prefix}/licenses/{name}")
                except KeyError as error:
                    raise ValueError("APK relay notices are incomplete; supply source-matched notices with --licenses") from error
                with stream:
                    return bounded_read(stream)
            # 旧 APK 的补充材料也必须匹配二进制源码与目标，不能用 Android SSH 许可代替。
            notice_reader = directory_reader(licenses / arch) if licenses is not None else read
            for name, data in read_bundle(notice_reader, commit, arch).items():
                members[f"{arch}/licenses/{name}"] = data
    members["install.sh"] = (root / "relay-native/install.sh").read_bytes()
    members["INSTALL.md"] = (root / "relay-native/INSTALL.md").read_bytes()
    members["LICENSE"] = (root.parent / "LICENSE").read_bytes()
    members["SOURCE_COMMIT"] = (commit + "\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(output, "w:gz") as archive:
        for name, data in members.items():
            info = tarfile.TarInfo("pebrel-relay-manual/" + name)
            info.size = len(data)
            info.mode = 0o700 if name.endswith("/pebrel-relay") or name == "install.sh" else 0o600
            archive.addfile(info, io.BytesIO(data))
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_name(output.name + ".sha256").write_text(digest + "  " + output.name + "\n", encoding="utf-8")
    print(json.dumps({"file": str(output), "sha256": digest, "bytes": output.stat().st_size, "binary_source": commit}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--licenses", type=Path, help="Matching source-bound notices for an older APK")
    args = parser.parse_args()
    package(args.apk, args.output, args.commit, args.licenses)
