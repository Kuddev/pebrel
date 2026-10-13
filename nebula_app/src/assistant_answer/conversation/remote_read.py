"""Bounded native transcript bytes over an already authenticated SSH channel."""
import base64
import json
import os
import stat


def stamp(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def capture(request):
    path = request["path"]
    if not isinstance(path, str) or not path.startswith("/") or len(path.encode("utf-8")) > 4096:
        raise ValueError("invalid_path")
    with open(path, "rb") as file:
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
        if stamp(before) != stamp(os.fstat(file.fileno())) or stamp(before) != stamp(os.stat(path)):
            return {"error": "conversation_changed"}
        return {"head": base64.b64encode(head).decode("ascii"),
                "bytes": base64.b64encode(data).decode("ascii"), "start": start, "end": end,
                "stamp": ":".join(map(str, stamp(before)))}


def main(request):
    try:
        result = capture(request)
    except (OSError, ValueError, KeyError, TypeError):
        result = {"error": "conversation_unavailable"}
    result["version"] = 1
    print("PEBREL_TRANSCRIPT=" + json.dumps(result, separators=(",", ":")))
