"""Compare the released and candidate tray icons on an isolated Windows runner."""

import argparse
import ctypes
from ctypes import wintypes as wt
import json
import os
from pathlib import Path
import subprocess
import sys
from contextlib import contextmanager
from types import SimpleNamespace
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


from windows_standard_user import ProcessInfo, StartupInfo, WindowsTokens

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import _WindowsJob


@contextmanager
def ordinary_product(app, profile):
    api = WindowsTokens()
    source = api.current_token(0x008B)
    token = None
    job = _WindowsJob()
    process = ProcessInfo()
    try:
        elevated = api.elevated(source)
        token = api.restrict(source) if elevated else source
        args = [str(app), "--gpui", "--working-directory", str(profile)]
        command = ctypes.create_unicode_buffer(subprocess.list2cmdline(args))
        startup = StartupInfo()
        startup.cb = ctypes.sizeof(startup)
        env = dict(os.environ, PEBREL_CONFIG_DIR=str(profile), NEBULA_CONFIG_DIR=str(profile))
        environment = ctypes.create_unicode_buffer(
            "\0".join(f"{k}={v}" for k, v in sorted(env.items(), key=lambda item: item[0].casefold())) + "\0"
        )
        api.require(api.security.CreateProcessAsUserW(
            token, str(app), command, None, None, False, 0x00000404,
            ctypes.cast(environment, wt.LPVOID), str(app.parent),
            ctypes.byref(startup), ctypes.byref(process),
        ))
        try:
            job.assign(SimpleNamespace(_handle=process.hProcess))
            resume = api.kernel.ResumeThread
            resume.argtypes, resume.restype = [wt.HANDLE], wt.DWORD
            if resume(process.hThread) == 0xFFFFFFFF:
                raise ctypes.WinError(ctypes.get_last_error())
            yield process.dwProcessId, lambda: api.kernel.WaitForSingleObject(process.hProcess, 0) == 0x102
        finally:
            terminate = api.kernel.TerminateProcess
            terminate.argtypes, terminate.restype = [wt.HANDLE, wt.UINT], wt.BOOL
            terminate(process.hProcess, 1)
            job.terminate()
    finally:
        job.close()
        if process.hThread:
            api.kernel.CloseHandle(process.hThread)
        if process.hProcess:
            api.kernel.CloseHandle(process.hProcess)
        if token and token != source:
            api.kernel.CloseHandle(token)
        api.kernel.CloseHandle(source)


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


def run_variant(desktop, app, root, expect_recovery):
    config = root / ("candidate" if expect_recovery else "baseline")
    config.mkdir()
    (config / "pebrel_settings.txt").write_text("tray=1\n", encoding="utf-8")
    with ordinary_product(app.resolve(strict=True), config) as (pid, alive):
        hwnd = wait_for("Pebrel's tray window", lambda: desktop.tray_window(pid))
        assert not is_elevated(pid), "Pebrel is unexpectedly elevated"
        before = wait_for("Pebrel's initial tray icon", lambda: desktop.icon_rect(hwnd))
        print(f"Initial icon {before} for ordinary Pebrel PID {pid}", flush=True)
        old_shell, new_shell = desktop.restart_explorer()
        elevated_shell = is_elevated(new_shell)
        print(f"Explorer restarted from PID {old_shell} to {new_shell}; elevated={elevated_shell}",
              flush=True)
        after = None
        if expect_recovery:
            after = wait_for("the candidate's restored tray icon", lambda: fresh_icon_rect(hwnd))
            desktop.verify_tray_menu(hwnd)
        else:
            end = time.monotonic() + 5
            while time.monotonic() < end:
                if fresh_icon_rect(hwnd):
                    raise AssertionError("the released build already restores its tray icon")
                time.sleep(0.1)
        if not alive() or desktop.tray_window(pid) != hwnd:
            raise AssertionError("the original Pebrel process or tray window exited")
        return {"app_pid": pid, "app_elevated": False, "old_explorer": old_shell,
                "new_explorer": new_shell, "explorer_elevated": elevated_shell,
                "icon_before": before, "icon_after": after, "menu_opens": expect_recovery}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--variant", choices=("baseline", "candidate"), required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        parser.error("requires a dedicated GitHub-hosted Windows desktop")
    with tempfile.TemporaryDirectory(prefix="pebrel-tray-restart-", ignore_cleanup_errors=True) as temporary:
        desktop, root = Desktop(), Path(temporary)
        candidate = args.variant == "candidate"
        app = args.candidate if candidate else args.baseline
        print(f"Starting {args.variant} with Explorer PID {desktop.taskbar()}", flush=True)
        report = {args.variant: run_variant(desktop, app, root, candidate)}
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    if (len(sys.argv) == 3 and sys.argv[1] == "--query-icon"
            and os.name == "nt" and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"):
        print(json.dumps(Desktop().icon_rect(int(sys.argv[2]))))
    else:
        main()
