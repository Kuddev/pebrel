"""Check file-configured key names using a real Windows key and window state."""
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


def focus_window(ctx, app_pid):
    user = ctypes.WinDLL("user32", use_last_error=True)
    hwnd_type = wintypes.HWND
    enum_proc = ctypes.WINFUNCTYPE(wintypes.BOOL, hwnd_type, wintypes.LPARAM)
    user.EnumWindows.argtypes = [enum_proc, wintypes.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [hwnd_type, ctypes.POINTER(wintypes.DWORD)]
    user.IsWindowVisible.argtypes = user.SetForegroundWindow.argtypes = [hwnd_type]
    user.GetForegroundWindow.restype = hwnd_type
    user.AllowSetForegroundWindow.argtypes = [wintypes.DWORD]

    def owner(hwnd):
        pid = wintypes.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        return pid.value

    windows = []
    @enum_proc
    def collect(hwnd, _):
        if owner(hwnd) == app_pid and user.IsWindowVisible(hwnd):
            windows.append(hwnd)
        return True

    user.EnumWindows(collect, 0)
    if len(windows) != 1:
        raise RuntimeError(f"Expected one visible Pebrel window, found {len(windows)}")
    user.AllowSetForegroundWindow(app_pid)
    ctx.api("window.focus", {"window_id": ctx.window_id, "pane_id": ctx.pane_id})
    for _ in range(15):
        if owner(user.GetForegroundWindow()) == app_pid:
            return windows[0]
        user.SetForegroundWindow(windows[0])
        time.sleep(0.1)
    raise RuntimeError(f"Could not focus Pebrel on hosted desktop; foreground PID "
                       f"{owner(user.GetForegroundWindow())}, expected {app_pid}")


def run(executable, root, combo):
    app = ResolvedApp(executable)
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "project",
                             root / "evidence", startup_timeout=45)
    ctx.prepare()
    with (ctx.config_dir / "pebrel_settings.txt").open("a", encoding="utf-8") as config:
        config.write(f"keybind={combo}:Minimize\n")
    try:
        ctx.start()
        ctx.refresh_targets()
        hwnd = focus_window(ctx, ctx.snapshot()["process_id"])
        user = ctypes.WinDLL("user32", use_last_error=True)
        user.IsIconic.argtypes = [wintypes.HWND]
        if user.IsIconic(hwnd):
            raise RuntimeError("Fixture window is already minimized")
        user.keybd_event.argtypes = [wintypes.BYTE, wintypes.BYTE, wintypes.DWORD, ctypes.c_size_t]
        keys = [0x11, 0x10, ord("M")] if combo == "ctrl+shift+m" else [0x1B]
        for key in keys:
            user.keybd_event(key, 0, 0, 0)
        for key in reversed(keys):
            user.keybd_event(key, 0, 2, 0)
        until = time.monotonic() + 4
        while time.monotonic() < until:
            if user.IsIconic(hwnd):
                return True
            time.sleep(0.05)
        return False
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated Windows runner")
    with tempfile.TemporaryDirectory(prefix="keybind-995-", ignore_cleanup_errors=True) as directory:
        results = {}
        for combo in ("ctrl+shift+m", "escape", "esc"):
            results[combo] = run(sys.argv[1], Path(directory) / combo, combo)
            print(combo, "minimized:", results[combo], flush=True)
        assert results == {"ctrl+shift+m": True, "escape": True, "esc": sys.argv[2] == "candidate"}, results
