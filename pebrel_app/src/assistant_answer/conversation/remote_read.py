"""Bounded native transcript bytes over an already authenticated SSH channel."""
import base64
import json
import os
import stat


def stamp(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def resolve_antigravity(session_id):
    if (not isinstance(session_id, str) or not (1 <= len(session_id) <= 64)
            or session_id.startswith(".") or ".." in session_id
            or any(not (c.isascii() and (c.isalnum() or c in "-_.")) for c in session_id)):
        raise ValueError("invalid_session")
    current = os.path.expanduser("~")
    if not current.startswith("/"):
        raise ValueError("invalid_home")
    for seg in (".gemini", "antigravity-cli", "brain", session_id, ".system_generated", "logs"):
        current = os.path.join(current, seg)
        info = os.lstat(current)
        if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
            raise ValueError("unsafe_dir")
    full = os.path.join(current, "transcript_full.jsonl")
    try:
        full_info = os.lstat(full)
        if stat.S_ISLNK(full_info.st_mode):
            raise ValueError("unsafe_file")
        return full
    except FileNotFoundError:
        compact = os.path.join(current, "transcript.jsonl")
        compact_info = os.lstat(compact)
        if stat.S_ISLNK(compact_info.st_mode):
            raise ValueError("unsafe_file")
        return compact


def capture(request):
    antigravity_id = request.get("antigravity_id")
    path = resolve_antigravity(antigravity_id) if antigravity_id is not None else request["path"]
    if not isinstance(path, str) or not path.startswith("/") or len(path.encode("utf-8")) > 4096:
        raise ValueError("invalid_path")
    flags = os.O_RDONLY | (getattr(os, "O_NOFOLLOW", 0) if antigravity_id is not None else 0)
    with os.fdopen(os.open(path, flags), "rb") as file:
        before = os.fstat(file.fileno())
        if not stat.S_ISREG(before.st_mode):
            raise ValueError("invalid_file")
        end = request.get("before")
        if end is None:
            end = before.st_size
        if not isinstance(end, int) or end < 0 or end > before.st_size:
            return {"error": "conversation_changed"}
        start = max(0, end - 1024 * 1024)
        read_start = max(0, start - 1)
        head = file.read(256 * 1024)
        file.seek(read_start)
        data = file.read(end - read_start)
        # 同时核对打开的句柄和路径，替换/截短中的记录不伪装成稳定的一页。
        after_path = os.lstat(path) if antigravity_id is not None else os.stat(path)
        if stamp(before) != stamp(os.fstat(file.fileno())) or stamp(before) != stamp(after_path):
            return {"error": "conversation_changed"}
        if antigravity_id is not None and resolve_antigravity(antigravity_id) != path:
            return {"error": "conversation_changed"}
        return {"path": path, "head": base64.b64encode(head).decode("ascii"),
                "bytes": base64.b64encode(data).decode("ascii"), "start": start, "end": end,
                "stamp": ":".join(map(str, stamp(before)))}


def main(request):
    try:
        result = capture(request)
    except (OSError, ValueError, KeyError, TypeError):
        result = {"error": "conversation_unavailable"}
    result["version"] = 1
    print("PEBREL_TRANSCRIPT=" + json.dumps(result, separators=(",", ":")))
