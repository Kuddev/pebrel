"""Compare the imported default in new tabs and consecutive splits on a Windows desktop."""
from contextlib import ExitStack
import json
import os
from pathlib import Path
import sys
from tempfile import TemporaryDirectory

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


def root_process(ctx, pane):
    def roots():
        result = ctx.api("pane.procs", {"window_id": ctx.window_id, "pane_id": pane})
        return [p for p in result["processes"] if p["depth"] == 0]
    rows = ctx.poll(roots, lambda rows: len(rows) == 1, "missing pane root", timeout=20)
    name = Path(rows[0]["executable"]).name.lower()
    print(f"Pane {pane} runs {name}, PID {rows[0]['pid']}", flush=True)
    return name, rows[0]["pid"]


def exercise(ctx, baseline):
    for origin in ("default", "cmd"):
        params = {"window_id": ctx.window_id}
        if origin == "cmd":
            params["shell"] = "cmd"
        pane = ctx.api("tab.new", params)["action"]["pane_id"]
        observed = [root_process(ctx, pane)]
        for direction in ("left_right", "top_bottom"):
            result = ctx.api("pane.split", {"window_id": ctx.window_id, "pane_id": pane,
                                           "direction": direction})
            pane = result["action"]["pane_id"]
            observed.append(root_process(ctx, pane))
        expected = ["niu.exe" if origin == "default" else "cmd.exe"]
        expected += ["powershell.exe" if baseline else "niu.exe"] * 2
        assert [name for name, _ in observed] == expected, (origin, observed, expected)
        assert len({pid for _, pid in observed}) == 3
        tab = ctx.tab_for_pane(ctx.snapshot(), pane)
        assert len(tab["panes"]) == 3
        print(f"Verified {origin} tab and its two splits: {expected}", flush=True)


if __name__ == "__main__":
    mode, binary, niu = sys.argv[1:]
    assert os.name == "nt" and mode in {"published", "candidate"}
    niu = Path(niu).resolve(strict=True)
    with ExitStack() as stack:
        root = Path(stack.enter_context(TemporaryDirectory(ignore_cleanup_errors=True)))
        app = ResolvedApp(binary)
        stack.callback(app.close)
        ctx = ConformanceContext(app, "windows-x86_64", root / "config",
                                 root / "work", root / "artifacts")
        ctx.prepare()
        profile = {"name": "Niubash", "id": "niu-check", "shell_id": "niubash",
                   "command": str(niu), "args": ["--login", "-i"]}
        (ctx.config_dir / "terminal_profiles.json").write_text(
            json.dumps({"version": 1, "profiles": [profile]}), encoding="utf-8")
        with (ctx.config_dir / "pebrel_settings.txt").open("a", encoding="utf-8") as settings:
            settings.write("shell=profile:niubash|niu-check\n")
        stack.callback(ctx.stop, force=True)
        ctx.start()
        ctx.refresh_targets()
        exercise(ctx, mode == "published")
