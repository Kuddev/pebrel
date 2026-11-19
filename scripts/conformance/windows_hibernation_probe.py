"""Verify the released terminal's Windows hibernation and subsequent session restore."""
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


def run(executable, root):
    ctx = ConformanceContext(executable, "windows-x86_64", root / "config", root / "project",
                             root / "evidence", startup_timeout=45)
    ctx.prepare()
    with (ctx.config_dir / "pebrel_settings.txt").open("a", encoding="utf-8") as settings:
        settings.write("shell=powershell\ntab_sleep_minutes=5\n")
    listing_marker = "sleep-listing-fixture.txt"
    raw_marker = "sleep-output-9284"
    (ctx.work_dir / listing_marker).write_text("synthetic\n", encoding="utf-8")

    def matches(tab_id, snapshot):
        return next((t for t in snapshot["windows"][0]["tabs"] if t.get("tab_id") == tab_id), None)

    def renamed(label):
        ctx.refresh_targets()
        active = ctx.tab_for_pane(ctx.snapshot(), ctx.pane_id)
        ctx.api("tab.rename", {"window_id": ctx.window_id, "tab_index": active["index"], "name": label})
        return active["tab_id"]

    os.environ.update(NEBULA_PTY_RECORD="1", NEBULA_RESIZE_TRACE="1")
    results = {}
    try:
        ctx.start()
        ctx.refresh_targets()
        pane = ctx.pane_id
        tab = renamed("sleep-probe-A")
        ctx.prompt("Write-Output ('sleep-output-' + '9284')", pane)
        ctx.poll(lambda: raw_marker in ctx.read(pane)["text"], bool, "missing synthetic stdout", 35)
        ctx.prompt("ls", pane)
        time.sleep(2)
        before = ctx.read(pane, lines=200)
        results["before_sleep"] = {"stdout": raw_marker in before["text"], "ls": listing_marker in before["text"]}
        print("Initial output:", results["before_sleep"], flush=True)
        ctx.api("tab.new", {"window_id": ctx.window_id})
        renamed("sleep-probe-B")
        started = time.monotonic()
        # Sleeping tabs can disappear from the runtime projection on older releases.
        while time.monotonic() - started < 365:
            state = ctx.snapshot()
            if any(ev.get("event") == "closed" and ev.get("pane_id") == pane
                   for ev in state.get("pane_lifecycles", [])):
                break
            time.sleep(2)
        else:
            raise RuntimeError("The idle PowerShell pane did not enter automatic sleep")
        results["slept_after_seconds"] = round(time.monotonic() - started)
        trace = ctx.artifact_dir / "pebrel-1.log"
        offset = trace.stat().st_size
        ctx.api("tab.focus", {"window_id": ctx.window_id, "tab_id": tab})
        live = ctx.poll(lambda: matches(tab, ctx.snapshot()),
                        lambda t: t and t["panes"] and t["panes"][0]["id"] != pane,
                        "could not wake the sleeping tab", 25)
        new_pane = live["panes"][0]["id"]
        ctx.prompt("Write-Output ('sleep-awake-' + '9284')", new_pane)
        ctx.poll(lambda: "sleep-awake-9284" in ctx.read(new_pane)["text"], bool, "woken shell not usable", 30)
        after = ctx.read(new_pane, lines=250)
        results["after_wake"] = {"stdout": raw_marker in after["text"], "ls": listing_marker in after["text"],
                                 "cwd": Path(live["panes"][0]["cwd"]) == ctx.work_dir}
        with trace.open("rb") as log:
            log.seek(offset)
            wake = log.read().decode("utf-8", errors="replace")
        results["trace_history"] = [
            [stage, int(count)] for stage, count in re.findall(
                r"\[nebula:resize-trace\] ([^ ]+) grid=[^\n]+? history=(\d+)", wake)
        ][:90]
        recordings = [p for p in (ctx.work_dir / "nebula.recording", Path.cwd() / "nebula.recording",
                                   ctx.config_dir / "nebula.recording") if p.is_file()]
        if recordings:
            stream = recordings[0].read_bytes()
            results["wake_sequences"] = {label: [m.start() for m in re.finditer(re.escape(seq), stream)][:8]
                                        for label, seq in [("ED3", b"\x1b[3J"), ("ED2", b"\x1b[2J"),
                                                           ("RIS", b"\x1bc"), ("prompt", b"\x1b]133;A")]}
        results["history_available_after"] = after["history_available"]
        ctx.close_and_wait(timeout=25)
        saved = json.loads(ctx.session_file.read_text(encoding="utf-8"))
        results["clean_exit"] = saved.get("clean_exit")
        ctx.start(explicit_working_directory=False)
        value = ctx.poll(ctx.snapshot, lambda s: all(t.get("panes") for t in s["windows"][0]["tabs"]),
                         "restored shells not ready", 25)
        results["restored_labels"] = [t["label"] for t in value["windows"][0]["tabs"]]
        print(json.dumps(results), flush=True)
        assert results["clean_exit"] and results["restored_labels"] == ["sleep-probe-A", "sleep-probe-B"]
        assert results["after_wake"]["cwd"] and all(results["before_sleep"].values())
        assert all(results["after_wake"].values()), "Previously visible output missing after wake"
    finally:
        print("Observed:", json.dumps(results), flush=True)
        ctx.stop(force=True)
        executable.close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Run on an isolated Windows runner")
    with tempfile.TemporaryDirectory(prefix="pebrel-sleep-", ignore_cleanup_errors=True) as directory:
        run(ResolvedApp(sys.argv[1]), Path(directory))
