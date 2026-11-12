"""Exercise the published Windows runtime with foreground Win32 key events."""
import argparse
import ctypes
from ctypes import wintypes as wt
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import time

from harness import ConformanceContext, ConformanceError, ResolvedApp, require


class KeyInput(ctypes.Structure):
    _fields_ = [
        ("vk", wt.WORD), ("scan", wt.WORD), ("flags", wt.DWORD),
        ("time", wt.DWORD), ("extra", ctypes.c_size_t),
    ]


class MouseInput(ctypes.Structure):
    _fields_ = [
        ("x", wt.LONG), ("y", wt.LONG), ("data", wt.DWORD), ("flags", wt.DWORD),
        ("time", wt.DWORD), ("extra", ctypes.c_size_t),
    ]


class InputData(ctypes.Union):
    _fields_ = [("key", KeyInput), ("mouse", MouseInput)]


class Input(ctypes.Structure):
    _fields_ = [("kind", wt.DWORD), ("data", InputData)]


class Keyboard:
    def __init__(self, pid):
        self.pid = pid
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        for name, result, arguments in [
            ("GetForegroundWindow", wt.HWND, []),
            ("GetWindowThreadProcessId", wt.DWORD, [wt.HWND, ctypes.POINTER(wt.DWORD)]),
            ("SendInput", wt.UINT, [wt.UINT, ctypes.POINTER(Input), ctypes.c_int]),
        ]:
            function = getattr(self.user, name)
            function.restype, function.argtypes = result, arguments
        require(ctypes.sizeof(Input) == 40, "unexpected Win32 INPUT layout")

    def foreground_pid(self):
        pid = wt.DWORD()
        window = self.user.GetForegroundWindow()
        if window:
            self.user.GetWindowThreadProcessId(window, ctypes.byref(pid))
        return pid.value

    def press(self, scan, *, extended=False):
        require(self.foreground_pid() == self.pid, "the foreground window is not the test instance")
        flags = 0x08 | int(extended)  # KEYEVENTF_SCANCODE | optional EXTENDEDKEY
        for state in (0, 0x02):  # KEYEVENTF_KEYUP
            event = Input(1, InputData(key=KeyInput(0, scan, flags | state, 0, 0)))
            if self.user.SendInput(1, ctypes.byref(event), ctypes.sizeof(event)) != 1:
                raise ConformanceError(f"SendInput rejected scan code {scan:#x}")
            time.sleep(0.04)

    def open_mcp(self):
        for scan in (0x35, 0x32, 0x2E, 0x19):  # /mcp on the hosted US keyboard
            self.press(scan)
        time.sleep(0.3)
        self.press(0x1C)  # Return


def quote_pwsh(path):
    return "'" + str(path).replace("'", "''") + "'"


def screen_text(ctx):
    response = ctx.api("pane.read", {
        "window_id": ctx.window_id, "pane_id": ctx.pane_id, "lines": 1, "screen": True,
    })
    return "\n".join("".join(cell[0] for cell in row).rstrip() for row in response["screen"]["rows"])


def cli_config(root, work):
    config = root / "cli-config"
    config.mkdir()
    key = "pebrel-test-local-fixture-key"
    settings = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
    (config / ".claude.json").write_text(json.dumps({
        "hasCompletedOnboarding": True,
        "theme": "dark",
        "customApiKeyResponses": {"approved": [key[-20:]], "rejected": []},
        "projects": {str(work): settings, work.as_posix(): settings},
    }), encoding="utf-8")
    mcp = root / "mcp.json"
    mcp.write_text(json.dumps({"mcpServers": {"escape-fixture": {
        "type": "stdio", "command": sys.executable,
        "args": [str(Path(__file__).with_name("windows_escape_mcp_fixture.py").resolve())],
    }}}), encoding="utf-8")
    for name in list(os.environ):
        if name.startswith(("CLAUDE_", "ANTHROPIC_")):
            del os.environ[name]
    os.environ.update({
        "CLAUDE_CONFIG_DIR": str(config),
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
        "DISABLE_AUTOUPDATER": "1",
        "DISABLE_TELEMETRY": "1",
        "ANTHROPIC_BASE_URL": "http://127.0.0.1:9",
        "ANTHROPIC_API_KEY": key,
    })
    return mcp


def run(args, root, report):
    work = root / "work"
    mcp = cli_config(root, work)
    report["cli"] = subprocess.check_output([str(args.cli), "--version"], text=True).strip()
    require(report["cli"].startswith("2.1.292 "), "unexpected CLI version")
    app = ResolvedApp(args.archive)
    ctx = ConformanceContext(app, "windows-x86_64", root / "pebrel-config", work, args.evidence)
    report["stage"] = "pebrel_startup"

    def save(label, text):
        (args.evidence / f"{label}.txt").write_text(text, encoding="utf-8")

    def observe(label, predicate, timeout=10):
        try:
            text = ctx.poll(lambda: screen_text(ctx), predicate, f"did not reach {label}", timeout)
        except ConformanceError:
            save(label, screen_text(ctx))
            raise
        save(label, text)

    def main_prompt(text):
        return "for agents" in text and "Manage MCP servers" not in text and "enter to return" not in text

    def menu_open(text):
        return "Manage MCP servers" in text and "escape-fixture" in text

    try:
        ctx.prepare()
        path = ctx.config_dir / "pebrel_settings.txt"
        settings = path.read_text(encoding="utf-8").replace("restore_session=1", "restore_session=0")
        path.write_text(settings + "shell=pwsh\nlanguage=en-US\n", encoding="utf-8")
        ctx.start()
        snapshot = ctx.snapshot()
        ctx.refresh_targets(snapshot)
        report["stage"] = "powershell_version"
        ctx.prompt("[Console]::WriteLine(('PEBREL_' + 'PS=' + $PSVersionTable.PSEdition + '|' + $PSVersionTable.PSVersion.ToString()))")
        match, _ = ctx.wait_for_line(re.compile(r"PEBREL_PS=Core\|(7\.[0-9.]+)"))
        report["powershell"] = match.group(1)
        report["stage"] = "cli_startup"
        ctx.prompt(f"& {quote_pwsh(args.cli)} --strict-mcp-config --mcp-config {quote_pwsh(mcp)}")
        observe("prompt", main_prompt, 30)
        ctx.api("window.focus", {"window_id": ctx.window_id, "pane_id": ctx.pane_id})
        keyboard = Keyboard(snapshot["process_id"])
        report["stage"] = "foreground"
        ctx.poll(keyboard.foreground_pid, lambda pid: pid == keyboard.pid, "test window is not foreground", 5)

        report["stage"] = "left_background"
        keyboard.press(0x4B, extended=True)
        observe("left", lambda text: "enter to return" in text or "conversation moved to the background" in text)
        report["stage"] = "right_reattach"
        keyboard.press(0x4D, extended=True)
        observe("right", main_prompt)
        report["stage"] = "native_mcp"
        keyboard.open_mcp()
        observe("mcp-open", menu_open)

        report["stage"] = "native_escape"
        keyboard.press(0x01)
        try:
            observe("native-escape", main_prompt, 5)
        except ConformanceError:
            require(menu_open(screen_text(ctx)), "native Escape did not return to the prompt or keep the MCP menu open")
            report["native_escape_closed"] = False
            report["stage"] = "runtime_escape_after_native_failure"
            ctx.send_key("escape")
            try:
                observe("runtime-escape", main_prompt, 5)
            except ConformanceError:
                require(menu_open(screen_text(ctx)), "Runtime Escape led to an unexpected terminal state")
                report["runtime_escape_closed"] = False
            else:
                report["runtime_escape_closed"] = True
            report["status"] = "reproduced"
            return 1
        report["native_escape_closed"] = True
        report["status"] = "passed"
        return 0
    finally:
        ctx.stop(force=True)
        app.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    args.archive, args.cli, args.evidence = (path.resolve() for path in (args.archive, args.cli, args.evidence))
    args.evidence.mkdir(parents=True, exist_ok=True)
    report = {"status": "blocked", "stage": "setup", "windows": platform.version(),
              "pebrel": "2.5.3", "input": "Win32 SendInput to the verified foreground window",
              "application_hooks": "disabled"}
    try:
        with tempfile.TemporaryDirectory(prefix="pebrel-655-") as temporary:
            return run(args, Path(temporary), report)
    except (ConformanceError, OSError, subprocess.SubprocessError) as error:
        report["error"] = str(error)
        return 1
    finally:
        encoded = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        (args.evidence / "result.json").write_text(encoded, encoding="utf-8")
        print(encoded, end="", flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
