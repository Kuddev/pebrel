"""Query and edit real Bash/zsh buffers through an owned PTY, without desktop input."""
from __future__ import annotations

import base64
import os
from pathlib import Path
import re
import shutil
import subprocess
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
        self.assertTrue(owner.startswith(("bash:", "zsh:", "wsl|")), owner)
        return owner, int(cursor), line

    def start_bootstrap(self, shell: str, route: str, owned_binding: bool = False) -> shell_integration.ShellSession:
        root = Path(__file__).resolve().parents[2]
        program = shutil.which(shell)
        if not program:
            self.skipTest(f"{shell} is not installed")
        adapter = "\n".join((root / "nebula_terminal/src/tty" / name).read_text()
                            for name in ("connection.sh", "completion.sh"))
        if route == "wsl":
            source = (root / "nebula_app/src/shell_detect.rs").read_text()
            report = re.search(r'const REPORT: &str = r#"(.*?)"#;', source, re.S)
            self.assertIsNotNone(report)
            prompt = report.group(1).replace("__PEBREL_CONNECTION_HOOK__", adapter)
            args = ["--noprofile", "--norc", "-i"]
            env = {"PROMPT_COMMAND": prompt, "WSL_DISTRO_NAME": "completion-fixture"}
        else:
            source = (root / "nebula_app/src/ssh.rs").read_text()
            name = "REMOTE_BASH" if shell == "bash" else "REMOTE_ZSH"
            body = re.search(rf'const {name}: &str = r#"(.*?)"#;', source, re.S)
            self.assertIsNotNone(body)
            directory = self.fixture.home / "remote-integration"
            directory.mkdir()
            rc = directory / ("bashrc" if shell == "bash" else ".zshrc")
            rc.write_text(body.group(1) + "\n" + adapter)
            args = ["--noprofile", "--rcfile", str(rc), "-i"] if shell == "bash" else ["-d", "-i"]
            env = {} if shell == "bash" else {"ZDOTDIR": str(directory)}
        if owned_binding:
            if shell == "bash":
                inputrc = self.fixture.home / ".inputrc"
                inputrc.write_text('"\\e[45~": beginning-of-line\n')
                env["INPUTRC"] = str(inputrc)
            else:
                (self.fixture.home / ".zshrc").write_text("bindkey '\\e[45~' beginning-of-line\n")
        session = shell_integration.ShellSession(program, self.fixture.home, args, env)
        self.addCleanup(session.close)
        session.wait(b"\x1b]133;A\x07")
        return session

    def check_bootstrap(self, shell: str, route: str) -> None:
        session = self.start_bootstrap(shell, route)
        ready = re.search(rb"SetUserVar=pebrel_editor_ready=([^\x07]+)\x07", session.output)
        if shell == "bash":
            version = subprocess.check_output(
                [shutil.which(shell), "--noprofile", "--norc", "-c", 'printf "%s" "${BASH_VERSINFO[0]}"'],
                text=True,
            )
            if int(version) < 4:
                self.assertIsNone(ready, "legacy Bash must retain native completion")
                output = session.command("printf 'LEGACY-EXEC\\n'", b"LEGACY-EXEC\r\n")
                self.assertIn(b"LEGACY-EXEC\r\n", output)
                return
        self.assertIsNotNone(ready, session.output)
        owner = base64.b64decode(ready.group(1)).decode()
        line = 'echo "中-old" tail'
        os.write(session.master, line.encode() + b"\x1b[D" * len('-old" tail'))
        reported_owner, cursor, actual = self.snapshot(session)
        self.assertEqual(reported_owner, owner)
        self.assertEqual(actual, line)
        self.assertEqual(cursor, len('echo "中'.encode()))
        os.write(session.master, b"\x1b[C" * len('-old"') + b"\x7f" * len('-old"') + '文😀"'.encode())
        _, _, actual = self.snapshot(session)
        self.assertEqual(actual, 'echo "中文😀" tail')
        os.write(session.master, b"\n")
        output = session.wait("中文😀 tail\r\n".encode())
        if b"\x1b]133;A\x07" not in output:
            output += session.wait(b"\x1b]133;A\x07")
        next_ready = re.search(rb"SetUserVar=pebrel_editor_ready=([^\x07]+)\x07", output)
        self.assertIsNotNone(next_ready, output)
        self.assertEqual(base64.b64decode(next_ready.group(1)).decode(), owner)

    def test_wsl_prompt_bootstrap_advertises_and_queries_the_native_editor(self) -> None:
        self.check_bootstrap("bash", "wsl")

    def test_ssh_bash_bootstrap_advertises_and_queries_the_native_editor(self) -> None:
        self.check_bootstrap("bash", "ssh")

    def test_ssh_zsh_bootstrap_advertises_and_queries_the_native_editor(self) -> None:
        self.check_bootstrap("zsh", "ssh")

    def check_bootstrap_binding(self, shell: str, route: str) -> None:
        session = self.start_bootstrap(shell, route, owned_binding=True)
        self.assertNotIn(b"SetUserVar=pebrel_editor_ready=", session.output)
        os.write(session.master, b"echo tail\x1b[45~printf 'OWNED-BINDING\\n'; \n")
        output = session.wait(b"OWNED-BINDING\r\n")
        self.assertIn(b"OWNED-BINDING\r\n", output)
        self.assertNotIn(b"SetUserVar=pebrel_editor=", output)

    def test_wsl_prompt_bootstrap_retains_user_owned_query_binding(self) -> None:
        self.check_bootstrap_binding("bash", "wsl")

    def test_ssh_bash_bootstrap_retains_user_owned_query_binding(self) -> None:
        self.check_bootstrap_binding("bash", "ssh")

    def test_ssh_zsh_bootstrap_retains_user_owned_query_binding(self) -> None:
        self.check_bootstrap_binding("zsh", "ssh")

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
