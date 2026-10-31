"""Native Bash/Readline and ZLE pointer edits, including editor-side race guards."""
from __future__ import annotations

import base64
import os
from pathlib import Path
import re
import shlex
import unittest

from scripts.tests import test_completion_editor


@unittest.skipUnless(os.name == "posix", "requires Unix PTYs")
class ShellPointerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.editor = test_completion_editor.CompletionEditorTests()
        self.editor.setUp()
        self.addCleanup(self.editor.doCleanups)

    def start(self, shell: str, setup: str = ""):
        session = self.editor.start(shell, setup)
        adapter = Path(__file__).resolve().parents[2] / "nebula_terminal/src/tty/pointer.sh"
        session.command(f"source {shlex.quote(str(adapter))}", b"\x1b]133;A\x07")
        return session

    def action(self, session, argument: int) -> None:
        encoded = b"".join(b"\x1b" + digit.encode() for digit in str(argument))
        os.write(session.master, encoded + b"\x1b[45~")

    def query(self, session, nonce: int = 0) -> tuple[int, str]:
        self.action(session, nonce)
        output = session.wait(b"SetUserVar=pebrel_pointer=")
        match = re.search(rb"SetUserVar=pebrel_pointer=([^\x07]+)\x07", output)
        if match is None:
            output += session.wait(b"\x07")
            match = re.search(rb"SetUserVar=pebrel_pointer=([^\x07]+)\x07", output)
        self.assertIsNotNone(match, output)
        owner, unit, cursor, line = base64.b64decode(match[1]).decode().split("\n", 3)
        self.assertTrue(owner.startswith(("bash:", "zsh:")), owner)
        self.assertEqual(unit, f"utf8:{nonce}" if nonce else "utf8")
        return int(cursor), line

    def check_edit(self, shell: str) -> None:
        session = self.start(shell)
        line = "echo 中文é👩‍💻 tail"
        os.write(session.master, line.encode())
        self.assertEqual(self.query(session), (len(line.encode()), line))
        self.action(session, 1 + 6)
        self.assertEqual(self.editor.snapshot(session)[1:], (len("echo 中".encode()), line))
        os.write(session.master, b"X")
        changed = "echo 中X文é👩‍💻 tail"
        self.assertEqual(self.query(session), (len("echo 中X".encode()), changed))
        self.action(session, 1 + 13)
        self.action(session, 4098 + 5)
        self.action(session, 8195 + 13)
        self.assertEqual(self.editor.snapshot(session)[1:], (5, "echo  tail"))
        os.write(session.master, b"\x1f")
        self.assertEqual(self.editor.snapshot(session)[2], changed, "one native undo restores the range")

    def test_bash_moves_and_deletes_unicode_range_with_one_undo(self) -> None:
        self.check_edit("bash")

    def test_zsh_moves_and_deletes_unicode_range_with_one_undo(self) -> None:
        self.check_edit("zsh")

    def check_stale(self, shell: str) -> None:
        session = self.start(shell)
        os.write(session.master, b"echo abc")
        self.query(session)
        os.write(session.master, b"\x1b[D")
        self.action(session, 1 + 1)
        self.assertEqual(self.editor.snapshot(session)[1:], (7, "echo abc"))
        self.query(session)
        self.action(session, 4098 + 5)
        os.write(session.master, b"X")
        self.action(session, 8195 + 8)
        self.assertEqual(self.editor.snapshot(session)[1:], (8, "echo abXc"))
        self.query(session)
        self.action(session, 4098 + 5)
        os.write(session.master, b"\x1b[D")
        self.action(session, 8195 + 8)
        self.assertEqual(self.editor.snapshot(session)[1:], (7, "echo abXc"))

    def test_bash_rechecks_native_buffer_and_cursor_before_every_operation(self) -> None:
        self.check_stale("bash")

    def test_zsh_rechecks_native_buffer_and_cursor_before_every_operation(self) -> None:
        self.check_stale("zsh")

    def check_reverse_and_bounds(self, shell: str) -> None:
        session = self.start(shell)
        os.write(session.master, b"echo abc")
        self.query(session)
        self.action(session, 1 + 4096)
        self.assertEqual(self.editor.snapshot(session)[1:], (8, "echo abc"))
        self.action(session, 4098 + 8)
        self.action(session, 8195 + 5)
        self.assertEqual(self.editor.snapshot(session)[1:], (5, "echo "))
        self.action(session, 8195 + 0)
        self.assertEqual(self.editor.snapshot(session)[2], "echo ", "an edit cannot be replayed")

    def test_bash_reverse_range_and_invalid_coordinates(self) -> None:
        self.check_reverse_and_bounds("bash")

    def test_zsh_reverse_range_and_invalid_coordinates(self) -> None:
        self.check_reverse_and_bounds("zsh")

    def test_numbered_queries_echo_nonce_without_inserting_digits(self) -> None:
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                session = self.start(shell)
                os.write(session.master, "echo 中文".encode())
                for nonce in (20000, 20001, 999999):
                    self.assertEqual(self.query(session, nonce), (11, "echo 中文"))
                self.action(session, 1 + 5)
                self.assertEqual(self.editor.snapshot(session)[1:], (5, "echo 中文"))

    def check_prompt_boundary(self, shell: str) -> None:
        setup = "PROMPT_COMMAND='PS1=\"dynamic> \"'\n" if shell == "bash" else (
            "user_prompt() { PS1='dynamic> '; }; precmd_functions=(user_prompt)\n"
        )
        session = self.start(shell, setup)
        boundary = b"dynamic> \x1b]133;B\x07"
        if boundary not in session.output:
            session.wait(boundary)
        # Repeated prompt changes and re-sourcing must retain exactly one
        # boundary, after all printable user prompt cells.
        adapter = Path(__file__).resolve().parents[2] / "nebula_terminal/src/tty/pointer.sh"
        session.command(f"source {shlex.quote(str(adapter))}", boundary)
        output = session.command(":", boundary)
        self.assertEqual(output.count(b"\x1b]133;B\x07"), 1, output)
        os.write(session.master, "echo 中文".encode())
        self.assertEqual(self.query(session), (11, "echo 中文"))
        self.action(session, 1 + 5)
        self.assertEqual(self.editor.snapshot(session)[1:], (5, "echo 中文"))

    def test_bash_marks_input_after_dynamic_prompt_without_duplicate_markers(self) -> None:
        self.check_prompt_boundary("bash")

    def test_zsh_marks_input_after_dynamic_prompt_without_duplicate_hooks(self) -> None:
        self.check_prompt_boundary("zsh")

    def check_binding(self, shell: str, vi: bool = False) -> None:
        if vi:
            setup = "set -o vi\n" if shell == "bash" else "bindkey -v\n"
        else:
            setup = "bind '\"\\e6\": beginning-of-line'\n" if shell == "bash" else "bindkey '\\e6' beginning-of-line\n"
        session = self.start(shell, setup)
        self.assertNotIn(b"SetUserVar=pebrel_pointer_ready=", session.output)

    def test_bash_preserves_user_bindings(self) -> None:
        self.check_binding("bash")

    def test_bash_does_not_advertise_vi_keymaps(self) -> None:
        self.check_binding("bash", vi=True)

    def test_zsh_preserves_user_bindings(self) -> None:
        self.check_binding("zsh")

    def test_zsh_does_not_advertise_vi_keymaps(self) -> None:
        self.check_binding("zsh", vi=True)


if __name__ == "__main__":
    unittest.main()
