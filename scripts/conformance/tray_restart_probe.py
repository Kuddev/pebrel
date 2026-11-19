"""Compare the released and candidate tray icons on an isolated Windows runner."""

import ctypes
from ctypes import wintypes as wt
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


class NotifyIconId(ctypes.Structure):
    _fields_ = [
        ("size", wt.DWORD), ("window", wt.HWND), ("icon", wt.UINT),
        ("guid", ctypes.c_uint32 * 4),
    ]


class GuiThreadInfo(ctypes.Structure):
    _fields_ = [
        ("size", wt.DWORD), ("flags", wt.DWORD), ("active", wt.HWND),
        ("focus", wt.HWND), ("capture", wt.HWND), ("menu_owner", wt.HWND),
        ("move_size", wt.HWND), ("caret", wt.HWND), ("caret_rect", wt.RECT),
    ]


from windows_standard_user import WindowsTokens

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


def is_elevated(pid):
    api = WindowsTokens()
    open_process = api.kernel.OpenProcess
    open_process.argtypes, open_process.restype = [wt.DWORD, wt.BOOL, wt.DWORD], wt.HANDLE
    process = open_process(0x1000, False, pid)
    if not process:
        raise ctypes.WinError(ctypes.get_last_error())
    token = wt.HANDLE()
    try:
        api.require(api.security.OpenProcessToken(process, 0x0008, ctypes.byref(token)))
        return api.elevated(token)
    finally:
        if token:
            api.kernel.CloseHandle(token)
        api.kernel.CloseHandle(process)


def fresh_icon_rect(hwnd):
    return json.loads(subprocess.check_output(
        [sys.executable, __file__, "--query-icon", str(hwnd)], timeout=5, text=True,
    ))


def wait_for(description, predicate, timeout=20):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        result = predicate()
        if result:
            return result
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {description}")


class Desktop:
    def __init__(self):
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        self.shell = ctypes.WinDLL("shell32", use_last_error=True)
        self.enum_callback = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
        for library, name, result, args in (
            (self.user, "FindWindowW", wt.HWND, [wt.LPCWSTR, wt.LPCWSTR]),
            (self.user, "EnumWindows", wt.BOOL, [self.enum_callback, wt.LPARAM]),
            (self.user, "GetWindowThreadProcessId", wt.DWORD, [wt.HWND, ctypes.POINTER(wt.DWORD)]),
            (self.user, "GetClassNameW", ctypes.c_int, [wt.HWND, wt.LPWSTR, ctypes.c_int]),
            (self.user, "PostMessageW", wt.BOOL, [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]),
            (self.user, "GetGUIThreadInfo", wt.BOOL, [wt.DWORD, ctypes.POINTER(GuiThreadInfo)]),
            (self.shell, "Shell_NotifyIconGetRect", ctypes.c_long, [ctypes.POINTER(NotifyIconId), ctypes.POINTER(wt.RECT)]),
        ):
            api = getattr(library, name)
            api.restype, api.argtypes = result, args

    def owner(self, hwnd):
        pid = wt.DWORD()
        thread = self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        return thread, pid.value

    def taskbar(self):
        hwnd = self.user.FindWindowW("Shell_TrayWnd", None)
        if hwnd:
            return self.owner(hwnd)[1]
        return None

    def tray_window(self, pid):
        windows = []

        @self.enum_callback
        def inspect(hwnd, _):
            name = ctypes.create_unicode_buffer(80)
            if self.owner(hwnd)[1] == pid:
                self.user.GetClassNameW(hwnd, name, len(name))
                if name.value == "NebulaTrayWindow":
                    windows.append(hwnd)
            return True

        self.user.EnumWindows(inspect, 0)
        return windows[0] if windows else None

    def icon_rect(self, hwnd):
        icon = NotifyIconId(size=ctypes.sizeof(NotifyIconId), window=hwnd, icon=1)
        rect = wt.RECT()
        if self.shell.Shell_NotifyIconGetRect(ctypes.byref(icon), ctypes.byref(rect)) == 0:
            if rect.right > rect.left and rect.bottom > rect.top:
                return [rect.left, rect.top, rect.right, rect.bottom]
        return None

    def restart_explorer(self):
        old = wait_for("the existing Explorer taskbar", self.taskbar)
        subprocess.run(
            ["taskkill.exe", "/F", "/PID", str(old)], timeout=20, check=True, capture_output=True,
        )
        try:
            wait_for("the old taskbar to exit", lambda: self.taskbar() != old)
            if self.taskbar() is None:
                subprocess.Popen([str(Path(os.environ["WINDIR"]) / "explorer.exe")])
            new = wait_for("the replacement Explorer taskbar", self.taskbar, 35)
            if new == old:
                raise RuntimeError("Explorer taskbar owner did not change")
            return old, new
        finally:
            if self.taskbar() is None:
                subprocess.Popen([str(Path(os.environ["WINDIR"]) / "explorer.exe")])

    def verify_tray_menu(self, hwnd):
        thread, _ = self.owner(hwnd)

        def menu_active():
            info = GuiThreadInfo(size=ctypes.sizeof(GuiThreadInfo))
            if self.user.GetGUIThreadInfo(thread, ctypes.byref(info)):
                return info.menu_owner == hwnd and bool(info.flags & 0x04)
            return False

        # Deliver the shell's right-click callback to the verified tray window.
        if not self.user.PostMessageW(hwnd, 0x8001, 1, 0x0205):
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            wait_for("the tray context menu", menu_active, 5)
        finally:
            self.user.PostMessageW(hwnd, 0x001F, 0, 0)  # WM_CANCELMODE


def worker(executable, root, candidate):
    desktop = Desktop()
    app = ResolvedApp(executable)
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "project",
                             root / "evidence", startup_timeout=45)
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    settings.write_text(settings.read_text().replace("tray=0", "tray=1"), encoding="utf-8")
    try:
        ctx.start()
        pid = ctx.snapshot()["process_id"]
        if is_elevated(pid):
            raise RuntimeError("The test product must run as an ordinary user")
        hwnd = wait_for("the Runtime process's tray HWND", lambda: desktop.tray_window(pid), 30)
        before = wait_for("the initial registered icon", lambda: fresh_icon_rect(hwnd), 45)
        print("Initial registered icon:", before, flush=True)
        (root / "ready").touch()
        wait_for("a new Explorer taskbar", lambda: (root / "restarted").exists(), 75)
        if candidate:
            after = wait_for("the restored icon", lambda: fresh_icon_rect(hwnd), 45)
            desktop.verify_tray_menu(hwnd)
            print("Restored icon and opened the same process's tray menu:", after, flush=True)
        else:
            until = time.monotonic() + 15
            while time.monotonic() < until:
                if fresh_icon_rect(hwnd):
                    raise RuntimeError("The published baseline recovered its icon")
                time.sleep(0.3)
            print("The published baseline lost its previously registered icon.", flush=True)
    finally:
        ctx.stop(force=True)
        app.close()


def supervise(executable, variant):
    desktop = Desktop()
    with tempfile.TemporaryDirectory(prefix="tray-980-", ignore_cleanup_errors=True) as temp:
        root = Path(temp)
        os.environ["PEBREL_QA_TRAY_TRACE"] = str(root / "tray-events")
        args = [sys.executable, str(Path(__file__).with_name("windows_standard_user.py")),
                __file__, "--worker", executable, temp, variant]
        worker_proc = subprocess.Popen(args)
        try:
            def ready():
                if worker_proc.poll() is not None:
                    raise RuntimeError(f"Ordinary-user fixture exited early: {worker_proc.returncode}")
                return (root / "ready").is_file()
            wait_for("the ordinary user's initial icon", ready, 140)
            old, new = desktop.restart_explorer()
            print("Replaced Explorer taskbar owner:", old, new, "elevated:", is_elevated(new), flush=True)
            (root / "restarted").touch()
            if worker_proc.wait(timeout=90):
                raise RuntimeError("Tray recovery verification failed")
        finally:
            path = root / "tray-events"
            if path.is_file():
                print("Registration events:", path.read_text(errors="replace")[:3000])
            if worker_proc.poll() is None:
                worker_proc.terminate()
                worker_proc.wait(timeout=10)


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated GitHub-hosted Windows desktop")
    if sys.argv[1] == "--query-icon":
        print(json.dumps(Desktop().icon_rect(int(sys.argv[2]))))
    elif sys.argv[1] == "--worker":
        worker(sys.argv[2], Path(sys.argv[3]), sys.argv[4] == "candidate")
    else:
        supervise(sys.argv[1], sys.argv[2])
