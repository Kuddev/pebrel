from dataclasses import dataclass, field
import os
from pathlib import Path, PurePosixPath


BUDGET_PATH = "architecture/file-budgets.txt"
SOURCE_SUFFIXES = {".rs", ".py", ".ps1", ".mjs", ".js", ".sh", ".lua", ".luau"}
EXCLUDED = {"node_modules", "__pycache__"}


def line_count(content: bytes) -> int:
    return content.count(b"\n") + int(bool(content) and not content.endswith(b"\n"))


def relative_path(value: str) -> str:
    path = PurePosixPath(value)
    if (not value or path.is_absolute() or ".." in path.parts
            or "\\" in value or ":" in value or path.as_posix() != value
            or any(part.startswith(".") for part in path.parts)):
        raise ValueError(f"invalid first-party path: {value!r}")
    return value


def checked_path(root: Path, relative: str) -> Path:
    path = PurePosixPath(relative)
    if path.is_absolute() or "\\" in relative or ":" in relative:
        raise ValueError(f"invalid repository path: {relative}")
    boundary = root.resolve()
    current = boundary
    for component in path.parts:
        current = current.parent if component == ".." else current / component
        if not current.is_relative_to(boundary) or current.is_symlink():
            raise ValueError(f"out-of-repository or symlinked source path: {relative}")
    return current


@dataclass
class Budget:
    limit: int
    roots: tuple[str, ...]
    exceptions: dict[str, int]
    # 旧根 -> 新根。只有基线里确实存在旧根时才生效，用来让整目录改名保留覆盖与例外额度。
    moves: dict[str, str] = field(default_factory=dict)

    @classmethod
    def parse(cls, text: str) -> "Budget":
        limit = None
        roots = []
        exceptions = {}
        moves = {}
        for number, record in enumerate(text.splitlines(), 1):
            if not record.strip():
                continue
            fields = record.split()
            if fields[0] == "moved":
                if len(fields) != 3:
                    raise ValueError(f"{BUDGET_PATH}:{number}: expected moved <old-root> <new-root>")
                old, new = relative_path(fields[1]), relative_path(fields[2])
                if old == new or old in moves or new in moves.values():
                    raise ValueError(f"invalid or duplicate root move: {old} -> {new}")
                moves[old] = new
                continue
            if len(fields) != 2:
                raise ValueError(f"{BUDGET_PATH}:{number}: expected two fields")
            name, value = fields
            if name == "limit":
                if limit is not None or not value.isascii() or not value.isdecimal() or int(value) < 1:
                    raise ValueError("invalid or duplicate line limit")
                limit = int(value)
            elif name == "root":
                relative_path(value)
                if value in roots:
                    raise ValueError(f"duplicate source root: {value}")
                roots.append(value)
            else:
                relative_path(name)
                if name in exceptions or not value.isascii() or not value.isdecimal():
                    raise ValueError(f"invalid or duplicate exception: {name}")
                exceptions[name] = int(value)
        if limit is None or not roots:
            raise ValueError("line budget needs a limit and source roots")
        for name, count in exceptions.items():
            if count <= limit or PurePosixPath(name).suffix not in SOURCE_SUFFIXES:
                raise ValueError(f"invalid exception: {name} ({count})")
            if not any(name.startswith(root + "/") for root in roots):
                raise ValueError(f"exception outside source roots: {name}")
        for root in roots:
            if any(root.startswith(other + "/") for other in roots if other != root):
                raise ValueError(f"overlapping source root: {root}")
        for old, new in moves.items():
            if new not in roots:
                raise ValueError(f"moved root target is not a source root: {new}")
            if any(old == root or old.startswith(root + "/") or root.startswith(old + "/") for root in roots):
                raise ValueError(f"moved root source overlaps current roots: {old}")
        return cls(limit, tuple(roots), exceptions, moves)


def source_files(root: Path, roots: tuple[str, ...]):
    def fail(error):
        raise error

    for relative in roots:
        directory = checked_path(root, relative)
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError(f"missing or symlinked source root: {relative}")
        for parent, directories, files in os.walk(directory, onerror=fail):
            excluded = EXCLUDED | ({"target"} if "Cargo.toml" in files else set())
            directories[:] = sorted(
                name for name in directories
                if name not in excluded
            )
            for name in directories + files:
                if (Path(parent) / name).is_symlink():
                    raise ValueError(f"symlink in first-party sources: {parent}/{name}")
            for name in sorted(files):
                path = Path(parent) / name
                if path.suffix in SOURCE_SUFFIXES:
                    yield path


def check(root: Path, budget: Budget, previous=None, read_previous=None, list_previous=None):
    errors = []
    notices = []
    counts = {
        path.relative_to(root.resolve()).as_posix(): line_count(path.read_bytes())
        for path in source_files(root, budget.roots)
    }
    if not counts:
        errors.append("no first-party sources found")
    for old in budget.moves:
        if checked_path(root, old).exists():
            errors.append(f"{old}: moved source root still exists")
    if previous is not None:
        if budget.limit > previous.limit:
            errors.append("line limit may not increase against the base revision")
        moved = {old for old in budget.moves if old in previous.roots}
        # 只映射基线里确实存在的旧根；合并后残留的记录不能改写后续 PR 的例外名。
        base_name = lambda name: next(
            (old + name[len(new):] for old, new in budget.moves.items()
             if old in moved and name.startswith(new + "/")), name)
        if not set(previous.roots).issubset(set(budget.roots) | moved):
            errors.append("source roots may not be removed from the budget")
        for old, new in budget.moves.items():
            if old not in moved:
                notices.append(f"{old}: base has no such root; remove the stale move record")
            elif new in previous.roots:
                errors.append(f"{new}: move target was already a source root in base")
            elif list_previous is None:
                errors.append(f"{old}: cannot verify moved sources without the base revision")
            else:
                # 改名不能顺带删掉文件：基线旧根下的每个源码文件都要在新根的同一相对位置出现。
                for name in list_previous(old):
                    if PurePosixPath(name).suffix in SOURCE_SUFFIXES and new + name[len(old):] not in counts:
                        errors.append(f"{name}: moved root lost {new + name[len(old):]}")
        for name, limit in budget.exceptions.items():
            if base_name(name) not in previous.exceptions or limit > previous.exceptions[base_name(name)]:
                errors.append(f"{name}: new or increased exception against base revision")
    for name, count in counts.items():
        limit = budget.exceptions.get(name, budget.limit)
        if previous is not None and base_name(name) in previous.exceptions and read_previous:
            content = read_previous(base_name(name))
            if content is None:
                errors.append(f"{name}: exception has no source in base revision")
            else:
                limit = min(limit, max(budget.limit, line_count(content)))
        if count > limit:
            errors.append(f"{name}: {count} lines > {limit}; split by responsibility")
        elif name in budget.exceptions:
            if count <= budget.limit:
                errors.append(f"{name}: remove the exception; now {count} lines")
            elif count < budget.exceptions[name]:
                notices.append(f"{name}: tighten exception to {count}")
    for name in sorted(budget.exceptions.keys() - counts.keys()):
        errors.append(f"{name}: remove missing-source exception")
    return errors, notices, counts
