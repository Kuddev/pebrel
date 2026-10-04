"""Query and edit real Bash/zsh buffers through an owned PTY, without desktop input."""
from __future__ import annotations

import base64
import os
from pathlib import Path
import re
import unittest

from scripts.tests import test_shell_integration as shell_integration


@unittest.skipUnless(os.name == "posix", "requires Unix PTYs")
class CompletionEditorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = shell_integration.ShellIntegrationTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.adapter = Path(__file__).resolve().parents[2] / "nebula_terminal/src/tty/completion.sh"

    def start(self, shell: str, rc: str = "") -> shell_integration.ShellSession:
        session = self.fixture.start(shell, rc)
        quoted = str(self.adapter).replace("'", "'\\''")
        session.command(f"source '{quoted}'", b"\x1b]133;A\x07")
        return session

    def snapshot(self, session: shell_integration.ShellSession) -> tuple[str, int, str]:
        os.write(session.master, b"\x1b[45~")
        output = session.wait(b"\x1b]1337;SetUserVar=pebrel_editor=")
        match = re.search(rb"SetUserVar=pebrel_editor=([^\x07]+)\x07", output)
        if match is None:
            output += session.wait(b"\x07")
            match = re.search(rb"SetUserVar=pebrel_editor=([^\x07]+)\x07", output)
        self.assertIsNotNone(match, output)
        owner, unit, cursor, line = base64.b64decode(match.group(1)).decode().split("\n", 3)
        self.assertEqual(unit, "utf8")
        self.assertTrue(owner.startswith(("bash:", "zsh:")), owner)
        return owner, int(cursor), line

    def check_middle_edit(self, shell: str) -> None:
        session = self.start(shell)
        line = 'echo "中-old" tail'
        prefix = 'echo "中'
        os.write(session.master, line.encode() + b"\x1b[D" * len('-old" tail'))
        owner, cursor, actual = self.snapshot(session)
        self.assertEqual(actual, line)
        self.assertEqual(cursor, len(prefix.encode()))
        # The edit removes only the active word's right half and retains tail.
        os.write(session.master, b"\x1b[C" * len('-old"') + b"\x7f" * len('-old"') + '文😀"'.encode())
        next_owner, cursor, actual = self.snapshot(session)
        self.assertEqual(next_owner, owner)
        self.assertEqual(actual, 'echo "中文😀" tail')
        self.assertEqual(cursor, len('echo "中文😀"'.encode()))
        os.write(session.master, b"\n")
        output = session.wait("中文😀 tail\r\n".encode())
        self.assertIn("中文😀 tail\r\n".encode(), output)

    def test_bash_queries_utf8_caret_and_preserves_following_text(self) -> None:
        self.check_middle_edit("bash")

    def test_zsh_queries_utf8_caret_and_preserves_following_text(self) -> None:
        self.check_middle_edit("zsh")

    def check_user_binding(self, shell: str) -> None:
        rc = "bind '\"\\e[45~\": beginning-of-line'\n" if shell == "bash" else "bindkey '\\e[45~' beginning-of-line\n"
        session = self.start(shell, rc)
        os.write(session.master, b"echo tail\x1b[45~printf 'OWNED-BINDING\\n'; \n")
        output = session.wait(b"OWNED-BINDING\r\n")
        self.assertIn(b"OWNED-BINDING\r\n", output)
        self.assertNotIn(b"SetUserVar=pebrel_editor=", output)

    def test_bash_preserves_existing_f24_binding(self) -> None:
        self.check_user_binding("bash")

    def test_zsh_preserves_existing_f24_binding(self) -> None:
        self.check_user_binding("zsh")


if __name__ == "__main__":
    unittest.main()
