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
from conformance.harness import ConformanceContext, ResolvedApp


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
    before = ImageGrab.grab(bbox=box)
    result = ctypes.c_size_t()
    user.SetForegroundWindow.argtypes = [wintypes.HWND]
    user.SetForegroundWindow(hwnd)
    user.keybd_event.argtypes = [wintypes.BYTE, wintypes.BYTE, wintypes.DWORD, ctypes.c_size_t]
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

    x, y = ctx.poll(settled_button, bool, "busy-tab Close button did not render", timeout=15)
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
        settings.write("\nshell=pwsh\nresume_ai=1\n")
    def stored_names():
        return [tab.get("custom_name") for tab in json.loads(ctx.session_file.read_text(encoding="utf-8"))["tabs"]]
    def live_names():
        return [tab["label"] for window in ctx.snapshot()["windows"] for tab in window["tabs"]]
    try:
        ctx.start()
        ctx.refresh_targets()
        ctx.api("tab.rename", {"window_id": ctx.window_id, "tab_index": 0, "name": "keep-this-shell"})
        ctx.api("tab.new", {"window_id": ctx.window_id})
        ctx.refresh_targets()
        ctx.api("tab.rename", {"window_id": ctx.window_id, "tab_index": 1, "name": "closed-codex"})
        ctx.prompt("Write-Output ('CLOSE-' + 'FIXTURE-READY')")
        ctx.poll(lambda: ctx.read(lines=20)["text"], lambda t: "CLOSE-FIXTURE-READY" in t,
                 "PowerShell did not start in the new tab", timeout=25)
        binary = Path(os.environ["PEBREL_QA_CODEX_BIN"]).resolve(strict=True)
        ctx.prompt("& '" + str(binary).replace("'", "''") + "'")
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
            print("Test terminal:", ctx.read(lines=15)["text"][-900:])
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
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted" or os.name != "nt":
        raise SystemExit("Requires an isolated hosted Windows desktop")
    with tempfile.TemporaryDirectory(prefix="pebrel-codex-close-", ignore_cleanup_errors=True) as test_dir:
        run(sys.argv[1], Path(test_dir))
