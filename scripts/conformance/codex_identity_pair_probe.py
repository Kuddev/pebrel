"""Compare two real Codex TUIs with their native history on an isolated Windows desktop."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import re
import threading
import time
from unittest.mock import patch

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


class LocalModel(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_error(404)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        message = {"type": "message", "role": "assistant",
                   "content": [{"type": "output_text", "text": "Fixture reply."}]}
        events = [
            {"type": "response.created", "response": {"id": "resp_qa"}},
            {"type": "response.output_item.done", "item": {"id": "msg_qa", **message}},
            {"type": "response.completed", "response": {"id": "resp_qa", "usage": {
                "input_tokens": 0, "output_tokens": 0, "total_tokens": 0}}},
        ]
        body = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


def verify_standalone(executable, root):
    app = ResolvedApp(executable)
    ctx = ConformanceContext(app, "windows", root / "config", root / "same-cwd", root / "out")
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    config_text = settings.read_text(encoding="utf-8")
    assert "resume_ai=0" in config_text
    settings.write_text(config_text.replace("resume_ai=0", "resume_ai=1"), encoding="utf-8")
    home = root / "provider"
    home.mkdir()
    # A newly started Windows shell rebuilds PATH from the user's environment.
    # Give both launches the same fixture-local Codex installation and profile.
    codex = Path(os.environ["PEBREL_QA_CODEX_BIN"]).resolve(strict=True)
    (ctx.work_dir / "codex.cmd").write_text(
        f'@echo off\nsetlocal\nset "CODEX_HOME={home}"\n"{codex}" --no-daemon %*\nexit /b %errorlevel%\n',
        encoding="utf-8", newline="\r\n",
    )
    server = ThreadingHTTPServer(("127.0.0.1", 0), LocalModel)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = (
        'model="gpt-5.4"\nmodel_provider="fixture"\ncheck_for_update_on_startup=false\n'
        '[tui]\nstatus_line=["session-id"]\n'
        '[model_providers.fixture]\nname="fixture"\nwire_api="responses"\n'
        'requires_openai_auth=false\nsupports_websockets=false\n'
        f'base_url="http://127.0.0.1:{server.server_port}/v1"\n'
        f'[projects.{json.dumps(str(ctx.work_dir))}]\ntrust_level="trusted"\n'
        '[windows]\nsandbox="unelevated"\n'
    )
    (home / "config.toml").write_text(config, encoding="utf-8")
    (home / "auth.json").write_text('{"OPENAI_API_KEY":"fixture"}', encoding="utf-8")
    names = ["qa-alpha", "qa-bravo"]
    shell = {"kind": "shell", "name": "Command Prompt", "program": os.environ["ComSpec"], "args": ["/D"]}
    tabs = [{"cwd": str(ctx.work_dir), "custom_name": name, "active_pane": 0,
             "launch": shell} for name in names]
    ctx.session_file.write_text(json.dumps({"version": 4, "active_tab": 0, "tabs": tabs,
                                            "clean_exit": True}), encoding="utf-8")
    try:
        with patch.dict(os.environ, CODEX_HOME=str(home), OPENAI_API_KEY="fixture"):
            ctx.start(explicit_working_directory=False)
            ctx.refresh_targets()
            panes = {t["label"]: t["panes"][0]["id"] for t in ctx.snapshot()["windows"][0]["tabs"]}
            assert set(panes) == set(names)
            def native_history():
                try:
                    records = (home / "history.jsonl").read_text(encoding="utf-8").splitlines()
                    return {r["text"]: r["session_id"] for r in map(json.loads, records)}
                except (FileNotFoundError, PermissionError, json.JSONDecodeError):
                    return {}

            for name, pane in panes.items():
                ctx.poll(lambda: ctx.read(pane)["text"].strip(), bool, "missing command prompt")
                ctx.prompt(f"codex {name}", pane)
                ctx.poll(native_history, lambda rows: name in rows,
                         "Codex did not record the test prompt in a native thread", timeout=35)
            history = native_history()
            expected = {name: history[name] for name in names}
            assert len(set(expected.values())) == len(names)
            for name, pane in panes.items():
                other = next(i for n, i in expected.items() if n != name)
                row = re.compile(r"(?m)^\s*" + re.escape(expected[name]) + r"\s*$")
                screen = ctx.poll(lambda: ctx.read(pane)["text"], row.search,
                                  "native thread did not appear in its own status line", timeout=30)
                assert other not in screen
            print("Both Codex status rows exactly matched their native threads.", flush=True)

            def saved_ids():
                try:
                    snap = json.loads(ctx.session_file.read_text(encoding="utf-8"))
                except (PermissionError, FileNotFoundError) as error:
                    return {"snapshot_read_error": str(error)}
                return {t["custom_name"]: (t.get("layout", {}).get("agent") or {}).get("session_id")
                        for t in snap["tabs"]}

            ctx.poll(saved_ids, lambda ids: ids == expected,
                     "the Windows snapshot did not identify both standalone threads", timeout=65)
            print("Both Windows snapshot IDs matched their native Codex threads.", flush=True)

            def tab_and_pane():
                return {tab["label"]: (tab["tab_id"], tab["panes"][0]["id"])
                        for tab in ctx.snapshot()["windows"][0]["tabs"]}

            initial = tab_and_pane()
            active = panes["qa-alpha"]
            first = expected["qa-alpha"]
            ctx.prompt("/new", active)
            thread_pattern = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", re.I)

            def footer():
                lines = [line.strip() for line in ctx.read(active)["text"].splitlines() if line.strip()]
                return next((line for line in reversed(lines[-4:]) if thread_pattern.fullmatch(line)), None)

            ctx.poll(footer, lambda value: value and value != first,
                     "Codex did not display a fresh thread after /new", timeout=30)
            ctx.prompt("qa-after-new", active)
            second = ctx.poll(native_history, lambda rows: "qa-after-new" in rows,
                              "no native message after /new", timeout=35)["qa-after-new"]
            assert second != first and footer() == second
            assert tab_and_pane() == initial
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                observed = saved_ids()
                if observed.get("qa-alpha") == second and observed.get("qa-bravo") == expected["qa-bravo"]:
                    break
                time.sleep(0.3)
            assert observed.get("qa-bravo") == expected["qa-bravo"]
            target = observed.get("qa-alpha")
            outcome = "current" if target == second else ("previous" if target == first else "missing-or-other")
            print("Windows /new: tab and pane IDs unchanged, native thread changed, persisted target=" + outcome,
                  flush=True)
            assert outcome == "current", "did not checkpoint the current native Codex thread"

            confirm_native_window_close(ctx)
            assert saved_ids() == {"qa-alpha": second, "qa-bravo": expected["qa-bravo"]}
            ctx.start(explicit_working_directory=False)
            ctx.refresh_targets()
            restored = tab_and_pane()
            assert set(restored) == set(expected)
            for name, thread_id in [("qa-alpha", second), ("qa-bravo", expected["qa-bravo"])]:
                _, pane = restored[name]
                ctx.poll(lambda: ctx.read(pane)["text"], lambda text: "Fixture reply." in text,
                         "cold launch did not restore the synthetic transcript", timeout=45)
                marker = f"qa-after-normal-restart-{name}"
                ctx.prompt(marker, pane)
                rows = ctx.poll(native_history, lambda history: marker in history,
                                "Codex did not record the post-restart input", timeout=35)
                assert rows[marker] == thread_id, f"{name} resumed the wrong conversation"
            print("Normal restart: both new messages reached their respective original threads.",
                  flush=True)
    finally:
        ctx.stop(force=True)
        app.close()
        server.shutdown()
        server.server_close()


def confirm_native_window_close(ctx):
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
    if not user.SendMessageTimeoutW(hwnd, 0x0010, 0, 0, 0x0002, 5000, ctypes.byref(result)):
        raise ctypes.WinError(ctypes.get_last_error())
    # The GPUI busy-window dialog uses a wide red confirmation button to the
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

    x, y = ctx.poll(settled_button, bool, "busy-window Close button did not render", timeout=15)
    location = y << 16 | x
    for message, flags in [(0x0200, 0), (0x0201, 1), (0x0202, 0)]:
        if not user.SendMessageTimeoutW(hwnd, message, flags, location, 0x0002, 5000, ctypes.byref(result)):
            raise ctypes.WinError(ctypes.get_last_error())
    print("Clicked the rendered busy-window Close button.", flush=True)
    if ctx.process.wait(timeout=20) != 0:
        raise RuntimeError("The window closed with an unsuccessful process exit")
    if not json.loads(ctx.session_file.read_text(encoding="utf-8")).get("clean_exit"):
        raise RuntimeError("Window exit did not persist a clean session")
    ctx.stop(force=True)


def isolated_desktop():
    # The ARM image can start with a full-screen Windows setup page. Run only
    # the fixture on a fresh desktop and return to the previous desktop on exit.
    import subprocess
    from conformance.windows_standard_user import StartupInfo, ProcessInfo
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    user = ctypes.WinDLL("user32", use_last_error=True)
    user.CreateDesktopW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.LPVOID,
                                    wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID]
    user.CreateDesktopW.restype = wintypes.HANDLE
    user.OpenInputDesktop.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    user.OpenInputDesktop.restype = wintypes.HANDLE
    user.SwitchDesktop.argtypes = user.CloseDesktop.argtypes = [wintypes.HANDLE]
    name = "PebrelRecovery" + str(os.getpid())
    previous = user.OpenInputDesktop(0, False, 0x100)
    if not previous:
        raise ctypes.WinError(ctypes.get_last_error())
    desktop = user.CreateDesktopW(name, None, None, 0, 0x1FF, None)
    if not desktop:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        if not user.SwitchDesktop(desktop):
            raise ctypes.WinError(ctypes.get_last_error())
        startup = StartupInfo()
        startup.cb = ctypes.sizeof(startup)
        startup.lpDesktop = "winsta0\\" + name
        startup.dwFlags = 0x100
        kernel.GetStdHandle.restype = wintypes.HANDLE
        startup.hStdInput, startup.hStdOutput, startup.hStdError = [kernel.GetStdHandle(i) for i in (-10,-11,-12)]
        info = ProcessInfo()
        args = [sys.executable, __file__, sys.argv[1], "--desktop-ready"]
        command = ctypes.create_unicode_buffer(subprocess.list2cmdline(args))
        if not kernel.CreateProcessW(sys.executable, command, None, None, True, 0, None, None,
                                     ctypes.byref(startup), ctypes.byref(info)):
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            if kernel.WaitForSingleObject(wintypes.HANDLE(info.hProcess), 240000) != 0:
                raise RuntimeError("isolated desktop fixture did not exit")
            code = wintypes.DWORD()
            if not kernel.GetExitCodeProcess(wintypes.HANDLE(info.hProcess), ctypes.byref(code)):
                raise ctypes.WinError(ctypes.get_last_error())
            return code.value
        finally:
            kernel.CloseHandle(wintypes.HANDLE(info.hThread))
            kernel.CloseHandle(wintypes.HANDLE(info.hProcess))
    finally:
        user.SwitchDesktop(previous)
        user.CloseDesktop(desktop)
        user.CloseDesktop(previous)


if __name__ == "__main__":
    from tempfile import TemporaryDirectory
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated hosted Windows runner")
    if "--desktop-ready" not in sys.argv and os.environ.get("RUNNER_ARCH") == "ARM64":
        raise SystemExit(isolated_desktop())
    ctypes.WinDLL("user32").SetProcessDPIAware()
    with TemporaryDirectory(prefix="pebrel-checkpoint-", ignore_cleanup_errors=True) as directory:
        verify_standalone(sys.argv[1], Path(directory))
