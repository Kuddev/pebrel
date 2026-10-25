#!/usr/bin/env python3
"""Plan the native CI matrices before GitHub creates runner jobs.

The output file is the path normally supplied as ``GITHUB_OUTPUT``. Validate
the event before requesting runners; draft and ready PRs get the same coverage.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence


@dataclass(frozen=True)
class Platform:
    runner: str
    release_check: bool = False


# 平台目录也是必需检查名称的来源；草稿和 Ready 使用相同的风险策略。
PLATFORMS = (
    Platform("ubuntu-24.04"),
    Platform("windows-2022"),
    Platform("macos-26", release_check=True),
    Platform("windows-11-arm"),
    Platform("macos-26-intel", release_check=True),
)

FULL_EVENTS = frozenset(
    {"push", "merge_group", "workflow_dispatch", "workflow_call", "schedule"}
)


class PlanError(ValueError):
    """An event cannot be converted into a safe CI plan."""


def _matrices(selected: set[str] | None = None) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    platforms = [p for p in PLATFORMS if selected is None or p.runner in selected]
    return (
        [{"os": platform.runner} for platform in platforms],
        [{"os": platform.runner} for platform in platforms if platform.release_check],
    )


def _pr_changes(pull_request: dict[str, Any]) -> tuple[list[str], set[str]]:
    revisions = []
    for side in ("base", "head"):
        branch = pull_request.get(side)
        revision = branch.get("sha") if isinstance(branch, dict) else None
        if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", revision):
            raise PlanError(f"pull_request.{side}.sha must be a full commit SHA")
        revisions.append(revision)
    try:
        revisions[0] = subprocess.run(
            ["git", "merge-base", *revisions], check=True, capture_output=True, timeout=30,
        ).stdout.decode("ascii").strip()
        # 禁用 rename 合并，避免把平台文件移走后只按新路径选择验证平台。
        result = subprocess.run(
            ["git", "diff", "--name-status", "--no-renames", "-z", *revisions, "--"],
            check=True, capture_output=True, timeout=30,
        )
        fields = result.stdout.decode("utf-8").split("\0")
        if fields.pop() != "" or len(fields) % 2:
            raise PlanError("incomplete Git changed-file output")
        paths: list[str] = []
        platforms: set[str] = set()
        for status, path in zip(fields[::2], fields[1::2]):
            if status not in {"A", "M", "D", "T"}:
                raise PlanError(f"unsupported Git file status {status!r}")
            paths.append(path)
            if not path.endswith(".rs"):
                continue
            # welcome.rs 等普通路径内也包含平台专属函数。读两端的条件编译，
            # 而不是只看改动行，才能覆盖函数体修改以及删除平台条件的情况。
            for revision in ([revisions[1]] if status == "A" else [revisions[0]] if status == "D" else revisions):
                source = subprocess.run(
                    ["git", "show", f"{revision}:{path}"], check=True,
                    capture_output=True, timeout=30,
                ).stdout.decode("utf-8")
                platforms.update(_source_platforms(source))
        return paths, platforms
    except (OSError, UnicodeError, subprocess.SubprocessError) as exc:
        raise PlanError(f"cannot determine the complete PR diff: {exc}") from exc


def _source_platforms(source: str) -> set[str]:
    if re.search(r"\btarget_arch\s*=", source):
        return {p.runner for p in PLATFORMS}
    conditions = " ".join(re.findall(
        r"(?:#\s*\[\s*cfg(?:_attr)?\s*\([^]]*|\bcfg!\s*\([^)]*)", source,
    ))
    selected: set[str] = set()
    if re.search(r"\bwindows\b", conditions) or re.search(r"\b(?:windows_sys|windows_core|std::os::windows)::", source):
        selected.update(("windows-2022", "windows-11-arm"))
    if re.search(r"\b(?:macos|unix)\b", conditions) or re.search(r"\b(?:objc2|core_foundation)::", source):
        selected.update(("macos-26", "macos-26-intel"))
    return selected


def select_pr_platforms(paths: Sequence[str]) -> set[str]:
    """Use Linux for ordinary changes; add native hosts at platform boundaries."""
    selected: set[str] = set()
    for path in paths:
        if not path or path.startswith("/") or "\\" in path or ".." in path.split("/"):
            raise PlanError(f"invalid repository path {path!r}")
        parts = re.split(r"[/_.-]+", path.lower())
        name = path.rsplit("/", 1)[-1].lower()
        # 工具链、依赖和平台入口会改变所有宿主的编译图，必须走完整矩阵。
        if (path.startswith((".github/", ".cargo/", "packaging/", "docs/release-notes/"))
                or name in {"cargo.toml", "cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml"}
                or (path.startswith("architecture/") and not path.startswith("architecture/notes/"))
                or path.startswith(("scripts/ci_", "scripts/tests/test_ci_", "scripts/build-", "scripts/package-"))
                or name in {"stable_release.py", "preview_release.py", "test_stable_release.py"}
                or name.endswith((".asm", ".s", ".wgsl"))
                or any(part in {"simd", "sse2", "avx", "neon", "x86", "aarch64"} for part in parts)
                or path in {"nebula_app/src/main.rs", "nebula_app/src/gpui_shell/mod.rs"}):
            return {p.runner for p in PLATFORMS}
        if name.endswith((".md", ".rst")) or path.startswith("docs/"):
            continue
        selected.add("ubuntu-24.04")
        if any(part.startswith(("windows", "win32", "wsl")) for part in parts) or name.endswith(".ps1"):
            selected.update(("windows-2022", "windows-11-arm"))
        elif any(part.startswith(("macos", "darwin", "appkit", "cocoa")) or part == "unix" for part in parts) or name.endswith((".m", ".mm", ".metal")):
            selected.update(("macos-26", "macos-26-intel"))
        elif "platform" in parts or "tty" in parts:
            return {p.runner for p in PLATFORMS}
        elif not path.startswith(("nebula_", "scripts/", "mobile/", "assets/", "locales/", "extra/", "tools/")):
            # 新的顶层代码区域没有经过分类时扩大验证，不默认为安全。
            return {p.runner for p in PLATFORMS}
    return selected


def _reject_json_constant(value: str) -> None:
    raise ValueError(f"non-standard JSON constant {value}")


def _read_event(path: Path) -> dict[str, Any]:
    try:
        raw = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        raise PlanError(f"cannot read event payload {path}: {exc}") from exc

    try:
        payload = json.loads(raw, parse_constant=_reject_json_constant)
    except (json.JSONDecodeError, ValueError) as exc:
        raise PlanError(f"invalid event JSON in {path}: {exc}") from exc

    if not isinstance(payload, dict):
        raise PlanError("event payload must be a JSON object")
    return payload


def plan_matrices(
    event_name: str,
    payload: dict[str, Any],
    changed_paths: Sequence[str] | None = None,
) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    """Return native and release matrices for one validated event."""

    if event_name == "pull_request":
        pull_request = payload.get("pull_request")
        if not isinstance(pull_request, dict):
            raise PlanError("pull_request event must contain a pull_request object")
        draft = pull_request.get("draft")
        if not isinstance(draft, bool):
            raise PlanError("pull_request.draft must be a boolean")
        paths, platforms = _pr_changes(pull_request) if changed_paths is None else (changed_paths, set())
        return _matrices(select_pr_platforms(paths) | platforms)

    if event_name in FULL_EVENTS:
        return _matrices()

    supported = sorted(FULL_EVENTS | {"pull_request"})
    raise PlanError(f"unsupported event name {event_name!r}; expected one of {', '.join(supported)}")


def _compact_output(name: str, value: list[dict[str, str]]) -> str:
    return f"{name}={json.dumps(value, separators=(',', ':'), ensure_ascii=True)}"


def write_outputs(path: Path, native_matrix: list[dict[str, str]], release_matrix: list[dict[str, str]]) -> None:
    """Append both outputs in one write after planning has fully succeeded."""

    native = {row["os"] for row in native_matrix}
    release = {row["os"] for row in release_matrix}
    checks = [
        {"name": f"Tests ({p.runner})", "scheduled": str(p.runner in native).lower()}
        for p in PLATFORMS
    ] + [
        {"name": f"Release workspace ({p.runner})", "scheduled": str(p.runner in release).lower()}
        for p in PLATFORMS if p.release_check
    ]
    content = "\n".join(
        (
            _compact_output("native_matrix", native_matrix),
            _compact_output("release_matrix", release_matrix),
            _compact_output("check_matrix", checks),
        )
    ) + "\n"
    try:
        with path.open("a", encoding="utf-8", newline="\n") as output:
            output.write(content)
    except (OSError, UnicodeError) as exc:
        raise PlanError(f"cannot append CI plan to {path}: {exc}") from exc


def _arguments(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event-name", required=True)
    parser.add_argument("--event-path", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    arguments = _arguments(argv)
    try:
        payload = _read_event(arguments.event_path)
        native_matrix, release_matrix = plan_matrices(arguments.event_name, payload)
        write_outputs(arguments.output, native_matrix, release_matrix)
    except PlanError as exc:
        print(f"ci_plan: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
