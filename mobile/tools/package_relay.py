"""Package the same bounded source kit for Android deployment and PC pairing."""
from pathlib import Path
import argparse
import gzip
import tarfile


def package_relay(root: Path, destination: Path) -> None:
    # A bounded source-only kit: no private configuration, dependencies or user files.
    relay_files = (
        "Caddyfile", "Dockerfile", "compose.yaml", "connector.mjs", "init.mjs",
        "package-lock.json", "package.json", "protocol.mjs", "runtime-link.mjs", "server.mjs",
        "invite.mjs", "tls.mjs", "lan.mjs", "loopback-proxy.mjs", "pairing.mjs", "qr-page.mjs",
        "README.md", "THIRD-PARTY-NOTICES.md",
    )
    payload = [(root / "mobile/relay" / name, "relay/" + name) for name in relay_files]
    payload.append((root / "mobile/protocol/bridge-policy.json", "protocol/bridge-policy.json"))
    payload.append((root / "LICENSE", "LICENSE"))
    if sum(path.stat().st_size for path, _ in payload) > 2 * 1024 * 1024:
        raise ValueError("Relay source kit exceeds its 2 MiB input budget")
    with destination.open("wb") as target:
        with gzip.GzipFile(filename="", fileobj=target, mode="wb", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for path, name in payload:
                    if path.is_symlink() or not path.is_file():
                        raise ValueError(f"Relay kit input must be a regular source file: {path}")
                    entry = archive.gettarinfo(str(path), arcname=name)
                    entry.uid = entry.gid = 0
                    entry.uname = entry.gname = ""
                    entry.mtime = 0
                    entry.mode = 0o644
                    with path.open("rb") as source_file:
                        archive.addfile(entry, source_file)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    package_relay(Path(__file__).resolve().parents[2], args.output)
