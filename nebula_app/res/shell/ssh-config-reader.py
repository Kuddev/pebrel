"""Bounded filesystem requests only; SSH syntax and Include policy stay in Rust."""
import errno
import json
import os
from pathlib import Path
import signal
import stat
import sys


def expired(_signal, _frame):
    raise TimeoutError("deadline")


def inspect(operation, path, limit):
    if operation == "identity":
        import pwd

        account = pwd.getpwuid(os.geteuid())
        return {"kind": "identity", "user": account.pw_name, "home": account.pw_dir}
    if operation == "canonical":
        target = Path(path).resolve(strict=True)
        return {"kind": "path", "path": str(target) if target.is_file() else None}
    if operation == "read":
        # stat 与 open 之间可能被替换成 FIFO；以非阻塞 fd 核对真实打开的对象。
        descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
        with os.fdopen(descriptor, "rb") as source:
            if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
                return {"kind": "text", "text": None}
            data = source.read(limit + 1)
        if len(data) > limit:
            return {"kind": "limit"}
        return {"kind": "text", "text": data.decode("utf-8")}
    if operation == "list":
        names = []
        with os.scandir(path) as entries:
            for entry in entries:
                if len(names) == limit:
                    return {"kind": "limit"}
                entry.name.encode("utf-8", "strict")
                names.append(entry.name)
        return {"kind": "entries", "names": names}
    raise ValueError("operation")


operation = sys.argv[1]
try:
    limit = int(sys.argv[3])
    milliseconds = int(sys.argv[4])
    if not 0 <= limit <= 1024 * 1024 or not 0 < milliseconds <= 10000:
        raise ValueError("budget")
    signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, milliseconds / 1000)
    reply = inspect(operation, sys.argv[2], limit)
except (FileNotFoundError, NotADirectoryError, IsADirectoryError):
    reply = {
        "canonical": {"kind": "path", "path": None},
        "read": {"kind": "text", "text": None},
        "list": {"kind": "entries", "names": []},
    }.get(operation, {"kind": "error", "code": "identity"})
except TimeoutError:
    reply = {"kind": "error", "code": "timeout"}
except UnicodeError:
    reply = {"kind": "error", "code": "encoding"}
except PermissionError:
    reply = {"kind": "error", "code": "permission"}
except (OSError, ValueError, KeyError) as error:
    reply = {"kind": "error", "code": str(getattr(error, "errno", None) or errno.EINVAL)}

# ASCII JSON preserves UTF-8 source text without relying on the Windows console code page.
sys.stdout.buffer.write(json.dumps(reply, ensure_ascii=True, separators=(",", ":")).encode("ascii"))
