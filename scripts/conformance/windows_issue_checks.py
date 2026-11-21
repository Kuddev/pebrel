"""Exercise the file-tab cursor and the hookless OMP title on a hosted Windows desktop."""
from contextlib import ExitStack
import os
from pathlib import Path
import re
import sys
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.agent_recovery.model import local_model
from conformance.agent_recovery.providers import command_for, configure
from conformance.harness import ConformanceContext, ResolvedApp
from windows_memory_stress import NativeProcess


def shell_ready(ctx):
    ctx.poll(lambda: ctx.read()["text"], lambda text: bool(text.strip()), "no initial shell output")
    ctx.prompt("Write-Output ('PWSH-CHECK-' + $PSVersionTable.PSVersion.Major)")
    ctx.wait_for_line(re.compile(r"PWSH-CHECK-7"))


def cursor(ctx):
    test_file = ctx.work_dir / "侧栏文件.md"
    test_file.write_text("# 示例\n", encoding="utf-8")
    tab_id = ctx.snapshot()["windows"][0]["tabs"][0]["tab_id"]
    native = NativeProcess(SimpleNamespace(pid=ctx.snapshot()["process_id"]), ctx.app.executable)
    def grid():
        frame = ctx.api("pane.read", {"window_id": ctx.window_id, "pane_id": ctx.pane_id,
                                      "lines": 40, "screen": True})["screen"]
        return frame["columns"], frame["cursor"][:2], [[cell[0] for cell in row] for row in frame["rows"]]
    try:
        for resize in (False, True, False):
            width = grid()[0]
            value = "长路径目录/" * (width // 9 + 2) + "END"
            # Put the editable string at the end, avoiding any cursor travel through shell syntax.
            ctx.prompt("Set-Content -NoNewline -Encoding utf8 cjk-result '" + value + "'", submit=False)
            ctx.poll(lambda: ctx.read()["text"].replace("\n", ""), lambda text: "END" in text, "input missing")
            for _ in range(3):
                ctx.send_key("left")
            time.sleep(.3)
            before = grid()
            ctx.api("tab.open", {"window_id": ctx.window_id, "path": str(test_file)})
            ctx.poll(lambda: ctx.snapshot()["windows"][0]["active_tab"], lambda tab: tab != 0, "file not active")
            if resize:
                native.resize(780 if width > 85 else 1180, 715)
            ctx.api("tab.focus", {"window_id": ctx.window_id, "tab_id": tab_id})
            ctx.poll(lambda: ctx.snapshot()["windows"][0]["active_tab"], lambda tab: tab == 0, "terminal not active")
            if resize:
                ctx.poll(lambda: grid()[0], lambda columns: columns != width, "terminal not resized")
            time.sleep(.5)
            after = grid()
            print("Columns and cursor before/after:", before[:2], after[:2], flush=True)
            if not resize:
                assert before == after, "fixed-width terminal grid or cursor changed"
            ctx.prompt("界", submit=False)
            ctx.send_key("enter")
            output = ctx.work_dir / "cjk-result"
            expected = value[:-2] + "界" + value[-2:]
            ctx.poll(lambda: output.read_text(encoding="utf-8-sig") if output.exists() else "",
                     lambda actual: actual == expected, "character inserted at the wrong position")
            output.unlink()
            print("Verified insertion at the original Unicode character.", flush=True)
    finally:
        native.close()


def omp(ctx):
    def state():
        tab = ctx.snapshot()["windows"][0]["tabs"][0]
        pane = tab["panes"][0]
        return tab["label"], pane.get("title"), pane.get("running_program")
    ctx.prompt(command_for("omp", 0))
    ctx.poll(state, lambda s: "π" in (s[1] or ""), "OMP did not become ready", timeout=45)
    ctx.prompt("Title test for issue 1029")
    ctx.poll(state, lambda s: "Native recovery fixture reply" in (s[1] or ""), "missing OMP title", timeout=45)
    for _ in range(4):
        time.sleep(.5)
        label, title, process = state()
        print("After reply:", label, title, process, flush=True)
        assert label == title and process == "omp", "task label lost after redraw"
    ctx.send_key("d", control=True)
    ctx.poll(state, lambda s: s[0] == ctx.work_dir.name and s[2] is None, "label after OMP exit", timeout=20)


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated hosted Windows desktop")
    with ExitStack() as stack:
        root = Path(stack.enter_context(TemporaryDirectory(prefix="pebrel-windows-qa-", ignore_cleanup_errors=True)))
        app = ResolvedApp(sys.argv[2])
        stack.callback(app.close)
        ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "ddlclock", root / "artifacts")
        ctx.prepare()
        with (ctx.config_dir / "pebrel_settings.txt").open("a", encoding="utf-8") as settings:
            settings.write("shell=pwsh\nai_hooks_omp=0\n")
        if sys.argv[1] == "omp":
            model = stack.enter_context(local_model())
            os.environ.update(configure("omp", root / "home", ctx.work_dir, f"http://127.0.0.1:{model.server_port}"))
        stack.callback(ctx.stop, force=True)
        ctx.start()
        ctx.refresh_targets()
        shell_ready(ctx)
        try:
            {"cursor": cursor, "omp": omp}[sys.argv[1]](ctx)
        except Exception:
            print("Fixture pane:", ctx.read()["text"][-1600:], flush=True)
            for log in ctx.artifact_dir.glob("pebrel-*.log"):
                rows = log.read_text(encoding="utf-8", errors="replace").splitlines()
                for row in [r for r in rows if r.startswith("QA-OMP ")][-80:]:
                    print(row, flush=True)
            raise
