"""Exercise reported mouse gestures in a synthetic multi-pane Windows workspace."""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time

from pi_image_paste_probe import ConformanceContext, ResolvedApp, focus_window

OBSERVER = r"""
import fs from "node:fs";
import path from "node:path";
export default function (pi) {
  pi.on("session_start", (_event, ctx) => {
    const base = path.join(process.env.PEBREL_MOUSE_DIR, process.env.PEBREL_MOUSE_PANE);
    let reports = 0, bytes = 0, dirty = false;
    let carry = "";
    process.stdin.on("data", chunk => {
      bytes += chunk.length;
      const data = carry + String(chunk);
      const events = [...data.matchAll(/\x1b\[<\d+;\d+;\d+[Mm]/g)];
      reports += events.length;
      const last = events.at(-1);
      carry = (last ? data.slice(last.index + last[0].length) : data).slice(-32);
    });
    setInterval(() => {
      dirty ||= ctx.ui.getEditorText().length > 0;
      fs.writeFileSync(base + ".tmp", JSON.stringify({reports, bytes, dirty, pid:process.pid}));
      fs.renameSync(base + ".tmp", base + ".json");
    }, 250).unref();
  });
}
"""


def pane_ids(snapshot):
    return {pane["id"] for tab in snapshot["windows"][0]["tabs"] for pane in tab["panes"]}


def gesture(ctx, pid, rounds):
    user = ctypes.WinDLL("user32", use_last_error=True)
    user.GetForegroundWindow.restype = wintypes.HWND
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
    user.mouse_event.argtypes = [wintypes.DWORD] * 4 + [ctypes.c_size_t]
    focus_window(ctx, pid)
    hwnd = user.GetForegroundWindow()
    owner = wintypes.DWORD()
    user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
    if owner.value != pid:
        raise RuntimeError("Synthetic gestures require the test window")
    area, origin = wintypes.RECT(), wintypes.POINT()
    if not user.GetClientRect(hwnd, ctypes.byref(area)) or not user.ClientToScreen(hwnd, ctypes.byref(origin)):
        raise ctypes.WinError(ctypes.get_last_error())

    def move(x, y):
        if user.GetForegroundWindow() != hwnd:
            raise RuntimeError("The test window lost foreground input")
        if not user.SetCursorPos(origin.x + int(area.right*x), origin.y + int(area.bottom*y)):
            raise ctypes.WinError(ctypes.get_last_error())

    for i in range(rounds):
        y = 0.29 + (i % 4)*0.12
        move(0.43, y)
        user.mouse_event(2, 0, 0, 0, 0)  # left down
        try:
            for step in range(1, 25):
                move(0.43 + 0.019*step, y + 0.004*step)
                time.sleep(0.008)
        finally:
            user.mouse_event(4, 0, 0, 0, 0)
        for delta in (120, -120):
            user.mouse_event(0x0800, 0, 0, delta & 0xffffffff, 0)
        time.sleep(0.03)


def run(app, root):
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "work",
                             root / "evidence", startup_timeout=50)
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    with settings.open("a", encoding="utf-8") as output:
        output.write("shell=powershell\ncopy_on_select=1\n")
    observer = root / "observer.mjs"
    observer.write_text(OBSERVER, encoding="utf-8")
    os.environ.update(PEBREL_MOUSE_DIR=str(root), PI_OFFLINE="1",
                      PI_CODING_AGENT_DIR=str(root / "pi-profile"))
    def quote(path):
        return "'" + str(path).replace("'", "''") + "'"
    try:
        ctx.start()
        ctx.refresh_targets()
        original = ctx.pane_id
        def split(source, direction):
            return ctx.api("pane.split", {"window_id":ctx.window_id, "pane_id":source,
                                           "direction":direction})["action"]["pane_id"]
        right = split(original, "left_right")
        split(right, "top_bottom")
        lower = split(original, "top_bottom")
        split(lower, "left_right")
        for _ in range(12):
            ctx.api("tab.new", {"window_id":ctx.window_id})
        snapshot = ctx.snapshot()
        expected = sorted(pane_ids(snapshot))
        assert len(snapshot["windows"][0]["tabs"]) == 13 and len(expected) == 17
        idle = {lower, expected[-2], expected[-1]}
        editors = sorted(set(expected) - idle)
        for pane in expected:
            ctx.api("window.focus", {"window_id":ctx.window_id, "pane_id":pane})
            ctx.poll(lambda: ctx.read(pane)["text"], lambda s: bool(s.strip()), f"shell {pane} not ready", 60)
            ctx.prompt("Write-Output ('PS-VERSION-' + $PSVersionTable.PSVersion.ToString(2))", pane)
            ctx.poll(lambda: ctx.read(pane)["text"], lambda s: "PS-VERSION-5.1" in s, "PowerShell 5.1 required", 30)
            if pane in editors:
                ctx.prompt(f"$env:PEBREL_MOUSE_PANE='{pane}'; & " + quote(os.environ["PEBREL_FIXTURE_PI"])
                           + " --no-session --tui-mode fullscreen -e " + quote(observer), pane)
                ctx.poll((root / f"{pane}.json").is_file, bool, f"Pi never started in pane {pane}", 50)
        def observations():
            return {pane: json.loads((root / f"{pane}.json").read_text()) for pane in editors}
        before = observations()
        assert all(not obs["dirty"] for obs in before.values())
        print("13 tabs, 17 Windows PowerShell 5.1 panes, 14 independent Pi 1.5.0 editors ready.", flush=True)
        for pane in [original, editors[-1], original, editors[-2], original]:
            ctx.pane_id = pane
            gesture(ctx, snapshot["process_id"], 8)
        time.sleep(1)
        final = ctx.snapshot()
        lost = sorted(set(expected) - pane_ids(final))
        closed = [ev["pane_id"] for ev in final.get("pane_lifecycles", [])
                  if ev["event"] == "closed" and ev["pane_id"] in expected]
        after = observations()
        delivered = sum(after[p]["reports"] - before[p]["reports"] for p in editors)
        dirty = [p for p in editors if after[p]["dirty"]]
        idle_fragments = [p for p in idle if p not in lost and re.search(r"\d+;\d+;\d+[Mm]", ctx.read(p)["text"])]
        print(json.dumps(dict(native_drags=40, wheel_events=80, raw_sgr_reports=delivered,
                              stdin_bytes=sum(after[p]["bytes"] - before[p]["bytes"] for p in editors),
                              editor_text_changed=dirty, idle_sgr=idle_fragments, lost=lost, closed=closed)), flush=True)
        assert delivered > 0, "Pi did not receive measurable SGR mouse input"
        assert not (dirty or idle_fragments or lost or closed), "Detected a reported symptom"
        for pane in editors:
            processes = ctx.api("pane.procs", {"window_id":ctx.window_id, "pane_id":pane})["processes"]
            assert any(row["pid"] == before[pane]["pid"] for row in processes), "Original Pi process exited"
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Run on a dedicated hosted Windows desktop")
    with tempfile.TemporaryDirectory(prefix="pebrel-mouse-", ignore_cleanup_errors=True) as directory:
        run(ResolvedApp(sys.argv[1]), Path(directory))
