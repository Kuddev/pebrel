"""Native hook delivery through a real detached Linux process and private PTY."""
import errno
import io
import json
import os
from pathlib import Path
import select
import signal
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

if sys.platform == "linux":
    import fcntl
    import pty
    import termios

ROOT = Path(__file__).resolve().parents[2]
bridge = types.ModuleType("remote_bridge")
if sys.platform == "linux":
    bridge_path = ROOT / "nebula_app/res/hooks/remote_bridge.py"
    exec(compile(bridge_path.read_text(), str(bridge_path), "exec"), bridge.__dict__)


def birth(pid):
    row = Path(f"/proc/{pid}/stat").read_text()
    return int(row[row.rfind(")") + 2:].split()[19])


def private_pty_case(case, provider="codex"):
    with tempfile.TemporaryDirectory(prefix="codex-hook-pty-") as temp:
        home = Path(temp)
        (home / ".local/bin").mkdir(parents=True)
        if case in ("npm_install", "wrong_npm_exe"):
            (home / ".npm-global/bin").mkdir(parents=True)
            (home / ".npm-global/bin" / provider).symlink_to(
                "/usr/bin/true" if case == "wrong_npm_exe" else sys.executable
            )
        else:
            (home / ".local/bin" / provider).symlink_to("/usr/bin/true" if case == "wrong_exe" else sys.executable)
        master, slave = pty.openpty()
        read_pipe, write_pipe = os.pipe()
        owner_pid = os.fork()
        if owner_pid == 0:
            try:
                os.close(master)
                os.close(read_pipe)
                os.setsid()
                if case != "owner_no_tty":
                    fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
                for std in (0, 1, 2):
                    os.dup2(slave, std)
                os.close(slave)
                owner = f"{os.getpid()}:{birth(os.getpid())}"
                hook_pid = os.fork()
                if hook_pid == 0:
                    try:
                        os.setsid()
                        with patch.object(bridge.Path, "home", return_value=home):
                            source = "pi" if case == "other_provider" else provider
                            if case == "stale_birth":
                                owner = f"{os.getppid()}:{birth(os.getppid()) + 1}"
                            if case == "not_ancestor":
                                owner = f"{os.getpid()}:{birth(os.getpid())}"
                            if case == "changed_snapshot":
                                original = bridge._proc_snapshot
                                calls = 0
                                def changed(fd):
                                    nonlocal calls
                                    calls += 1
                                    row = original(fd)
                                    return (*row[:3], row[3] + 1) if calls > 1 else row
                                bridge._proc_snapshot = changed
                            if case == "fd0_denied":
                                original_open = bridge.os.open
                                def fail_fd0(path, *args, **kwargs):
                                    if path == "fd/0":
                                        raise PermissionError("synthetic fd0 denial")
                                    return original_open(path, *args, **kwargs)
                                bridge.os.open = fail_fd0
                            if case == "wrong_uid":
                                uid = os.getuid()
                                bridge.os.getuid = lambda: uid + 1
                            if case == "changed_exe":
                                original_stat = bridge.os.stat
                                calls = 0
                                def changed_exe(path, *args, **kwargs):
                                    nonlocal calls
                                    if path == "exe":
                                        calls += 1
                                        if calls > 1:
                                            return original_stat("/usr/bin/true")
                                    return original_stat(path, *args, **kwargs)
                                bridge.os.stat = changed_exe
                            if case == "send":
                                with patch("sys.stdout", new_callable=io.StringIO), patch.dict(
                                    os.environ, {"XDG_CACHE_HOME": str(home / ".cache")}
                                ):
                                    bridge.send(
                                        source, False, "", {"hook_event_name": "Stop"},
                                        "isolated-test-token", owner=owner,
                                    )
                                accepted = True
                            elif case == "write_terminal":
                                # Hook stdout is consumed by the provider, not
                                # relayed to the terminal as ordinary output.
                                with patch("sys.stdout", new_callable=io.StringIO) as hook_stdout:
                                    bridge.write_terminal(b"PRIVATE-PTY-MARKER", source, owner=owner)
                                accepted = True
                            else:
                                fd = bridge.owner_tty(source, owner)
                                accepted = fd is not None
                                if fd is not None:
                                    os.write(fd, b"PRIVATE-PTY-MARKER")
                                    os.close(fd)
                            try:
                                tty = os.open("/dev/tty", os.O_WRONLY | os.O_NOCTTY)
                            except OSError as error:
                                detached = error.errno == errno.ENXIO
                            else:
                                os.close(tty)
                                detached = False
                        os.write(write_pipe, json.dumps({"accepted": accepted, "still_detached": detached}).encode())
                    except BaseException as error:
                        os.write(write_pipe, json.dumps({"error": repr(error)}).encode())
                    os._exit(0)
                os.close(write_pipe)
                os.waitpid(hook_pid, 0)
            finally:
                os._exit(0)
        os.close(write_pipe)
        os.close(slave)
        try:
            if not select.select([read_pipe], [], [], 5)[0]:
                os.kill(owner_pid, signal.SIGKILL)
                raise TimeoutError("private PTY test")
            result = json.loads(os.read(read_pipe, 65536))
            os.waitpid(owner_pid, 0)
            output = b""
            while select.select([master], [], [], 0)[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not chunk:
                    break
                output += chunk
            return result, output
        finally:
            os.close(master)
            os.close(read_pipe)


@unittest.skipUnless(sys.platform == "linux", "Linux procfs ownership contract")
class NativeOwnerTtyTests(unittest.TestCase):
    def test_detached_native_claude_uses_owner_pty(self):
        result, output = private_pty_case("send", "claude")
        self.assertNotIn("error", result)
        self.assertTrue(result["still_detached"])
        self.assertIn(b"\x1b]777;nebula-hook;isolated-test-token;", output)

    def test_claude_pinned_owner_routing_and_rejections(self):
        cases = {
            "valid": True, "write_terminal": True, "fd0_denied": True,
            "npm_install": True, "wrong_npm_exe": False,
            "stale_birth": False, "not_ancestor": False, "wrong_exe": False,
            "owner_no_tty": False, "changed_snapshot": False,
            "wrong_uid": False, "changed_exe": False,
        }
        for case, expected in cases.items():
            with self.subTest(case=case):
                result, output = private_pty_case(case, "claude")
                self.assertNotIn("error", result)
                self.assertEqual(result["accepted"], expected)
                self.assertTrue(result["still_detached"])
                self.assertEqual(b"PRIVATE-PTY-MARKER" in output, expected)
                print(f"PASS Claude private PTY: {case}")

    def test_private_pty_routing_and_rejections(self):
        cases = {
            "valid": True, "write_terminal": True, "fd0_denied": True,
            "other_provider": False, "stale_birth": False, "not_ancestor": False,
            "wrong_exe": False, "owner_no_tty": False, "changed_snapshot": False,
            "wrong_uid": False,
            "changed_exe": False,
        }
        for case, expected in cases.items():
            with self.subTest(case=case):
                result, output = private_pty_case(case)
                self.assertNotIn("error", result)
                self.assertEqual(result["accepted"], expected)
                self.assertTrue(result["still_detached"])
                self.assertEqual(b"PRIVATE-PTY-MARKER" in output, expected)
                print(f"PASS private PTY: {case}")

    def test_claude_stdout_fallback_preserved(self):
        with patch.object(bridge.os, "open", side_effect=OSError(errno.ENXIO, "no tty")), patch("sys.stdout", new_callable=io.StringIO) as output:
            bridge.write_terminal(b"test-sequence", "claude")
        self.assertEqual(json.loads(output.getvalue()), {"terminalSequence": "test-sequence"})

    def test_other_provider_does_not_fallback(self):
        with patch.object(bridge.os, "open", side_effect=OSError(errno.ENXIO, "no tty")), patch.object(bridge, "owner_tty") as fallback:
            with self.assertRaises(OSError):
                bridge.write_terminal(b"test", "opencode")
            fallback.assert_not_called()

    def test_explicit_route_does_not_fallback(self):
        with patch.object(bridge.os, "open", side_effect=OSError(errno.ENXIO, "no tty")), patch.object(bridge, "owner_tty") as fallback:
            with self.assertRaises(OSError):
                bridge.write_terminal(b"test", "codex", route={"tty": "/dev/pts/999999"})
            fallback.assert_not_called()

    def test_missing_owner_rejected(self):
        self.assertIsNone(bridge.owner_tty("codex", None))


if __name__ == "__main__":
    unittest.main(verbosity=2)
