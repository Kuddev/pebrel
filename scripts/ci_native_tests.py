#!/usr/bin/env python3
"""Run the complete native suite with one Rust workspace feature graph."""

from __future__ import annotations

import argparse
import subprocess
import sys
from typing import Sequence


def native_commands(runner: str = "cargo") -> list[list[str]]:
    profile = ["--config", ".github/ci-profile.toml", "--profile", "ci"]
    workspace = ["--locked", "--workspace", "--features", "nebula/gpui-test-support", "--timings"]
    if runner == "cargo":
        tests = [["cargo", "test", *profile, *workspace]]
    elif runner == "nextest":
        tests = [
            [
                "cargo", "nextest", "run", "--config", ".github/ci-profile.toml",
                "--cargo-profile", "ci", *workspace, "--no-fail-fast", "--retries", "0",
            ],
            # nextest 不执行 doctest；显式补跑，提速不改变 workspace 的覆盖范围。
            ["cargo", "test", *profile, *workspace, "--doc"],
        ]
    else:
        raise ValueError(f"unknown test runner: {runner}")
    return [
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-v"],
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/conformance/tests", "-v"],
        *tests,
        # Link the test graph first: check can reuse compatible compiled
        # dependencies, while metadata-only check output cannot link the tests.
        # Keep the actual production feature graph independently checked.
        [
            "cargo", "check", "--locked", *profile, "-p", "nebula", "--bin", "pebrel",
            "--features", "gpui-shell", "--timings",
        ],
    ]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    # 发布脚本和本地入口保留 cargo 默认值，仅原生 CI 显式选择已安装的 nextest。
    parser.add_argument("--runner", choices=("cargo", "nextest"), default="cargo")
    arguments = parser.parse_args(argv)
    for command in native_commands(arguments.runner):
        print("Running:", " ".join(command), flush=True)
        subprocess.run(command, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
