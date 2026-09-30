#!/usr/bin/env python3
"""Focused real-systemd acceptance on a disposable GitHub runner only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def verify(binary: Path) -> None:
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("Only disposable GitHub-hosted runners are allowed")
    # Never adopt, stop or remove any installation that predates this test.
    for path in ("/opt/pebrel-relay", "/etc/pebrel-relay", "/etc/systemd/system/pebrel-relay.service"):
        if Path(path).exists() or Path(path).is_symlink():
            raise RuntimeError("Existing relay installation is outside the fixture")
    binary = binary.resolve(strict=True)
    sha = hashlib.sha256(binary.read_bytes()).hexdigest()

    def run(*args: str) -> list[dict]:
        completed = subprocess.run(["sudo", str(binary), *args], capture_output=True, timeout=90)
        if completed.returncode:
            raise RuntimeError(f"Native relay {args[0]} failed (server details redacted)")
        return [json.loads(line) for line in completed.stdout.decode().splitlines() if line.startswith("{")]

    install = ("service-install", "--source", str(binary), "--sha256", sha,
               "--address", "127.0.0.1", "--port", "18443")
    try:
        # Match the private umask used by Android, not the permissive CI default.
        previous_umask = os.umask(0o077)
        try:
            run(*install)
        finally:
            os.umask(previous_umask)
        assert run("service-status")[-1]["ready"]
        run("service-stop")
        assert not run("service-status")[-1]["running"]
        run("service-start")
        assert run("service-status")[-1]["ready"]
        run("service-uninstall")
        state = run("service-status")[-1]
        assert not state["installed"] and state["configuration_retained"]
        run(*install)
        assert run("service-status")[-1]["ready"]
        run("service-uninstall", "--purge")
        state = run("service-status")[-1]
        assert not state["installed"] and not state["configuration_retained"]
        print("Native service: install, readiness, stop/start, uninstall, retained-config reinstall and purge passed")
    finally:
        # The executable itself verifies the exact ownership manifest. No broad
        # directory removal or fallback deletion on an ownership mismatch.
        if Path("/opt/pebrel-relay/installation.json").exists():
            run("service-uninstall", "--purge")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    verify(parser.parse_args().binary)
