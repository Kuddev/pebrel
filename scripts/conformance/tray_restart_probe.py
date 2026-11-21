"""Compare the released and candidate tray icons on an isolated Windows runner."""

import ctypes
from ctypes import wintypes as wt
import json
import os
import platform
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import uuid


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


def registered(desktop, hwnd):
    reply = ctypes.c_size_t()
    received = desktop.user.SendMessageTimeoutW(hwnd, 0x804E, 0, 0, 2, 3000, ctypes.byref(reply))
    if not received or not reply.value & 8:
        raise RuntimeError(f"No reply from the tested tray thread: {received}, {reply.value}")
    diagnostic = (bool(reply.value & 4), bool(reply.value & 2), bool(reply.value & 1))
    if getattr(desktop, "last_probe", None) != diagnostic:
        print("Tray enabled / app added / Explorer recognizes the icon:", diagnostic, flush=True)
        desktop.last_probe = diagnostic
    return diagnostic[-1]



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
            (self.user, "FindWindowExW", wt.HWND, [wt.HWND, wt.HWND, wt.LPCWSTR, wt.LPCWSTR]),
            (self.user, "EnumWindows", wt.BOOL, [self.enum_callback, wt.LPARAM]),
            (self.user, "GetWindowThreadProcessId", wt.DWORD, [wt.HWND, ctypes.POINTER(wt.DWORD)]),
            (self.user, "GetClassNameW", ctypes.c_int, [wt.HWND, wt.LPWSTR, ctypes.c_int]),
            (self.user, "SendMessageTimeoutW", ctypes.c_ssize_t, [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM, wt.UINT, wt.UINT, ctypes.POINTER(ctypes.c_size_t)]),
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

    def icon_rect(self, hwnd, diagnose=False):
        icon = NotifyIconId(size=ctypes.sizeof(NotifyIconId), window=hwnd, icon=1)
        rect = wt.RECT()
        status = self.shell.Shell_NotifyIconGetRect(ctypes.byref(icon), ctypes.byref(rect))
        if diagnose:
            print(f"Shell_NotifyIconGetRect HRESULT 0x{status & 0xffffffff:08x}, "
                  f"rect=({rect.left}, {rect.top}, {rect.right}, {rect.bottom})", flush=True)
        if status == 0:
            if rect.right > rect.left and rect.bottom > rect.top:
                return [rect.left, rect.top, rect.right, rect.bottom]
        return None

    def stop_explorer(self, pid):
        subprocess.run(
            ["taskkill.exe", "/F", "/PID", str(pid), "/FI", "IMAGENAME eq explorer.exe"],
            timeout=20, check=True, capture_output=True,
        )
        wait_for("the old taskbar to exit", lambda: self.taskbar() != pid)

    def ordinary_notification_area(self):
        if not self.taskbar():
            subprocess.Popen([str(Path(os.environ["WINDIR"]) / "explorer.exe")],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)

        def ready():
            bar = self.user.FindWindowW("Shell_TrayWnd", None)
            if bar and self.user.FindWindowExW(bar, None, "TrayNotifyWnd", None):
                return self.owner(bar)[1]
            return None

        pid = wait_for("Explorer's notification area on the worker desktop", ready, 45)
        if is_elevated(pid):
            raise RuntimeError(f"Explorer PID {pid} is elevated on the ordinary user's desktop")
        return pid

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
    root.mkdir()
    desktop = Desktop()
    original_shell = desktop.ordinary_notification_area()
    print("Initial notification-area PID:", original_shell, flush=True)
    app = ResolvedApp(executable)
    print("Windows build:", sys.getwindowsversion().build, "architecture:", platform.machine(), flush=True)
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
        try:
            before = wait_for("the initial registered icon", lambda: registered(desktop, hwnd), 45)
        except RuntimeError:
            desktop.icon_rect(hwnd, diagnose=True)
            raise
        print("Initial registered icon:", before, flush=True)
        desktop.stop_explorer(original_shell)
        replacement = desktop.ordinary_notification_area()
        if original_shell == replacement:
            raise RuntimeError("Explorer did not restart")
        print("Replacement notification-area PID:", replacement, flush=True)
        if candidate:
            after = wait_for("the restored icon", lambda: registered(desktop, hwnd), 45)
            desktop.verify_tray_menu(hwnd)
            print("Restored icon and opened the same process's tray menu:", after, flush=True)
        else:
            until = time.monotonic() + 15
            while time.monotonic() < until:
                if registered(desktop, hwnd):
                    raise RuntimeError("The published baseline recovered its icon")
                time.sleep(0.3)
            print("The published baseline lost its previously registered icon.", flush=True)
    finally:
        ctx.stop(force=True)
        app.close()


def supervise(executable, variant):
    desktop = Desktop()
    # The ordinary token creates its own directory and inherits its own ACL.
    root = Path(tempfile.gettempdir()) / ("tray-980-" + uuid.uuid4().hex)
    os.environ["PEBREL_QA_TRAY_TRACE"] = str(root / "tray-events")
    args = [sys.executable, str(Path(__file__).with_name("windows_standard_user.py")),
            __file__, "--worker", executable, str(root), variant]
    # A hosted desktop may start with an elevated Explorer. Its replacement
    # and Pebrel must share the ordinary user's desktop and integrity level.
    existing = desktop.taskbar()
    if existing and is_elevated(existing):
        desktop.stop_explorer(existing)
    try:
        subprocess.run(args, check=True, timeout=240)
    finally:
        path = root / "tray-events"
        if path.is_file():
            print("Registration events:", path.read_text(errors="replace")[:3000])
        if root.is_dir():
            shutil.rmtree(root, ignore_errors=True)



if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated GitHub-hosted Windows desktop")
    if sys.argv[1] == "--query-icon":
        print(json.dumps(Desktop().icon_rect(int(sys.argv[2]))))
    elif sys.argv[1] == "--worker":
        worker(sys.argv[2], Path(sys.argv[3]), sys.argv[4] == "candidate")
    else:
        supervise(sys.argv[1], sys.argv[2])
