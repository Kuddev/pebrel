"""Real PTY delivery and transactional SSH file-adapter regressions."""
import base64
import importlib.util
import json
import os
from pathlib import Path
import re
import select
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ASSETS = Path(__file__).resolve().parents[2] / "nebula_app/res/hooks"
TOKEN = "0123456789abcdef0123456789abcdef"


def module(name):
    spec = importlib.util.spec_from_file_location(name, ASSETS / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


@unittest.skipUnless(os.name == "posix", "remote adapter targets POSIX SSH hosts")
class RemoteHooksTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="pebrel-remote-hooks-")
        self.addCleanup(self.tmp.cleanup)
        # macOS exposes its temporary directory through a system symlink.
        # Ordinary fixture paths must exercise transactions, while redirection
        # is supplied explicitly by the negative tests below.
        self.root = Path(self.tmp.name).resolve()
        env = {"HOME": str(self.root), "PATH": os.environ["PATH"], "SHELL": "/bin/bash",
               "XDG_DATA_HOME": str(self.root / "data"), "XDG_CACHE_HOME": str(self.root / "cache"),
               "PEBREL_REMOTE_HOOK_TOKEN": TOKEN}
        self.environment = patch.dict(os.environ, env, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def test_discovery_finds_nvm_cli_and_its_interpreter_without_login_path(self):
        files = module("remote_files")
        bindir = self.root / ".nvm/versions/node/v24.18.0/bin"
        bindir.mkdir(parents=True)
        node = bindir / "node"
        node.write_text("#!/bin/sh\nprintf 'hooks stable true\\n'\n")
        node.chmod(0o700)
        cli = bindir / "codex"
        cli.write_text("#!/usr/bin/env node\n")
        cli.chmod(0o700)
        with patch.dict(os.environ, {"PATH": "/usr/bin:/bin"}):
            self.assertEqual(files.find_program("codex"), str(cli))
            self.assertEqual(files.capture(str(cli), "features", "list"), "hooks stable true\n")
            configured = self.root / "bin"
            configured.mkdir()
            preferred = configured / "codex"
            preferred.write_text("#!/bin/sh\n")
            preferred.chmod(0o700)
            with patch.dict(os.environ, {"PATH": str(configured)}):
                self.assertEqual(files.find_program("codex"), str(preferred))

    def test_compare_and_swap_does_not_overwrite_concurrent_user_edit(self):
        files = module("remote_files")
        snapshot = files.snapshot()
        root, paths = files.locations()
        paths["claude"].parent.mkdir(parents=True)
        paths["claude"].write_text("user change")
        plan = [{"name": "claude", "expected": snapshot["files"]["claude"]["sha256"], "content": "new"}]
        with self.assertRaisesRegex(ValueError, "changed during setup"):
            files.apply(plan)
        self.assertEqual(paths["claude"].read_text(), "user change")

    def test_transaction_rolls_back_only_its_writes(self):
        files = module("remote_files")
        real_replace = files.replace
        root, paths = files.locations()
        def replace(path, content, executable=False):
            if path == paths["bridge.py"]:
                raise OSError("fixture write failure")
            real_replace(path, content, executable)
        plan = [{"name": name, "expected": None, "content": "new"} for name in ["pebrel-hook", "bridge.py"]]
        with patch.object(files, "replace", replace), self.assertRaises(OSError):
            files.apply(plan)
        self.assertFalse(paths["pebrel-hook"].exists())
        self.assertFalse(paths["bridge.py"].exists())

    def test_unknown_name_and_symlink_cannot_redirect_writes(self):
        files = module("remote_files")
        with self.assertRaises(ValueError):
            files.apply([{"name": "../outside", "expected": None, "content": "bad"}])
        root, paths = files.locations()
        outside = self.root / "outside"
        outside.write_text("preserve")
        paths["bridge.py"].symlink_to(outside)
        with self.assertRaises(ValueError):
            files.apply([{"name": "bridge.py", "expected": None, "content": "bad"}])
        self.assertEqual(outside.read_text(), "preserve")

    def test_symlinked_integration_directory_cannot_redirect_writes(self):
        files = module("remote_files")
        root, paths = files.locations()
        outside = self.root / "outside"
        outside.mkdir()
        root.parent.mkdir(parents=True)
        root.symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symbolic link"):
            files.apply([{"name": "bridge.py", "expected": None, "content": "bad"}])
        self.assertEqual(list(outside.iterdir()), [])

    def test_bsd_ancestry_recognizes_interpreted_provider_without_trusting_other_arguments(self):
        bridge = module("remote_bridge")
        prefix = "400 Sat Sep 19 12:00:00 2026 "
        for source, command in [
            ("codex", "/usr/bin/python3 /private/var/fixture/codex"),
            ("claude", "/usr/bin/node /opt/modules/@anthropic-ai/claude-code/cli.js"),
            ("pi", "/usr/bin/node /opt/modules/pi-coding-agent/dist/cli.js"),
        ]:
            def ps_output(args, **kwargs):
                observed = command if "args=" in args else command.split()[0]
                return (prefix + observed).encode()
            with self.subTest(source=source), patch.object(bridge.os, "getppid", return_value=500), patch.object(bridge.Path, "read_text", side_effect=FileNotFoundError), patch.object(bridge.subprocess, "check_output", side_effect=ps_output):
                self.assertRegex(bridge.process_identity(source), r"^500:[a-f0-9]{16}$")
        row = b"1 Sat Sep 19 12:00:00 2026 /usr/bin/python3 worker.py unrelated codex"
        with patch.object(bridge.os, "getppid", return_value=500), patch.object(bridge.Path, "read_text", side_effect=FileNotFoundError), patch.object(bridge.subprocess, "check_output", return_value=row):
            self.assertIsNone(bridge.process_identity("codex"))

    def test_installed_launcher_delivers_native_stdin_over_real_controlling_tty(self):
        import pty
        provider = self.root / "codex"
        bridge = ASSETS / "remote_bridge.py"
        # Parent ancestry supplies process identity, never a PID from JSON.
        provider.write_text(
            "import json, subprocess, sys\n"
            f"bridge={str(bridge)!r}\n"
            "for event in ['SessionStart','UserPromptSubmit','PermissionRequest','PreToolUse','Stop']:\n"
            " p={'hook_event_name':event,'session_id':'main','turn_id':'turn','pid':1}\n"
            " if event == 'PreToolUse': p.update(tool_name='request_user_input',tool_input={'questions':[{'id':'scope','question':'Which scope?'}]})\n"
            " if event == 'Stop': p['last_assistant_message']='x'*70000\n"
            " subprocess.run([sys.executable,bridge,'codex','--hooks=full'],input=json.dumps(p).encode(),check=True)\n"
        )
        pid, master = pty.fork()
        if pid == 0:
            os.execv(sys.executable, [sys.executable, str(provider)])
        output = b""
        try:
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                if select.select([master], [], [], 0.1)[0]:
                    try:
                        part = os.read(master, 65536)
                    except OSError:
                        break
                    if not part:
                        break
                    output += part
            waited, status = os.waitpid(pid, 0)
            self.assertEqual(status, 0, output.decode(errors="replace"))
        finally:
            os.close(master)
        envelopes = re.findall(rb"\x1b\]777;nebula-hook;" + TOKEN.encode() + rb";([^\x07]+)\x07", output)
        self.assertEqual(len(envelopes), 5, output[:2000])
        decoded = [base64.b64decode(value).split(b"\n", 1) for value in envelopes]
        for header, payload in decoded:
            self.assertIn(f"process={pid}:".encode(), header)
            self.assertIn(b"codex_hooks=full", header)
            self.assertLessEqual(len(header) + len(payload) + 1, 65536)
        payloads = [json.loads(payload) for _, payload in decoded]
        self.assertEqual([p["bridge_sequence"] for p in payloads], [1, 2, 3, 4, 5])
        self.assertEqual(payloads[3]["tool_input"]["questions"][0]["question"], "Which scope?")
        self.assertEqual(payloads[-1]["hook_event_name"], "Stop")
        self.assertNotIn("last_assistant_message", payloads[-1])

    def test_invalid_token_never_writes_and_failed_delivery_keeps_notify_chain(self):
        bridge = module("remote_bridge")
        with patch.dict(os.environ, {"PEBREL_REMOTE_HOOK_TOKEN": "bad"}), patch.object(bridge, "send") as send:
            bridge.run(["codex", '{"type":"agent-turn-complete"}'])
            send.assert_not_called()
        for payload in ['{"type":"agent-turn-complete"}', "malformed"]:
            with patch.object(bridge, "send", side_effect=OSError("no tty")), patch.object(bridge.subprocess, "Popen") as popen:
                with self.assertRaises((OSError, ValueError)):
                    bridge.run(["codex", "--chain", "user-notifier", "original-arg", payload])
                self.assertEqual(popen.call_args.args[0], ["user-notifier", "original-arg", payload])

    def test_partial_terminal_frame_is_cancelled_on_timeout(self):
        bridge = module("remote_bridge")
        writes = []
        def write(fd, value):
            writes.append(value)
            return 2 if len(writes) == 1 else len(value)
        with patch.object(bridge.os, "open", return_value=5), patch.object(bridge.os, "close"), patch.object(bridge.os, "write", write), patch.object(bridge.select, "select", side_effect=[([], [5], []), ([], [], []), ([], [5], [])]):
            with self.assertRaises(TimeoutError):
                bridge.write_terminal(b"\x1b]777;partial\x07")
        self.assertEqual(writes[-1], b"\x18")


if __name__ == "__main__":
    unittest.main()
