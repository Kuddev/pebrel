"""Query and edit real Bash/zsh buffers through an owned PTY, without desktop input."""
from __future__ import annotations

import base64
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
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

    def start_bootstrap(self, shell: str, route: str, owned_binding: bool = False,
                        owned_redraw: bool = False, owned_macro: bool = False,
                        vicmd_widget: bool = False) -> shell_integration.ShellSession:
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
        if owned_binding or owned_redraw or owned_macro or vicmd_widget:
            if shell == "bash":
                inputrc = self.fixture.home / ".inputrc"
                lines = ""
                if owned_binding:
                    lines += '"\\e[45~": beginning-of-line\n'
                if owned_redraw:
                    lines += '"\\e[46~": beginning-of-line\n'
                if owned_macro:
                    # A string macro, which bind -p and bind -X do not list.
                    lines += '"\\e[46~": "PEBREL-USER-MACRO"\n'
                inputrc.write_text(lines)
                env["INPUTRC"] = str(inputrc)
            else:
                zshrc = ""
                if owned_binding:
                    zshrc += "bindkey '\\e[45~' beginning-of-line\n"
                if owned_redraw:
                    zshrc += "bindkey '\\e[46~' beginning-of-line\n"
                if owned_macro:
                    zshrc += "bindkey -s '\\e[46~' 'PEBREL-USER-MACRO'\n"
                if vicmd_widget:
                    # vi mode: the chord is free in viins but carries a user
                    # widget in vicmd, reachable mid-prompt via ESC.
                    zshrc += (
                        "bindkey -v\n"
                        "userwid() { BUFFER+='VICMD-USER' }; zle -N userwid\n"
                        "bindkey -M vicmd '\\e[46~' userwid\n"
                    )
                (self.fixture.home / ".zshrc").write_text(zshrc)
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

    def check_integration_clear(self, shell: str, route: str) -> None:
        session = self.start_bootstrap(shell, route)
        ready = re.search(rb"SetUserVar=pebrel_editor_ready=([^\x07]+)\x07", session.output)
        if shell == "bash":
            version = subprocess.check_output(
                [shutil.which(shell), "--noprofile", "--norc", "-c", 'printf "%s" "${BASH_VERSINFO[0]}"'],
                text=True,
            )
            if int(version) < 4:
                # Legacy Bash never installs the channel, so nothing owns the
                # redraw key either: the byte stays inert for the host to decide.
                self.assertIsNone(ready, session.output)
                return
        self.assertIsNotNone(ready, session.output)
        # The redraw key drives the line editor's own clear-screen: the editor
        # wipes the glass and repaints prompt, buffer and caret.
        os.write(session.master, b"echo kept-line")
        os.write(session.master, b"\x1b[46~")
        output = session.wait(b"\x1b[2J")
        self.assertIn(b"\x1b[2J", output)
        os.write(session.master, b"\n")
        output = session.wait(b"kept-line\r\n")
        self.assertIn(b"kept-line\r\n", output)
        # While the integration still owns the key, every prompt keeps
        # announcing the redraw capability with the same token.
        sustained = session.command("printf 'SUSTAINED\\n'", b"SUSTAINED\r\n")
        self.assertIn(b"SetUserVar=pebrel_editor_redraw=", sustained)
        self.assertNotIn(b"SetUserVar=pebrel_editor_redraw=\x07", sustained)

    def test_ssh_bash_bootstrap_redraws_through_the_owned_clear_key(self) -> None:
        self.check_integration_clear("bash", "ssh")

    def test_ssh_zsh_bootstrap_redraws_through_the_owned_clear_key(self) -> None:
        self.check_integration_clear("zsh", "ssh")

    def check_legacy_bash(self, shell: str, session: shell_integration.ShellSession) -> bool:
        if shell == "bash":
            version = subprocess.check_output(
                [shutil.which(shell), "--noprofile", "--norc", "-c", 'printf "%s" "${BASH_VERSINFO[0]}"'],
                text=True,
            )
            if int(version) < 4:
                # Legacy Bash installs neither capability and binds no key.
                self.assertNotIn(b"SetUserVar=pebrel_editor_ready=", session.output)
                self.assertNotIn(b"SetUserVar=pebrel_editor_redraw=", session.output)
                return True
        return False

    def check_owned_redraw_binding(self, shell: str, route: str) -> None:
        session = self.start_bootstrap(shell, route, owned_redraw=True)
        if self.check_legacy_bash(shell, session):
            return
        # A user-owned redraw key only withdraws the redraw capability: the
        # query capability keeps reporting, matching the pre-redraw releases.
        self.assertIn(b"SetUserVar=pebrel_editor_ready=", session.output)
        self.assertNotIn(b"SetUserVar=pebrel_editor_redraw=", session.output)
        os.write(session.master, b"echo tail\x1b[46~printf 'OWNED-REDRAW\\n'; \n")
        output = session.wait(b"OWNED-REDRAW\r\n")
        self.assertIn(b"OWNED-REDRAW\r\n", output)
        self.assertNotIn(b"SetUserVar=pebrel_editor=", output)
        # The user's function binding survived; the reserved chord itself was
        # never replaced by clear-screen (the table still lists the user's
        # normal Ctrl+L entry, so only the reserved chord may be inspected).
        if shell == "bash":
            bindings = session.command(
                "bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null; printf 'BINDINGS-END\\n'",
                b"BINDINGS-END\r\n",
            )
            self.assertIn(b'"\\e[46~": beginning-of-line', bindings)
            self.assertNotIn(b'"\\e[46~": clear-screen', bindings)

    def test_ssh_bash_bootstrap_retains_user_owned_redraw_binding(self) -> None:
        self.check_owned_redraw_binding("bash", "ssh")

    def test_ssh_zsh_bootstrap_retains_user_owned_redraw_binding(self) -> None:
        self.check_owned_redraw_binding("zsh", "ssh")

    def check_owned_redraw_macro(self, shell: str, route: str) -> None:
        session = self.start_bootstrap(shell, route, owned_macro=True)
        if self.check_legacy_bash(shell, session):
            return
        # A string macro on the redraw key is invisible to bind -p and bind -X
        # in Bash; the probe must still treat the key as user-owned and leave
        # the macro working, with the query capability unaffected.
        self.assertIn(b"SetUserVar=pebrel_editor_ready=", session.output)
        self.assertNotIn(b"SetUserVar=pebrel_editor_redraw=", session.output)
        os.write(session.master, b"echo tail\x1b[46~")
        _, _, line = self.snapshot(session)
        self.assertEqual(line, "echo tailPEBREL-USER-MACRO")
        if shell == "bash":
            bindings = session.command(
                "bind -s 2>/dev/null; printf 'BINDINGS-END\\n'",
                b"BINDINGS-END\r\n",
            )
            self.assertIn(b'PEBREL-USER-MACRO', bindings)
            self.assertNotIn(b'"\\e[46~": clear-screen', bindings)

    def test_ssh_bash_bootstrap_retains_user_owned_redraw_macro(self) -> None:
        self.check_owned_redraw_macro("bash", "ssh")

    def test_ssh_zsh_bootstrap_retains_user_owned_redraw_macro(self) -> None:
        self.check_owned_redraw_macro("zsh", "ssh")

    def check_query_only_session(self, shell: str, route: str) -> None:
        # The pre-redraw integrations advertised query readiness only and left
        # the redraw key unbound: the current installation must not touch the
        # key at all in such a session.
        session = self.start_bootstrap(shell, route)
        if self.check_legacy_bash(shell, session):
            return
        ready_token = re.search(rb"SetUserVar=pebrel_editor_ready=([^\x07]+)\x07", session.output)
        self.assertIsNotNone(ready_token, session.output)
        # Withdraw only the redraw binding this invocation installed, which is
        # exactly the state a query-only integration advertises.
        session.command(
            "bind -r '\\e[46~' 2>/dev/null; bindkey -r '\\e[46~' 2>/dev/null; printf 'WITHDRAWN\\n'",
            b"WITHDRAWN\r\n",
        )
        # The next prompt re-verifies ownership and revokes the capability with
        # an empty value while query readiness keeps its token.
        output = session.command("printf 'REPROBE\\n'", b"REPROBE\r\n")
        self.assertIn(b"SetUserVar=pebrel_editor_redraw=\x07", output)
        self.assertIn(ready_token.group(0), output)
        if shell == "bash":
            bindings = session.command(
                "bind -p 2>/dev/null; bind -X 2>/dev/null; bind -s 2>/dev/null; printf 'BINDINGS-END\\n'",
                b"BINDINGS-END\r\n",
            )
            self.assertNotIn(b'"\\e[46~"', bindings)
            self.assertIn(b'"\\e[45~"', bindings)
        else:
            output = session.command("bindkey '\\e[46~'; printf 'UNBOUND-CHECK\\n'", b"UNBOUND-CHECK\r\n")
            self.assertIn(b"undefined-key", output)

    def test_ssh_bash_bootstrap_query_only_session_leaves_redraw_unbound(self) -> None:
        self.check_query_only_session("bash", "ssh")

    def test_ssh_zsh_bootstrap_query_only_session_leaves_redraw_unbound(self) -> None:
        self.check_query_only_session("zsh", "ssh")

    def check_redraw_rebind(self, shell: str, route: str) -> None:
        # The advertisement follows the live binding: rebinding the chord in
        # the same shell — without reconnecting — revokes the capability on
        # the next prompt, and the user's new binding keeps firing.
        # zsh additionally covers the two lookalike rebinds from review: a
        # macro whose text is exactly the widget's name, and a user widget
        # shadowing that name — neither may keep the capability announced.
        rebinds = {
            "bash": [
                # Review repro: a lookalike line that merely CONTAINS the
                # entry (a chord sequence ending in \e[46~ bound to
                # clear-screen) must not satisfy the probe once the real
                # entry is gone — the quoted macro keeps the chord and fires.
                (
                    "bind '\"\\C-x\\\"\\e[46~\": clear-screen';"
                    " bind '\"\\e[46~\": \"QUOTED-KEY-MACRO\"'",
                    "QUOTED-KEY-MACRO",
                ),
                ("bind '\"\\e[46~\": \"PEBREL-REBIND\"'", "PEBREL-REBIND"),
            ],
            "zsh": [
                ("bindkey -s '\\e[46~' 'PEBREL-REBIND'", "PEBREL-REBIND"),
                ("bindkey -s '\\e[46~' 'clear-screen'", "clear-screen"),
                (
                    "userfn() { BUFFER+='USERWID'; }; zle -N clear-screen userfn;"
                    " bindkey '\\e[46~' clear-screen",
                    "USERWID",
                ),
            ],
        }[shell]
        session = self.start_bootstrap(shell, route)
        if self.check_legacy_bash(shell, session):
            return
        token = re.search(rb"SetUserVar=pebrel_editor_redraw=([^\x07]+)\x07", session.output)
        self.assertIsNotNone(token, session.output)
        if shell == "bash":
            # A decoy clear-screen on \e[146~ must not satisfy the probe for
            # \e[46~: the following rebinds revoke despite the decoy staying.
            session.command("bind '\"\\e[146~\": clear-screen'; printf 'DECOY\\n'", b"DECOY\r\n")
        for rebind, inserted in rebinds:
            os.write(session.master, b"\x15")  # discard any line from the prior case
            session.command(rebind + "; printf 'REBIND-DONE\\n'", b"REBIND-DONE\r\n")
            output = session.command("printf 'REBOUND\\n'", b"REBOUND\r\n")
            self.assertIn(b"SetUserVar=pebrel_editor_redraw=\x07", output)
            self.assertIn(b"SetUserVar=pebrel_editor_ready=", output)
            os.write(session.master, b"echo tail\x1b[46~")
            _, _, line = self.snapshot(session)
            self.assertEqual(line, f"echo tail{inserted}", f"{shell}: {rebind}")

    def test_ssh_bash_rebinding_the_redraw_chord_revokes_the_advertisement(self) -> None:
        self.check_redraw_rebind("bash", "ssh")

    def test_ssh_zsh_rebinding_the_redraw_chord_revokes_the_advertisement(self) -> None:
        self.check_redraw_rebind("zsh", "ssh")

    def check_redraw_keymap_switch(self, route: str) -> None:
        # vi mode: the prompt validates the main keymap, but one prompt can
        # switch keymaps mid-line (ESC -> vicmd). A user widget on the chord
        # in vicmd keeps the capability revoked — announcing from viins alone
        # would send the byte into user code — while the widget itself stays.
        session = self.start_bootstrap("zsh", route, vicmd_widget=True)
        self.assertIsNotNone(
            re.search(rb"SetUserVar=pebrel_editor_ready=([^\x07]+)\x07", session.output),
            session.output,
        )
        token = re.search(rb"SetUserVar=pebrel_editor_redraw=([^\x07]+)\x07", session.output)
        self.assertIsNone(token, session.output)
        self.assertIn(b"SetUserVar=pebrel_editor_redraw=\x07", session.output)
        # Within the same prompt: ESC switches to vicmd and the chord fires
        # the user's widget into the line, then Enter runs the buffer.
        os.write(session.master, b"echo tail")
        time.sleep(0.4)
        os.write(session.master, b"\x1b")
        time.sleep(0.4)
        os.write(session.master, b"\x1b[46~\n")
        output = session.wait(b"tailVICMD-USER\r\n")
        self.assertIn(b"tailVICMD-USER\r\n", output)

    def test_ssh_zsh_redraw_keymap_switch_in_the_same_prompt_revokes(self) -> None:
        self.check_redraw_keymap_switch("ssh")

    def test_ssh_bash_bootstrap_query_only_session_leaves_redraw_unbound(self) -> None:
        self.check_query_only_session("bash", "ssh")

    def test_ssh_zsh_bootstrap_query_only_session_leaves_redraw_unbound(self) -> None:
        self.check_query_only_session("zsh", "ssh")

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
