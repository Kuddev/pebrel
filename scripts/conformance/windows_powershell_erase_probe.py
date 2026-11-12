"""Capture the v2.5.3 terminal grid and PTY output after PowerShell erases input."""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import tempfile

from scripts.conformance.harness import ConformanceContext, ResolvedApp, require


class RecordingContext(ConformanceContext):
    shell_args = ()

    def _spawn_process(self, command, env):
        env.update(NEBULA_PTY_RECORD="1", NEBULA_BOOT_TRACE="1")
        return super()._spawn_process([*command, *self.shell_args], env)


def grid(ctx):
    return ctx.api(
        "pane.read",
        {"window_id": ctx.window_id, "pane_id": ctx.pane_id, "screen": True, "lines": 1},
    )["screen"]


def visible_text(screen, x, y, count):
    columns = screen["columns"]
    cells = []
    for row in screen["rows"][y:]:
        for cell in row:
            cells.extend([cell[0], *(" " for _ in range(cell[1] - 1))])
        if len(cells) >= x + count:
            return "".join(cells[x:x + count])
        require(len(cells) % columns == 0, "unexpected terminal row width")
    return ""


def wait_grid(ctx, label, predicate):
    def observe():
        screen = grid(ctx)
        return screen if predicate(screen) else None

    return ctx.poll(observe, lambda screen: screen is not None, label)


def blank_background_changes(before, after):
    palette = dict(after["palette"])
    changed = {}
    for y, (old_row, new_row) in enumerate(zip(before["rows"], after["rows"], strict=True)):
        x = 0
        for old, new in zip(old_row, new_row, strict=True):
            if old[0] == new[0] == " " and old[3] != new[3]:
                bg = new[3]
                rgb = palette[-bg - 1] if bg < 0 else bg
                item = changed.setdefault((new[2], bg), {"rgb": f"#{rgb:06x}", "count": 0, "sample": []})
                item["count"] += 1
                if len(item["sample"]) < 5:
                    item["sample"].append([x, y])
            x += new[1]
    return [{"fg": fg, "bg": bg, **value} for (fg, bg), value in changed.items()]


def capture(ctx, out, label, count, *, individual):
    recording = ctx.work_dir / "nebula.recording"
    before = grid(ctx)
    x, y = before["cursor"][:2]
    columns = before["columns"]
    require(y + 1 < len(before["rows"]), "prompt too near the last row to compare grids")
    if count is None:
        count = columns - x + 6
    text = ("qxz" * count)[:count]
    base = recording.stat().st_size
    if individual:
        for char in text:
            ctx.prompt(char, submit=False)
    else:
        ctx.prompt(text, submit=False)
    end = ((x + count) % columns, y + (x + count) // columns)
    typed = wait_grid(ctx, f"{label}: did not see typed text and cursor", lambda screen: (
        tuple(screen["cursor"][:2]) == end and visible_text(screen, x, y, count) == text
    ))
    erase_start = recording.stat().st_size
    while count > 0:
        step = 1 if individual else min(12, count)
        ctx.api("pane.send_key", {
            "window_id": ctx.window_id, "pane_id": ctx.pane_id,
            "key": "backspace", "repeat": step,
        })
        count -= step
        expected = ((x + count) % columns, y + (x + count) // columns)
        after = wait_grid(ctx, f"{label}: Backspace did not reach {expected}", lambda screen: (
            tuple(screen["cursor"][:2]) == expected
            and visible_text(screen, x, y, len(text)) == text[:count].ljust(len(text))
        ))
    (out / f"{label}-screen.json").write_text(
        json.dumps({"before": before, "typed": typed, "after": after}, ensure_ascii=False),
        encoding="utf-8",
    )
    raw = recording.read_bytes()
    require(0 <= base < erase_start < len(raw), "missing typing or Backspace bytes")
    (out / f"{label}-typing.bin").write_bytes(raw[base:erase_start])
    (out / f"{label}-erase.bin").write_bytes(raw[erase_start:])
    return {"columns": columns, "origin": [x, y], "typed": len(text),
            "blank_background_changes": blank_background_changes(before, after)}


def run_case(app, case, shell, edition, bundled, temp, out, report):
    scratch = temp / case
    out.mkdir()
    ctx = RecordingContext(app, "windows-x86_64", scratch / "config", scratch / "work", scratch / "logs")
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    config = settings.read_text(encoding="utf-8").replace("restore_session=1", "restore_session=0")
    settings.write_text(config + f"theme=SlateLight\nshell=powershell\nopenconsole={int(bundled)}\n",
                        encoding="utf-8")
    if shell:
        ctx.shell_args = ("-e", str(shell), "-NoLogo", "-NoProfile")
    try:
        ctx.start()
        ctx.refresh_targets()
        wait_grid(ctx, "PowerShell prompt did not appear", lambda screen: (
            screen["cursor"][0] > 0 and screen["cursor"][2]
        ))
        ctx.prompt(
            "$m=Get-Module PSReadLine; if ($m) { Set-PSReadLineOption -HistorySaveStyle SaveNothing }; "
            "[Console]::WriteLine(('Q'+'A958:{0}|{1}|{2}' -f "
            "$PSVersionTable.PSEdition,$PSVersionTable.PSVersion,$m.Version))"
        )
        match, _ = ctx.wait_for_line(re.compile(r"^QA958:(Desktop|Core)\|([\d.]+)\|([\d.]+)$"))
        report.update(edition=match[1], powershell=match[2], psreadline=match[3])
        require(match[1] == edition, f"expected {edition} PowerShell, saw {match[1]}")
        log = (ctx.artifact_dir / "pebrel-1.log").read_text(encoding="utf-8", errors="replace")
        host = "sideloaded OpenConsole" if bundled else "in-box"
        require(f"conpty api ready ({host})" in log, f"could not confirm {host} from boot trace")
        report["host"] = host
        wait_grid(ctx, "prompt did not return after reporting versions", lambda screen: (
            screen["cursor"][0] > 0 and screen["cursor"][2]
            and "QA958:" not in "".join(c[0] for c in screen["rows"][screen["cursor"][1]])
        ))
        report["short"] = capture(ctx, out, "short", 8, individual=True)
        report["wrapped"] = capture(ctx, out, "wrapped", None, individual=False)
        report["status"] = "captured"
    finally:
        ctx.stop(force=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True)
    require(os.name == "nt", "run the diagnostic on native Windows")
    ps5 = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
    ps7 = shutil.which("pwsh.exe")
    require(ps5.is_file() and ps7, "Windows PowerShell and PowerShell 7 are required")
    report = {"pebrel": "2.5.3", "windows": platform.version(), "cases": {}}
    app = ResolvedApp(args.app)
    try:
        with tempfile.TemporaryDirectory(prefix="pebrel-958-") as tmp:
            for name, shell, edition, bundled in [
                ("ps5-clean-bundled", ps5, "Desktop", True),
                ("ps7-clean-bundled", ps7, "Core", True),
                ("ps5-clean-inbox", ps5, "Desktop", False),
                ("ps5-managed-bundled", None, "Desktop", True),
            ]:
                result = {"status": "incomplete"}
                report["cases"][name] = result
                try:
                    run_case(app, name, shell, edition, bundled, Path(tmp), args.output / name, result)
                except (RuntimeError, OSError, KeyError, ValueError) as error:
                    result.update(status="incomplete", error=str(error))
    finally:
        app.close()
        encoded = json.dumps(report, indent=2) + "\n"
        (args.output / "report.json").write_text(encoded, encoding="utf-8")
        print(encoded, end="", flush=True)
    return int(any(case["status"] != "captured" for case in report["cases"].values()))


if __name__ == "__main__":
    raise SystemExit(main())
