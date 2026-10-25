"""Drain an owned POSIX launch group before releasing its leader's PID."""

from __future__ import annotations

import os
import signal
import subprocess
import time


def poll_exit(process: subprocess.Popen) -> int | None:
    # 保留已退出的组长，避免清理后代期间 PID/PGID 被复用而误伤新进程。
    if process.returncode is not None:
        raise RuntimeError("launch leader was reaped before its process group was drained")
    result = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
    if result is None:
        return None
    return result.si_status if result.si_code == os.CLD_EXITED else -result.si_status


def wait_exit(process: subprocess.Popen, timeout: float) -> int:
    deadline = time.monotonic() + timeout
    while True:
        code = poll_exit(process)
        if code is not None:
            return code
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise subprocess.TimeoutExpired(process.args, timeout)
        time.sleep(min(0.01, remaining))


def _live_group_members(pgid: int, timeout: float) -> list[int]:
    # macOS/Linux 的 ps 对 -g 含义不同；使用两者共有的列格式按 PGID 过滤。
    output = subprocess.run(
        ["ps", "-axo", "pid=,pgid=,stat="], check=True, capture_output=True,
        text=True, encoding="utf-8", timeout=timeout,
        env={**os.environ, "LC_ALL": "C"},
    ).stdout
    members = []
    for line in output.splitlines():
        pid, group, state = line.split()
        if int(group) == pgid and not state.startswith("Z"):
            members.append(int(pid))
    return members


def stop_group(process: subprocess.Popen, *, force: bool, timeout: float) -> None:
    poll_exit(process)
    graceful_deadline = time.monotonic() + timeout
    deadline = graceful_deadline if force else graceful_deadline + timeout

    def send(signum: int) -> None:
        try:
            os.killpg(process.pid, signum)
        except ProcessLookupError:
            pass
        except PermissionError:
            # Darwin 对只剩僵尸的组也返回 EPERM；须确认没有活进程，不能吞掉真实权限错误。
            if _live_group_members(process.pid, max(0, deadline - time.monotonic())):
                raise

    send(signal.SIGKILL if force else signal.SIGTERM)
    while True:
        code = poll_exit(process)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise subprocess.TimeoutExpired(process.args, timeout)
        # 组长退出不代表后代退出：组信号的成员快照可能漏掉正在创建的后代。
        if code is not None and not _live_group_members(process.pid, remaining):
            process.wait(timeout=max(0, deadline - time.monotonic()))
            return
        if code is not None or time.monotonic() >= graceful_deadline:
            send(signal.SIGKILL)
        time.sleep(min(0.02, max(0, deadline - time.monotonic())))
