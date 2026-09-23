#!/usr/bin/env python3
"""Plan the native CI matrices before GitHub creates runner jobs.

The output file is the path normally supplied as ``GITHUB_OUTPUT``.  Planning
is deliberately separate from workflow steps: a draft pull request therefore
does not materialize jobs for the scarce platforms.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence


@dataclass(frozen=True)
class Platform:
    runner: str
    draft: bool
    release_check: bool = False


# One platform catalog owns both draft eligibility and release compilation.
PLATFORMS = (
    Platform("ubuntu-24.04", draft=True),
    Platform("windows-2022", draft=True),
    Platform("macos-26", draft=True, release_check=True),
    Platform("windows-11-arm", draft=False),
    Platform("macos-26-intel", draft=False, release_check=True),
)

FULL_EVENTS = frozenset(
    {"push", "merge_group", "workflow_dispatch", "workflow_call", "schedule"}
)


class PlanError(ValueError):
    """An event cannot be converted into a safe CI plan."""


def _matrices(draft: bool) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    selected = [platform for platform in PLATFORMS if not draft or platform.draft]
    return (
        [{"os": platform.runner} for platform in selected],
        [{"os": platform.runner} for platform in selected if platform.release_check],
    )


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


def plan_matrices(event_name: str, payload: dict[str, Any]) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    """Return native and release matrices for one validated event."""

    if event_name == "pull_request":
        pull_request = payload.get("pull_request")
        if not isinstance(pull_request, dict):
            raise PlanError("pull_request event must contain a pull_request object")
        draft = pull_request.get("draft")
        if not isinstance(draft, bool):
            raise PlanError("pull_request.draft must be a boolean")
        return _matrices(draft)

    if event_name in FULL_EVENTS:
        return _matrices(draft=False)

    supported = sorted(FULL_EVENTS | {"pull_request"})
    raise PlanError(f"unsupported event name {event_name!r}; expected one of {', '.join(supported)}")


def _compact_output(name: str, value: list[dict[str, str]]) -> str:
    return f"{name}={json.dumps(value, separators=(',', ':'), ensure_ascii=True)}"


def write_outputs(path: Path, native_matrix: list[dict[str, str]], release_matrix: list[dict[str, str]]) -> None:
    """Append both outputs in one write after planning has fully succeeded."""

    content = "\n".join(
        (
            _compact_output("native_matrix", native_matrix),
            _compact_output("release_matrix", release_matrix),
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
