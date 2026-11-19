"""Reopen a real OMP 18.22.2 conversation after terminating its Pebrel workspace."""
from collections import Counter
import ctypes
from ctypes import wintypes
import json
import os
import re
import shutil
import subprocess
from pathlib import Path
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp
from conformance.omp_fixture_provider import REPLY, start_provider


SID = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c"
MARKER = "PEBREL-OMP-RESTORED-1927"


def write_conversation(path, cwd):
    stamp = "2026-11-19T06:41:00.000Z"
    title = {"type": "title", "v": 1, "title": "Recovery check", "updatedAt": stamp, "pad": ""}
    title["pad"] = " " * (255 - len(json.dumps(title, separators=(",", ":"))))
    records = [
        title, {"type": "session", "version": 3, "id": SID, "cwd": str(cwd), "timestamp": stamp},
        {"type": "message", "id": "0033aabb", "parentId": None, "timestamp": stamp,
         "message": {"role": "user", "content": [{"type": "text", "text": MARKER}],
                     "timestamp": 1795068000000}},
    ]
    path.parent.mkdir()
    path.write_text("".join(json.dumps(item, separators=(",", ":")) + "\n" for item in records),
                    encoding="utf-8", newline="\n")


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
    # The stable dialog paints its danger fill at 16% opacity. Compare it
    # with the neutral Cancel button mirrored across the center of the dialog.
    def find_confirm():
        frame = ImageGrab.grab(bbox=box)
        width, height = frame.size
        rows = []
        for y in range(height // 2 - 12, min(height, height // 2 + 180), 2):
            hits = []
            for x in range(width // 2, min(width, width // 2 + 320), 3):
                color = frame.getpixel((x, y))[:3]
                r, g, b = color
                cr, cg, cb = frame.getpixel((width - x - 1, y))[:3]
                tint = (r - g) - (cr - cg)
                if tint > 9 and (r - b) - (cr - cb) > 5 and color != before.getpixel((x, y))[:3]:
                    hits.append(x)
            if len(hits) >= 17 and hits[-1] - hits[0] < len(hits) * 4:
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


def run(app, root, fresh):
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "project",
                             root / "evidence", startup_timeout=40)
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    settings.write_text(settings.read_text().replace("resume_ai=0", "resume_ai=1")
                        + "ai_hooks_omp=1\nshell=pwsh\n", encoding="utf-8")
    profile = Path(os.environ["USERPROFILE"])
    agent_dir = profile / ".omp" / "agent"
    agent_dir.mkdir(parents=True, exist_ok=True)
    events = root / "omp-event-routing.txt"
    os.environ["PEBREL_TEST_OMP_EVENTS"] = str(events)
    hook_log = root / "hook-outcomes.jsonl"
    os.environ.update(PEBREL_HOOK_LOG=str(hook_log), PI_CODING_AGENT_DIR=str(agent_dir),
                      OMP_SKIP_VERSION_CHECK="1", OMP_SKIP_SETUP="1", PI_OFFLINE="1",
                      OPENAI_API_KEY="fixture-only", OPENAI_BASE_URL="http://127.0.0.1:9/v1")
    source = root / "custom sessions" / ("2026-11-19T06-41-00-000Z_" + SID + ".jsonl")
    provider = start_provider(agent_dir) if fresh else None
    if not fresh:
        write_conversation(source, ctx.work_dir)

    def live_agent():
        tab = ctx.tab_for_pane(ctx.snapshot(), ctx.pane_id)
        pane = next(p for p in tab["panes"] if p["id"] == ctx.pane_id)
        agent = pane.get("agent") or {}
        return agent.get("kind"), agent.get("session_id"), agent.get("hook_seen")

    def saved_agent():
        try:
            data = json.loads(ctx.session_file.read_text(encoding="utf-8"))
            return data["tabs"][0]["layout"].get("agent", {})
        except (OSError, ValueError, KeyError, IndexError):
            return {}

    def verify_live(session_id):
        ctx.poll(live_agent, lambda agent: agent == ("omp", session_id, True),
                 "the running OMP process did not confirm the saved session", timeout=50)
        ctx.poll(lambda: MARKER in ctx.read(lines=100)["text"], bool,
                 "the transcript message was missing from the live terminal", timeout=20)

    binary = shutil.which("omp")
    if not binary:
        raise RuntimeError("Verified OMP executable is missing from the ordinary user's PATH")
    version = subprocess.run([binary, "--version"], capture_output=True, text=True,
                             timeout=25, cwd=ctx.work_dir, errors="replace")
    print("OMP version check:", version.returncode,
          (version.stdout + version.stderr).strip()[:350], flush=True)
    version.check_returncode()
    try:
        ctx.start()
        ctx.refresh_targets()
        extension = agent_dir / "extensions" / "pebrel.ts"
        ctx.poll(extension.is_file, bool, "Pebrel did not install the OMP integration", timeout=20)
        ctx.prompt("Write-Output ('PEBREL-PWSH-' + $PSVersionTable.PSVersion.Major)")
        ctx.poll(lambda: "PEBREL-PWSH-7" in ctx.read()["text"], bool,
                 "the pane did not run PowerShell 7")
        if fresh:
            ctx.prompt("omp --model pebrelfixture/fixture")
            _, session_id, _ = ctx.poll(
                live_agent, lambda state: state[0] == "omp" and state[1] and state[2],
                "a new OMP session did not announce its native identity", timeout=60)
            ctx.prompt(MARKER)
            ctx.poll(lambda: REPLY in ctx.read(lines=100)["text"], bool,
                     "the native OMP editor did not receive the local model reply", timeout=50)
            def native_conversation():
                for transcript in (agent_dir / "sessions").rglob("*.jsonl"):
                    try:
                        records = [json.loads(row) for row in transcript.read_text(
                            encoding="utf-8").splitlines() if row.strip()]
                    except (OSError, ValueError):
                        continue
                    if (any(r.get("type") == "session" and r.get("id") == session_id for r in records)
                            and any(r.get("type") == "message" and MARKER in str(r["message"])
                                    for r in records)):
                        return transcript
                return None

            source = ctx.poll(native_conversation, bool,
                              "OMP did not persist the submitted message in its default catalog",
                              timeout=90)
        else:
            session_id = SID
            ctx.prompt("omp --resume '" + str(source).replace("'", "''") + "'")
        verify_live(session_id)
        ctx.poll(saved_agent, lambda a: a.get("session_id") == session_id
                 and Path(a.get("session_file") or "") == source,
                 "autosave did not retain the native file and identity", timeout=30)
        print("PowerShell 7 displayed the message and autosave recorded its exact transcript.", flush=True)
        first_pid = ctx.snapshot()["process_id"]
        hook_log = root / "forced-restart-hooks.jsonl"
        os.environ["PEBREL_HOOK_LOG"] = str(hook_log)
        ctx.restart()
        assert ctx.snapshot()["process_id"] != first_pid
        ctx.refresh_targets()
        verify_live(session_id)
        print("A fresh Pebrel process restored OMP's confirmed ID and the stored message.", flush=True)
        if fresh:
            confirm_native_window_close(ctx)
            hook_log = root / "normal-restart-hooks.jsonl"
            os.environ["PEBREL_HOOK_LOG"] = str(hook_log)
            ctx.start(explicit_working_directory=False)
            ctx.refresh_targets()
            verify_live(session_id)
            print("Normal window close and reopen restored the same conversation.", flush=True)
    except Exception:
        if events.is_file():
            print("Isolated OMP lifecycle events:", events.read_text(encoding="utf-8")[-4000:], flush=True)
        current = saved_agent() or {}
        print("Persisted OMP identity:", current.get("source"), bool(current.get("session_id")),
              bool(current.get("session_file")), flush=True)
        outcomes = Counter()
        if hook_log.exists():
            for line in hook_log.read_text(encoding="utf-8", errors="replace").splitlines():
                try:
                    entry = json.loads(line)
                except ValueError:
                    continue
                if entry.get("source") == "omp":
                    outcomes[entry.get("outcome")] += 1
        print("OMP hook delivery outcomes since the last launch:", dict(outcomes), flush=True)
        if ctx.client and ctx.pane_id:
            screen = ctx.read(lines=45).get("text", "")
            screen = re.sub(r"(?i)(api[_ -]?key|token)(\s*[:=]\s*)[^\s]+", r"\1\2[redacted]", screen)
            print("Isolated test pane:", screen[-3000:], flush=True)
            processes = ctx.api("pane.procs", {"window_id": ctx.window_id, "pane_id": ctx.pane_id},
                                allow_error=True)
            rows = (processes.get("result") or {}).get("processes") or []
            print("Pane processes:", [(row.get("depth"), Path(row.get("executable", "")).name)
                                       for row in rows], flush=True)
        raise
    finally:
        ctx.stop(force=True)
        app.close()
        if provider:
            provider.shutdown()
            provider.server_close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Run on an isolated hosted Windows runner")
    with tempfile.TemporaryDirectory(prefix="pebrel-omp-", ignore_cleanup_errors=True) as directory:
        run(ResolvedApp(sys.argv[1]), Path(directory), "--fresh" in sys.argv[2:])
