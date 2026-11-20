"""Check explicit Codex tab closure across a normal Windows process restart."""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ConformanceError, ResolvedApp


def close_codex_tab(ctx):
    user = ctypes.WinDLL("user32", use_last_error=True)
    callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user.EnumWindows.argtypes = [callback, wintypes.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.IsWindowVisible.argtypes = [wintypes.HWND]
    user.SendMessageTimeoutW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM,
                                         wintypes.LPARAM, wintypes.UINT, wintypes.UINT,
                                         ctypes.POINTER(ctypes.c_size_t)]
    user.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    pid = ctx.snapshot()["process_id"]
    windows = []

    @callback
    def collect(hwnd, _):
        owner = wintypes.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user.IsWindowVisible(hwnd):
            windows.append(hwnd)
        return True

    user.EnumWindows(collect, 0)
    if len(windows) != 1:
        raise RuntimeError(f"Expected one visible test window, found {len(windows)}")
    hwnd = windows[0]
    ctx.api("window.focus", {"window_id": ctx.window_id, "pane_id": ctx.pane_id})
    from PIL import ImageGrab
    rect = wintypes.RECT()
    origin = wintypes.POINT()
    user.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
    if not user.GetClientRect(hwnd, ctypes.byref(rect)) or not user.ClientToScreen(hwnd, ctypes.byref(origin)):
        raise ctypes.WinError(ctypes.get_last_error())
    box = (origin.x, origin.y, origin.x + rect.right, origin.y + rect.bottom)
    result = ctypes.c_size_t()
    user.GetForegroundWindow.restype = wintypes.HWND
    user.SetForegroundWindow.argtypes = [wintypes.HWND]
    user.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    user.ShowWindow(hwnd, 9)
    user.keybd_event.argtypes = [wintypes.BYTE, wintypes.BYTE, wintypes.DWORD, ctypes.c_size_t]
    # Windows grants the last input recipient permission to activate another
    # window. Release Alt, focus the fixture, and verify keyboard ownership.
    user.keybd_event(0x12, 0, 0, 0)
    user.keybd_event(0x12, 0, 2, 0)
    user.SetForegroundWindow(hwnd)
    ctx.poll(user.GetForegroundWindow, lambda target: target == hwnd,
             "the fixture window did not acquire keyboard focus", timeout=5)
    time.sleep(0.3)
    before = ImageGrab.grab(bbox=box)
    for key in (0x11, 0x10, 0x57):
        user.keybd_event(key, 0, 0, 0)
    for key in (0x57, 0x10, 0x11):
        user.keybd_event(key, 0, 2, 0)
    # The GPUI busy-tab dialog uses a wide red confirmation button to the
    # right of Cancel. Wait for it to render and settle before clicking.
    def find_confirm():
        frame = ImageGrab.grab(bbox=box)
        width, height = frame.size
        rows = []
        for y in range(height // 2 - 12, min(height, height // 2 + 180), 2):
            hits = []
            for x in range(width // 2, min(width, width // 2 + 320), 3):
                color = frame.getpixel((x, y))[:3]
                r, g, b = color
                if r > g + 38 and r > b + 27 and color != before.getpixel((x, y))[:3]:
                    hits.append(x)
            if len(hits) >= 28 and hits[-1] - hits[0] < len(hits) * 4:
                rows.append(((hits[-1] + hits[0]) // 2, y))
        return rows[len(rows) // 2] if len(rows) >= 8 else None

    def settled_button():
        point = find_confirm()
        if point:
            time.sleep(0.22)
            if point == find_confirm():
                return point
        return None

    try:
        x, y = ctx.poll(settled_button, bool, "Close button is not visible", timeout=5)
    except ConformanceError:
        # Some hosted desktops do not expose the dialog pixels to ImageGrab.
        # Enter is the dialog's native confirmation binding. The caller checks
        # live tab membership before proceeding to the normal window close.
        if any(tab["label"] == "closed-codex" for window in ctx.snapshot()["windows"]
               for tab in window["tabs"]):
            user.keybd_event(0x0D, 0, 0, 0)
            user.keybd_event(0x0D, 0, 2, 0)
        return hwnd
    location = y << 16 | x
    for message, flags in [(0x0200, 0), (0x0201, 1), (0x0202, 0)]:
        if not user.SendMessageTimeoutW(hwnd, message, flags, location, 0x0002, 5000, ctypes.byref(result)):
            raise ctypes.WinError(ctypes.get_last_error())
    return hwnd


def run(executable, root):
    app = ResolvedApp(executable)
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "work",
                             root / "evidence", startup_timeout=50)
    ctx.prepare()
    with (ctx.config_dir / "pebrel_settings.txt").open("a", encoding="utf-8") as settings:
        settings.write("\nshell=cmd\nresume_ai=1\n")
    def stored_names():
        return [tab.get("custom_name") for tab in json.loads(ctx.session_file.read_text(encoding="utf-8"))["tabs"]]
    def live_names():
        return [tab["label"] for window in ctx.snapshot()["windows"] for tab in window["tabs"]]
    try:
        binary = Path(os.environ["PEBREL_QA_CODEX_BIN"]).resolve(strict=True)
        # Start the real CLI as a child of cmd using the released v4 schema.
        # This also exercises closing a tab opened by an earlier restore.
        cmd = {"kind": "shell", "name": "Command Prompt", "program": os.environ["ComSpec"]}
        tabs = [{"cwd": str(ctx.work_dir), "custom_name": name, "active_pane": 0,
                 "launch": {**cmd, "args": args}} for name, args in (
                     ("keep-this-shell", ["/D"]), ("closed-codex", ["/D", "/K", str(binary)]))]
        ctx.session_file.write_text(json.dumps({"version": 4, "boot_attempts": 0,
            "active_tab": 1, "tabs": tabs, "clean_exit": True}), encoding="utf-8")
        ctx.start(explicit_working_directory=False)
        ctx.refresh_targets()
        assert live_names() == ["keep-this-shell", "closed-codex"], live_names()
        target = {"window_id": ctx.window_id, "pane_id": ctx.pane_id}
        def processes():
            return [row.get("executable", "") for row in ctx.api("pane.procs", target)["processes"]]
        def running():
            rows = ctx.api("agents.list", {}).get("agents", [])
            return next((row["agent"] for row in rows if row["pane_id"] == ctx.pane_id
                         and row["agent"]["kind"] == "codex"), None)
        def codex_process():
            rows = ctx.api("pane.procs", {"window_id": ctx.window_id, "pane_id": ctx.pane_id})["processes"]
            return any("codex" in str(row.get("executable") or "").lower() for row in rows)
        try:
            ctx.poll(codex_process, bool, "The Windows pane did not run the released Codex executable", timeout=25)
        except Exception:
            print("Test processes:", processes(), "Test terminal:", ctx.read(lines=250)["text"][-900:])
            raise
        agent = running() or {}
        print("Codex process present. Agent recognized:", bool(agent),
              "Native identity:", bool(agent.get("session_id")), flush=True)
        ctx.poll(stored_names, lambda names: "closed-codex" in names,
                 "the original Codex tab was never checkpointed")
        hwnd = close_codex_tab(ctx)
        ctx.poll(live_names, lambda names: names == ["keep-this-shell"],
                 "tab confirmation did not remove the selected Codex tab")
        print("Stored names immediately after tab close:", stored_names(), flush=True)
        user = ctypes.WinDLL("user32", use_last_error=True)
        user.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
        if not user.PostMessageW(hwnd, 0x0010, 0, 0):
            raise ctypes.WinError(ctypes.get_last_error())
        assert ctx.process.wait(timeout=25) == 0
        assert json.loads(ctx.session_file.read_text(encoding="utf-8"))["clean_exit"]
        print("Stored names after normal program exit:", stored_names(), flush=True)
        assert stored_names() == ["keep-this-shell"]
        ctx.stop(force=True)
        ctx.start(explicit_working_directory=False)
        assert live_names() == ["keep-this-shell"], live_names()
        print("Fresh Windows process restored exactly the surviving tab.", flush=True)
    except Exception:
        from PIL import ImageGrab
        ImageGrab.grab().save(Path(os.environ["RUNNER_TEMP"]) / "codex-close-desktop.png")
        raise
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted" or os.name != "nt":
        raise SystemExit("Requires an isolated hosted Windows desktop")
    ctypes.WinDLL("user32").SetProcessDPIAware()
    with tempfile.TemporaryDirectory(prefix="pebrel-codex-close-", ignore_cleanup_errors=True) as test_dir:
        run(sys.argv[1], Path(test_dir))
