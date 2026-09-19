#!/usr/bin/env python3
"""Run the complete native suite with one Rust workspace feature graph."""

from __future__ import annotations

import subprocess
import sys


def native_commands() -> list[list[str]]:
    profile = ["--config", ".github/ci-profile.toml", "--profile", "ci"]
    return [
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-v"],
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/conformance/tests", "-v"],
        [
            "cargo", "test", "--locked", *profile, "--workspace",
            "--features", "nebula/gpui-test-support", "--timings",
        ],
        # Link the test graph first: check can reuse compatible compiled
        # dependencies, while metadata-only check output cannot link the tests.
        # Keep the actual production feature graph independently checked.
        [
            "cargo", "check", "--locked", *profile, "-p", "nebula", "--bin", "pebrel",
            "--features", "gpui-shell", "--timings",
        ],
    ]


def main() -> int:
    for command in native_commands():
        print("Running:", " ".join(command), flush=True)
        subprocess.run(command, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
