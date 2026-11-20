"""Exercise Codex exit and both PowerShell leaves across a native Windows close."""
import ctypes
from ctypes import wintypes
from http.server import ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import threading
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp
from codex_identity_pair_probe import LocalModel, configure_provider


def native_close(ctx):
    user = ctypes.WinDLL("user32", use_last_error=True)
    callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user.EnumWindows.argtypes = [callback, wintypes.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.IsWindowVisible.argtypes = [wintypes.HWND]
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

    if not user.EnumWindows(collect, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    assert len(windows) == 1, "expected one visible window owned by the test process"
    if not user.PostMessageW(windows[0], 0x0010, 0, 0):
        raise ctypes.WinError(ctypes.get_last_error())
    assert ctx.process.wait(timeout=25) == 0, "normal close failed"
    saved = json.loads(ctx.session_file.read_text(encoding="utf-8"))
    assert saved["clean_exit"] is True
    ctx.stop(force=True)


def split_leaves(ctx):
    tabs = ctx.snapshot()["windows"][0]["tabs"]
    assert len(tabs) == 1 and len(tabs[0]["panes"]) == 2
    layout = tabs[0]["layout"]
    assert layout["direction"] == "top_bottom"
    return [layout[key]["pane_id"] for key in ("first", "second")]


def shell_command(ctx, pane, command):
    ctx.prompt(command, pane, submit=False)
    # Native ConPTY can present the prompt before PSReadLine accepts Enter.
    ctx.poll(lambda: command[-14:] in "".join(ctx.read(pane)["text"].splitlines()),
             bool, "PowerShell did not display the test command", timeout=20)
    ctx.send_key("enter", pane)


def shell_reply(ctx, pane):
    ctx.poll(lambda: ctx.read(pane)["text"][-500:],
             lambda tail: bool(re.search(r"(?m)^\s*PS [^\r\n]*>\s*$", tail)),
             "the split did not display a ready PowerShell prompt", timeout=35)
    marker = uuid4().hex[:8]
    shell_command(ctx, pane, f"Write-Output ('{marker}-' + $PSVersionTable.PSVersion.Major)")
    output = re.compile(r"(?m)^\s*" + marker + r"-5\s*$")
    ctx.poll(lambda: ctx.read(pane)["text"][-800:], output.search,
             "Windows PowerShell did not execute the marker", timeout=18)


def saved_agents(ctx):
    try:
        tabs = json.loads(ctx.session_file.read_text(encoding="utf-8"))["tabs"]
        if len(tabs) != 1:
            return None
        layout = tabs[0]["layout"]
        return [layout[key].get("agent") for key in ("first", "second")]
    except (KeyError, OSError, ValueError):
        return None


def provider_id(home, marker):
    try:
        history = (home / "history.jsonl").read_text(encoding="utf-8")
        return next((r["session_id"] for r in map(json.loads, history.splitlines())
                     if r.get("text") == marker), None)
    except (OSError, ValueError):
        return None


def run(executable, root, port):
    app = ResolvedApp(executable)
    ctx = ConformanceContext(app, "windows", root / "config", root / "project", root / "out")
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    settings.write_text(settings.read_text().replace("resume_ai=0", "resume_ai=1"), encoding="utf-8")
    home = root / "provider"
    home.mkdir()
    configure_provider(home, ctx.work_dir, port)
    shell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
    launch = {"kind": "shell", "name": "Windows PowerShell", "program": str(shell),
              "args": ["-NoLogo", "-NoProfile"]}
    leaf = {"kind": "pane", "launch": launch, "cwd": str(ctx.work_dir)}
    layout = {"kind": "split", "axis": "top_bottom", "ratio_permille": 500,
              "first": leaf, "second": leaf}
    tab = {"launch": launch, "layout": layout, "cwd": str(ctx.work_dir), "active_pane": 0}
    ctx.session_file.write_text(json.dumps({"version": 4, "tabs": [tab], "active_tab": 0,
                                            "clean_exit": True}), encoding="utf-8")
    cli = Path(os.environ["PEBREL_QA_CODEX_BIN"]).resolve(strict=True)
    quote = lambda path: "'" + str(path).replace("'", "''") + "'"
    try:
        ctx.start(explicit_working_directory=False)
        ctx.refresh_targets()
        top, bottom = split_leaves(ctx)
        for pane in (top, bottom):
            shell_reply(ctx, pane)
        marker = "qa-split-" + uuid4().hex[:8]
        shell_command(ctx, top, f"$env:CODEX_HOME={quote(home)}")
        shell_command(ctx, top, f"& {quote(cli)} --no-daemon {marker}")
        native = ctx.poll(lambda: provider_id(home, marker), bool, "native ID missing", timeout=45)
        ctx.poll(lambda: saved_agents(ctx), lambda a: a and a[0] and a[0].get("source") == "codex"
                 and a[0].get("session_id") == native and not a[1],
                 "Codex did not bind to its own leaf", timeout=65)
        shell_reply(ctx, bottom)
        ctx.poll(lambda: "Fixture reply." in ctx.read(top)["text"], bool, "model reply missing")
        ctx.prompt("/exit", top)
        ctx.poll(lambda: saved_agents(ctx), lambda a: a == [None, None],
                 "exited Codex remained in the snapshot", timeout=35)
        shell_reply(ctx, top)
        native_close(ctx)
        assert saved_agents(ctx) == [None, None]
        ctx.start(explicit_working_directory=False)
        ctx.refresh_targets()
        for pane in split_leaves(ctx):
            shell_reply(ctx, pane)
        assert saved_agents(ctx) == [None, None]
        print("Verified the native ID, shell-only sibling, clean WM_CLOSE and both restored shells.", flush=True)
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated hosted Windows desktop")
    with ThreadingHTTPServer(("127.0.0.1", 0), LocalModel) as server:
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            with tempfile.TemporaryDirectory(prefix="pebrel-shell-split-", ignore_cleanup_errors=True) as tmp:
                for i in range(3):
                    run(sys.argv[1], Path(tmp) / str(i), server.server_port)
        finally:
            server.shutdown()
