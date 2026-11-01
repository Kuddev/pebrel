"""Exercise the production PSReadLine adapter over a real Unix PTY."""
from __future__ import annotations

import base64
import os
from pathlib import Path
import re
import select
import shutil
import tempfile
import time
import unittest

from scripts.tests.test_shell_integration import ShellSession


class PowerShellSession(ShellSession):
    def consume(self, block: bytes) -> None:
        previous = self.output.count(b"\x1b[6n")
        self.output += block
        for _ in range(self.output.count(b"\x1b[6n") - previous):
            os.write(self.master, b"\x1b[1;5R")

    def settle(self, quiet: float = 0.3) -> None:
        # Complete Console.ReadKey's cursor-position handshake before sending keys.
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if not select.select([self.master], [], [], quiet)[0]:
                return
            block = os.read(self.master, 65536)
            self.consume(block)

    def wait(self, marker: bytes) -> bytes:
        collected = b""
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.1)[0]:
                block = os.read(self.master, 65536)
                collected += block
                self.consume(block)
                if marker in collected:
                    return collected
        raise AssertionError(f"Missing {marker!r}: {self.output[-4000:]!r}")


@unittest.skipUnless(os.name == "posix", "requires Unix PTYs")
class PowerShellPointerTests(unittest.TestCase):
    def start(self, mode: str = "Emacs", before: str = "", *, legacy: bool = False) -> PowerShellSession:
        program = os.environ.get("NEBULA_TEST_PWSH") or shutil.which("pwsh")
        if not program:
            self.skipTest("pwsh is not installed")
        module = os.environ.get("NEBULA_TEST_PSREADLINE_LEGACY") if legacy else None
        if legacy and not module:
            self.skipTest("NEBULA_TEST_PSREADLINE_LEGACY is not configured")
        import_module = "Import-Module " + ("'" + module.replace("'", "''") + "'" if module else "PSReadLine")
        if legacy:
            import_module += "\nif ((Get-Module PSReadLine).Version -ge [Version]'2.1.0') { throw 'Fixture requires a legacy editor' }"
        temporary = tempfile.TemporaryDirectory(prefix="pebrel-pwsh-pointer-")
        self.addCleanup(temporary.cleanup)
        home = Path(temporary.name)
        root = Path(__file__).resolve().parents[2] / "nebula_terminal/src/tty"
        script = home / "init.ps1"
        (home / "native-editor.ps1").write_text(
            (root / "completion.ps1").read_text(encoding="utf-8") + "\n" +
            (root / "pointer.ps1").read_text(encoding="utf-8"), encoding="utf-8",
        )
        bootstrap = home / "bootstrap.ps1"
        bootstrap.write_text((root.parents[2] / "nebula_app/res/shell/powershell.ps1").read_text(encoding="utf-8"), encoding="utf-8")
        script.write_text(
            import_module + "\n"
            "Set-PSReadLineOption -HistorySaveStyle SaveNothing\n"
            "if ((Get-Command Set-PSReadLineOption).Parameters.ContainsKey('PredictionSource')) { Set-PSReadLineOption -PredictionSource None }\n"
            f"Set-PSReadLineOption -EditMode {mode}\n"
            "$global:PebrelShellToken=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('pwsh:fixture'))\n"
            "function prompt { 'PS> ' }\n" + before + "\n" +
            f". '{bootstrap}'\n" +
            "$global:QaOriginalReadLine = (Get-Command PSConsoleHostReadLine).ScriptBlock\n"
            "function global:PSConsoleHostReadLine {\n"
            "  [Console]::Write(\"$([char]27)]1337;SetUserVar=qa_ready=eA==$([char]7)\")\n"
            "  & $global:QaOriginalReadLine\n"
            "}\n" + (
                # Bypass ConsoleHost's preloaded editor and run the imported
                # legacy module's real ReadLine on this PTY instead.
                "while ($true) { [Console]::Write((prompt)); $null = PSConsoleHostReadLine }\n"
                if legacy else ""
            ), encoding="utf-8",
        )
        # .NET's named mutexes (including PSReadLine history initialization)
        # require a real temporary directory on macOS even with SaveNothing.
        # Keep it inside this fixture instead of inheriting the runner's home.
        runtime_temp = home / "tmp"
        runtime_temp.mkdir()
        environment = {"TMPDIR": str(runtime_temp) + os.sep}
        session = PowerShellSession(
            program, home, ["-NoLogo", "-NoProfile", "-NonInteractive" if legacy else "-NoExit", "-File", str(script)],
            environment,
        )
        self.addCleanup(session.close)
        import fcntl
        import struct
        import termios
        fcntl.ioctl(session.master, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
        session.wait(b"SetUserVar=qa_ready=")
        session.settle(0.6)
        return session

    def value(self, session: PowerShellSession, name: str) -> list[str]:
        marker = f"SetUserVar={name}=".encode()
        output = session.wait(marker)
        pattern = re.escape(marker) + rb"([^\x07]+)\x07"
        match = re.search(pattern, output)
        if not match:
            output += session.wait(b"\x07")
            match = re.search(pattern, output)
        self.assertIsNotNone(match, output[-2000:])
        return base64.b64decode(match[1]).decode().split("\n", 3)

    def action(self, session: PowerShellSession, argument: int) -> None:
        session.settle()
        digits = b"".join(b"\x1b[21;6~" + digit.encode() for digit in str(argument))
        os.write(session.master, digits + b"\x1b[24;6~")

    def query(self, session: PowerShellSession, nonce: int = 20000) -> list[str]:
        self.action(session, nonce)
        return self.value(session, "pebrel_pointer")

    def snapshot(self, session: PowerShellSession) -> tuple[int, str]:
        session.settle()
        os.write(session.master, b"\x1b[24;6~")
        owner, unit, cursor, line = self.value(session, "pebrel_editor")
        self.assertEqual((owner, unit), ("pwsh:fixture", "utf16"))
        return int(cursor), line

    def test_native_unicode_moves_range_delete_and_one_undo(self) -> None:
        for mode in ("Emacs", "Windows"):
            with self.subTest(mode=mode):
                session = self.start(mode)
                line = "echo 中文é👩‍💻 tail"
                os.write(session.master, line.encode())
                self.assertEqual(self.query(session), ["pwsh:fixture", "utf8:20000", str(len(line.encode())), line])
                self.action(session, 1 + 6)
                self.assertEqual(self.snapshot(session), (6, line))
                os.write(session.master, b"X")
                changed = "echo 中X文é👩‍💻 tail"
                self.query(session)
                self.action(session, 1 + 13)
                self.action(session, 4098 + 5)
                self.action(session, 8195 + 13)
                self.assertEqual(self.snapshot(session), (5, "echo  tail"))
                os.write(session.master, b"\x18\x15" if mode == "Emacs" else b"\x1a")
                self.assertEqual(self.snapshot(session)[1], changed)

    def test_legacy_editor_moves_ascii_and_cjk_and_preserves_undo(self) -> None:
        for mode in ("Emacs", "Windows"):
            with self.subTest(mode=mode):
                session = self.start(mode, legacy=True)
                self.assertIn(b"SetUserVar=pebrel_pointer_ready=", session.output)
                os.write(session.master, "ls 中文".encode())
                self.assertEqual(self.query(session)[3], "ls 中文")
                self.action(session, 1 + 1)
                self.assertEqual(self.snapshot(session), (1, "ls 中文"))
                os.write(session.master, b"X")
                self.assertEqual(self.snapshot(session), (2, "lXs 中文"))
                self.query(session)
                self.action(session, 4098 + 4)
                self.action(session, 8195 + 6)
                self.assertEqual(self.snapshot(session), (4, "lXs "))
                os.write(session.master, b"\x18\x15" if mode == "Emacs" else b"\x1a")
                self.assertEqual(self.snapshot(session)[1], "lXs 中文")

    def test_legacy_editor_rejects_surrogates_without_moving_or_deleting(self) -> None:
        session = self.start("Windows", before=(
            "Set-PSReadLineKeyHandler -Chord Ctrl+o -ScriptBlock { "
            "[Microsoft.PowerShell.PSConsoleReadLine]::Insert('echo ' + [char]::ConvertFromUtf32(0x1F431)) }"
        ), legacy=True)
        os.write(session.master, b"\x0f")
        before = self.snapshot(session)
        self.assertEqual(before[1], "echo 🐱")
        self.assertEqual(self.query(session), ["pwsh:fixture", "unsupported:20000"])
        self.action(session, 1)
        self.action(session, 4098)
        self.action(session, 8195 + 6)
        self.assertEqual(self.snapshot(session), before)

    def test_native_races_bounds_and_replay(self) -> None:
        session = self.start()
        os.write(session.master, b"echo abc")
        self.query(session)
        self.action(session, 4097)
        self.assertEqual(self.snapshot(session), (8, "echo abc"))
        os.write(session.master, b"\x1b[D")
        self.action(session, 1)
        self.assertEqual(self.snapshot(session), (7, "echo abc"))
        self.query(session, 999999)
        self.action(session, 4098 + 5)
        os.write(session.master, b"X")
        self.action(session, 8195 + 8)
        self.assertEqual(self.snapshot(session), (8, "echo abXc"))
        self.query(session)
        self.action(session, 4098 + 9)
        self.action(session, 8195 + 5)
        self.assertEqual(self.snapshot(session), (5, "echo "))
        self.action(session, 8195)
        self.assertEqual(self.snapshot(session)[1], "echo ")

    def test_vi_and_user_binding_ownership_are_not_advertised(self) -> None:
        for mode, before in [
            ("Vi", ""),
            ("Emacs", "Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F12 -Function BeginningOfLine"),
            ("Windows", "Set-PSReadLineKeyHandler -Chord 'Ctrl+Shift+F10,1' -Function BeginningOfLine"),
            ("Emacs", "Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F10 -Function BeginningOfLine"),
        ]:
            with self.subTest(mode=mode, before=before):
                session = self.start(mode, before)
                self.assertNotIn(b"SetUserVar=pebrel_pointer_ready=", session.output)

    def test_pointer_chords_preserve_the_clipboard_key(self) -> None:
        session = self.start(before="Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F11 -Function BeginningOfLine")
        os.write(session.master, b"echo abc")
        self.assertEqual(self.query(session)[3], "echo abc")
        self.action(session, 1 + 5)
        self.assertEqual(self.snapshot(session), (5, "echo abc"))
        os.write(session.master, b"\x1b[23;6~")
        self.assertEqual(self.snapshot(session), (0, "echo abc"))

    def test_production_bootstrap_preserves_native_prompt_and_command_lifecycle(self) -> None:
        session = self.start(before="function prompt { if ($?) { 'native-ok> ' } else { 'native-failed> ' } }")
        self.assertIn(b"native-ok> ", session.output)
        self.assertIn(b"\x1b]133;A\x07native-ok> ", session.output)
        self.assertIn(b"\x1b]133;B\x07", session.output)
        os.write(session.master, b"\x0c")
        redrawn = session.wait(b"native-ok> ")
        session.settle()
        self.assertIn(b"SetUserVar=pebrel_pointer_ready=", redrawn)
        self.assertIn(b"native-ok> \x1b]133;B\x07", redrawn)
        os.write(session.master, b"Write-Error fixture\r")
        session.wait(b"native-failed> ")
        session.settle()
        self.assertIn(b"\x1b]133;D;1\x07", session.output)
        self.assertIn(b"SetUserVar=pebrel_input=", session.output)
        self.assertIn(b"\x1b]133;C\x07", session.output)
        # Re-source the production script; the native prompt/editor must not recurse.
        os.write(session.master, b". './bootstrap.ps1'\r")
        session.wait(b"SetUserVar=qa_ready=")
        session.settle()
        os.write(session.master, b"echo abc")
        self.assertEqual(self.query(session)[3], "echo abc")

    def test_fragment_races_overflow_and_buffer_limit_fail_closed(self) -> None:
        session = self.start(before="Set-PSReadLineKeyHandler -Chord Ctrl+o -ScriptBlock { [Microsoft.PowerShell.PSConsoleReadLine]::Insert(('a' * 4097)) }")
        os.write(session.master, b"echo abc")
        self.query(session)
        # A native edit between argument fragments must never move the old caret.
        os.write(session.master, b"\x1b[21;6~1X\x1b[24;6~")
        self.assertEqual(self.snapshot(session), (9, "echo abcX"))
        self.query(session)
        self.action(session, 1000000)
        self.assertEqual(self.snapshot(session), (9, "echo abcX"))
        os.write(session.master, b"\x0f")
        self.assertEqual(self.query(session)[:2], ["pwsh:fixture", "unsupported:20000"])


if __name__ == "__main__":
    unittest.main()
