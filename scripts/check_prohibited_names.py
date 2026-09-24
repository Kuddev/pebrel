#!/usr/bin/env python3
"""拒绝把竞品名称写入新增内容、提交说明或待推送提交。"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path
from typing import Iterable


PROHIBITED_PATTERNS = (
    re.compile(r"(?<![A-Za-z0-9_])tty[-_ ]?7(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])otty(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])netcaxx(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])netcat[-_ ]?ty(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])arxxx(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])cmux(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])ghostty(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])herdr(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])kaku(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])orca(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])tmux(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])wezterm(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])windows[-_ ]?terminal(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])alacritty(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])kitty(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])tabby(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])warp(?:[-_ ]?terminal)?(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])wave[-_ ]?terminal(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])electerm(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])termius(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])xshell(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])windterm(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])mobaxterm(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])iterm2?(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])hyper[-_ ]?terminal(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])rio[-_ ]?terminal(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])contour(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])extraterm(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])finalshell(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])royal[-_ ]?ts(?![A-Za-z0-9_])", re.IGNORECASE),
    re.compile(r"(?<![A-Za-z0-9_])securecrt(?![A-Za-z0-9_])", re.IGNORECASE),
)
PROHIBITED_EXAMPLES = (
    "tty7",
    "otty",
    "netcaxx",
    "netcatty",
    "arxxx",
    "cmux",
    "ghostty",
    "herdr",
    "kaku",
    "orca",
    "tmux",
    "wezterm",
    "Windows Terminal",
    "Alacritty",
    "Kitty",
    "Tabby",
    "Warp",
)

# 门禁必须能维护自己的关键词表；除此之外没有源码或文档例外。
EXEMPT_PATHS = {
    "scripts/check_prohibited_names.py",
    # These fixtures intentionally contain both accepted and rejected names so
    # the policy can test its own false-positive boundaries.
    "scripts/tests/test_prohibited_names.py",
    "scripts/tests/fixtures/prohibited_names/ordinary-comparison.md",
}
LEGAL_ATTRIBUTION_PATTERNS = (
    re.compile(
        r"^\s*(?:[#/;*=-]+\s*)?copyright\b[^;\n]*\b(?:project|contributors?|authors?)\b",
        re.IGNORECASE,
    ),
    re.compile(
        r"^\s*(?:[#/;*=-]+\s*)?(?:third[- ]party|upstream)\s+(?:notice|attribution|project)\b[^;\n]*",
        re.IGNORECASE,
    ),
)
DEPENDENCY_REFERENCE_PATTERNS = (
    re.compile(r"\bgit\s*=\s*[\"']https?://[^\"']+[\"']", re.IGNORECASE),
    re.compile(r"\bsource\s*=\s*[\"']git\+https?://[^\"']+[\"']", re.IGNORECASE),
)
PROTOCOL_COMPATIBILITY_PATTERNS = (
    re.compile(r"\bKITTY_[A-Z0-9_]+\b"),
    re.compile(r"\bkitty_(?:keyboard|seq|event|release|escape|input)[A-Za-z0-9_]*\b", re.IGNORECASE),
    re.compile(r"\b(?:kitty|tmux)/(?:legacy|style|protocol)\b", re.IGNORECASE),
    re.compile(r"\b(?:kitty|tmux)\s+(?:keyboard|protocol|escape|sequence)\b", re.IGNORECASE),
    re.compile(r"\bThemeFormat::(?:WindowsTerminal|Kitty|Ghostty|WezTerm|Alacritty)\b"),
    re.compile(
        r"unsupported theme format; use Pebrel JSON, Windows Terminal JSON, Kitty, Ghostty, WezTerm or Alacritty",
        re.IGNORECASE,
    ),
    re.compile(r"\\x1bPtmux;"),
)


def git(*args: str) -> bytes:
    return subprocess.check_output(["git", "-c", "i18n.logOutputEncoding=utf-8", *args], stderr=subprocess.DEVNULL)


def prohibited(text: str) -> bool:
    return any(pattern.search(text) for pattern in PROHIBITED_PATTERNS)


def source_without_allowed_occurrences(path: str, text: str) -> str:
    for pattern in LEGAL_ATTRIBUTION_PATTERNS:
        text = pattern.sub("", text)
    if Path(path).name in {"Cargo.toml", "Cargo.lock"} and any(
        pattern.search(text) for pattern in DEPENDENCY_REFERENCE_PATTERNS
    ):
        for pattern in DEPENDENCY_REFERENCE_PATTERNS:
            text = pattern.sub("", text)
    for pattern in PROTOCOL_COMPATIBILITY_PATTERNS:
        text = pattern.sub("", text)
    return text


def prohibited_source_line(path: str, text: str) -> bool:
    return prohibited(source_without_allowed_occurrences(path, text))


def changed_paths(*revision_args: str) -> list[str]:
    raw = git("diff", *revision_args, "--name-only", "-z", "--diff-filter=ACMR", "--", ".")
    return [path.decode("utf-8") for path in raw.split(b"\0") if path]


def added_lines(path: str, *revision_args: str) -> Iterable[tuple[int, str]]:
    patch = git("diff", *revision_args, "--no-ext-diff", "--unified=0", "--", path)
    for line_no, raw_line in enumerate(patch.decode("utf-8").splitlines(), 1):
        if raw_line.startswith("+") and not raw_line.startswith("+++"):
            yield line_no, raw_line[1:]


def scan_added_lines(*revision_args: str) -> list[str]:
    hits: list[str] = []
    for path in changed_paths(*revision_args):
        normalized = path.replace("\\", "/")
        if normalized in EXEMPT_PATHS:
            continue
        for line_no, line in added_lines(path, *revision_args):
            if prohibited_source_line(normalized, line):
                hits.append(f"{normalized}:{line_no}:{line}")
    return hits


def added_patch_lines(patch: bytes, merge_parent_count: int | None) -> Iterable[tuple[int, str]]:
    lines = patch.decode("utf-8").splitlines()
    if merge_parent_count is None:
        for line_no, raw_line in enumerate(lines, 1):
            if raw_line.startswith("+") and not raw_line.startswith("+++"):
                yield line_no, raw_line[1:]
        return

    in_hunk = False
    prefix = "+" * merge_parent_count
    for line_no, raw_line in enumerate(lines, 1):
        if raw_line.startswith("@@@"):
            in_hunk = True
            continue
        # Combined diff headers (including `+++ `) occur before the hunk.
        if in_hunk and raw_line.startswith(prefix):
            yield line_no, raw_line[merge_parent_count:]


def scan_pending_commits(revision_range: str) -> list[str]:
    hits: list[str] = []
    commits = git("rev-list", "--reverse", revision_range).decode("ascii").splitlines()
    for commit in commits:
        parents = git("rev-list", "--parents", "-n", "1", commit).decode("ascii").split()
        parent = parents[1] if len(parents) > 1 else None
        merge_parent_count = len(parents) - 1
        merge_commit = merge_parent_count > 1
        if merge_commit:
            # Combined diff reports only content resolved against all parents;
            # comparing to one parent would replay the other branch's changes.
            paths = git("diff-tree", "--cc", "--no-commit-id", "--name-only", "-z", "-r", commit)
        elif parent:
            # Comparing against the first parent includes a merge resolution's
            # actual additions for ordinary commits.
            paths = git("diff", "--name-only", "-z", "--diff-filter=ACMR", parent, commit, "--", ".")
        else:
            paths = git("diff-tree", "--root", "--no-commit-id", "--name-only", "-z", "-r", commit)
        for raw_path in paths.split(b"\0"):
            if not raw_path:
                continue
            path = raw_path.decode("utf-8")
            normalized = path.replace("\\", "/")
            if normalized in EXEMPT_PATHS:
                continue
            if merge_commit:
                patch = git("show", "--format=", "--cc", "--no-ext-diff", "--unified=0", commit, "--", path)
            elif parent:
                patch = git("diff", "--no-ext-diff", "--unified=0", parent, commit, "--", path)
            else:
                patch = git("show", "--format=", "--no-ext-diff", "--unified=0", commit, "--", path)
            for line_no, line in added_patch_lines(
                patch, merge_parent_count if merge_commit else None
            ):
                if prohibited_source_line(normalized, line):
                    hits.append(f"{commit}:{normalized}:{line_no}:{line}")
    return hits


def scan_message(path: Path) -> list[str]:
    return [
        f"{line_no}:{line}"
        for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1)
        if prohibited(line)
    ]


def resolved_commit(revision: str) -> str:
    if not revision or re.fullmatch(r"0+", revision):
        raise ValueError(f"missing or zero revision: {revision!r}")
    try:
        return git("rev-parse", "--verify", f"{revision}^{{commit}}").decode("ascii").strip()
    except (subprocess.CalledProcessError, UnicodeDecodeError) as exc:
        raise ValueError(f"cannot resolve revision {revision!r}") from exc


def checked_range(base: str, head: str) -> tuple[str, str]:
    base_commit = resolved_commit(base)
    head_commit = resolved_commit(head)
    try:
        common = git("merge-base", base_commit, head_commit).decode("ascii").strip()
    except (subprocess.CalledProcessError, UnicodeDecodeError) as exc:
        raise ValueError(f"base {base_commit} and head {head_commit} are unrelated") from exc
    if not common:
        raise ValueError(f"base {base_commit} and head {head_commit} have no common ancestor")
    return common, head_commit


def scan_commit_messages(revision_range: str) -> list[str]:
    hits: list[str] = []
    commits = git("rev-list", "--reverse", revision_range).decode("ascii").splitlines()
    for commit in commits:
        message = git("show", "-s", "--format=%B", commit).decode("utf-8")
        for line_no, line in enumerate(message.splitlines(), 1):
            if prohibited(line):
                hits.append(f"commit-message:{commit}:{line_no}:{line}")
    return hits


def scan_range(base: str, head: str) -> list[str]:
    base_commit, head_commit = checked_range(base, head)
    revision_range = f"{base_commit}..{head_commit}"
    hits = scan_commit_messages(revision_range)
    # Scan each commit's added lines so a name added and later deleted in the
    # same range cannot disappear from the final two-commit diff.
    hits.extend(scan_pending_commits(revision_range))
    return hits


def pending_push_range() -> str | None:
    result = subprocess.run(
        ["git", "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    upstream = result.stdout.decode("utf-8").strip()
    return f"{upstream}..HEAD" if result.returncode == 0 and upstream else None


def report(title: str, hits: list[str]) -> int:
    if not hits:
        return 0
    print(f"ERROR: {title}\n" + "\n".join(hits), file=sys.stderr)
    return 1


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(
            "usage: check_prohibited_names.py staged|message|push [message-file] | "
            "range --base BASE --head HEAD",
            file=sys.stderr,
        )
        return 2

    mode = argv[1]
    if mode == "staged":
        return report("staged additions contain prohibited product names", scan_added_lines("--cached"))
    if mode == "message":
        if len(argv) != 3:
            print("message mode requires the commit message path", file=sys.stderr)
            return 2
        return report("commit message contains prohibited product names", scan_message(Path(argv[2])))
    if mode == "push":
        revision_range = pending_push_range()
        if revision_range is None:
            print("No upstream branch; skipping pending-push name check")
            return 0
        message_text = git("log", "--format=%H:%s%n%b", revision_range).decode("utf-8")
        hits = [
            f"commit-message:{line_no}:{line}"
            for line_no, line in enumerate(message_text.splitlines(), 1)
            if prohibited(line)
        ]
        # 逐提交看新增行，避免“先加入、后删除”在最终 range diff 中被抵消。
        hits.extend(scan_pending_commits(revision_range))
        return report("commits pending push contain prohibited product names", hits)
    if mode == "range":
        if len(argv) != 6 or argv[2] != "--base" or argv[4] != "--head":
            print("range mode requires --base BASE --head HEAD", file=sys.stderr)
            return 2
        try:
            hits = scan_range(argv[3], argv[5])
        except (OSError, ValueError, subprocess.CalledProcessError) as exc:
            print(f"ERROR: cannot scan prohibited-name range: {exc}", file=sys.stderr)
            return 1
        return report("range contains prohibited product names", hits)

    print(f"unknown mode: {mode}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv))
    except (OSError, UnicodeError, subprocess.CalledProcessError) as exc:
        print(f"ERROR: cannot read naming-check input: {exc}", file=sys.stderr)
        raise SystemExit(1)
