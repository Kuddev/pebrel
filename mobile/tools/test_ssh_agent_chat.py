"""The server-side reader behind the SSH conversation view: which terminal, which program, which transcript."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
# Android packages res/raw verbatim; a bytecode cache beside the script would become a resource.
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location(
    "ssh_agent_chat", ROOT / "mobile/android/app/src/main/res/raw/ssh_agent_chat.py")
chat = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(chat)

CONNECTION = "203.0.113.7 50422 192.0.2.1 22"


def row(pid, ppid, tty, args, pgid=None, tpgid=None, age=100):
    return {"pid": pid, "ppid": ppid, "pgid": pgid or pid, "tpgid": tpgid or -1, "tty": tty, "age": age, "args": args}


def table(*rows):
    return {item["pid"]: item for item in rows}


class ProgramNameTest(unittest.TestCase):
    def test_names_follow_what_a_person_would_call_the_program(self):
        cases = {
            "claude": "claude",
            "-bash": "",
            "/bin/zsh -l": "",
            "node /usr/local/bin/codex": "codex",
            "node --no-warnings /opt/app/cli.js serve": "cli",
            "python3 -I /home/u/.local/bin/codex": "codex",
            "bash ./deploy.sh": "deploy",
            "sh -c npm run dev": "npm",
            "env FOO=1 node app.js": "app",
            "vim notes.md": "vim",
        }
        for args, expected in cases.items():
            self.assertEqual(expected, chat.program(args), args)

    def test_native_claude_started_by_its_versioned_path_is_claude(self):
        self.assertEqual("claude", chat.agent_kind("/home/u/.local/share/claude/versions/2.1.310"))
        self.assertEqual("codex", chat.agent_kind("node /usr/local/bin/codex"))
        self.assertIsNone(chat.agent_kind("vim claude.md"))


class ConnectionTerminalTest(unittest.TestCase):
    def locate(self, rows, environments):
        with mock.patch.dict(os.environ, {"SSH_CONNECTION": CONNECTION}), \
                mock.patch.object(chat, "environ", lambda pid: environments.get(pid)), \
                mock.patch.object(chat.os.path, "isdir", lambda path: True):
            return chat.connection_terminal(rows)

    def test_the_shell_carrying_this_connection_owns_the_terminal(self):
        rows = table(row(10, 1, "pts/3", "-bash"), row(11, 10, "pts/3", "claude"),
                     row(20, 1, "pts/4", "-bash"), row(30, 1, "pts/9", "bash"))
        environments = {
            10: {"SSH_CONNECTION": CONNECTION, "SSH_TTY": "/dev/pts/3"},
            11: {"SSH_CONNECTION": CONNECTION, "SSH_TTY": "/dev/pts/3"},
            # Another connection from the same address differs by its client port.
            20: {"SSH_CONNECTION": "203.0.113.7 50423 192.0.2.1 22", "SSH_TTY": "/dev/pts/4"},
            # A multiplexer pane started from this connection inherits its variables on another PTY.
            30: {"SSH_CONNECTION": CONNECTION, "SSH_TTY": "/dev/pts/3"},
        }
        self.assertEqual("pts/3", self.locate(rows, environments))

    def test_two_terminals_on_one_connection_are_ambiguous(self):
        rows = table(row(10, 1, "pts/3", "-bash"), row(20, 1, "pts/4", "-bash"))
        environments = {10: {"SSH_CONNECTION": CONNECTION, "SSH_TTY": "/dev/pts/3"},
                        20: {"SSH_CONNECTION": CONNECTION, "SSH_TTY": "/dev/pts/4"}}
        self.assertIsNone(self.locate(rows, environments))

    def test_without_readable_environments_the_sshd_ancestry_decides(self):
        me = os.getpid()
        rows = table(row(5, 1, "?", "sshd: u@pts/3"), row(10, 5, "pts/3", "-bash"),
                     row(me, 5, "?", "python3 reader.py"))
        self.assertEqual("pts/3", self.locate(rows, {}))


class LocateTest(unittest.TestCase):
    def test_a_tmux_client_leads_to_the_agent_in_the_pane_it_shows(self):
        rows = table(row(10, 1, "pts/3", "-bash", pgid=10, tpgid=12),
                     row(12, 10, "pts/3", "tmux attach", pgid=12, tpgid=12),
                     row(40, 1, "pts/9", "-zsh", pgid=40, tpgid=41),
                     row(41, 40, "pts/9", "node /usr/local/bin/codex", pgid=41, tpgid=41),
                     row(42, 41, "pts/9", "/usr/lib/codex/vendor/codex", pgid=41, tpgid=41))
        with mock.patch.object(chat, "connection_terminal", lambda rows: "pts/3"), \
                mock.patch.object(chat, "tmux_pane", lambda client, terminal: ("pts/9", "%2")), \
                mock.patch.object(chat, "link", lambda path: "/srv/app" if path == "/proc/42/cwd" else None):
            view = chat.locate(rows, {})
        self.assertEqual(("tmux", "%2", "codex", "codex", "/srv/app"),
                         (view["multiplexer"], view["pane"], view["agent"], view["program"], view["cwd"]))
        self.assertEqual(42, view["agents"][0]["pid"])

    def test_an_idle_shell_reports_its_directory_and_no_program(self):
        rows = table(row(10, 1, "pts/3", "-bash", pgid=10, tpgid=10))
        with mock.patch.object(chat, "connection_terminal", lambda rows: "pts/3"), \
                mock.patch.object(chat, "link", lambda path: "/home/u" if path == "/proc/10/cwd" else None):
            view = chat.locate(rows, {})
        self.assertEqual(("", None, "/home/u", None), (view["program"], view["agent"], view["cwd"], view["multiplexer"]))

    def test_tmux_is_asked_on_the_socket_its_client_names(self):
        client = row(12, 10, "pts/3", "tmux -L work attach")
        calls = []

        def run(args, timeout=5):
            calls.append(args)
            return "99\t/dev/pts/7\t/dev/pts/8\t%1\n12\t/dev/pts/3\t/dev/pts/9\t%2\n"

        with mock.patch.object(chat, "run", run), mock.patch.object(chat, "link", lambda path: None), \
                mock.patch.object(chat, "environ", lambda pid: {"TMUX_TMPDIR": "/run/t"}):
            self.assertEqual(("pts/9", "%2"), chat.tmux_pane(client, "pts/3"))
        self.assertEqual(["tmux", "-S", "/run/t/tmux-%d/work" % os.getuid()], calls[0][:3])


class ClaudeSessionTest(unittest.TestCase):
    def setUp(self):
        self.home = tempfile.TemporaryDirectory()
        self.addCleanup(self.home.cleanup)
        self.claude = Path(self.home.name, ".claude")
        self.folder = self.claude / "projects" / "-srv-app"
        self.folder.mkdir(parents=True)
        (self.claude / "sessions").mkdir()
        self.agent = row(77, 10, "pts/3", "claude", age=30)
        patches = [mock.patch.object(chat, "HOME", self.home.name), mock.patch.object(chat, "environ", lambda pid: {}),
                   mock.patch.object(chat, "started", lambda pid: "4242")]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def register(self, session, stamp="4242"):
        (self.claude / "sessions" / "77.json").write_text(
            json.dumps({"pid": 77, "sessionId": session, "cwd": "/srv/app", "procStart": stamp}), encoding="utf-8")

    def test_the_registered_session_wins_over_a_newer_transcript(self):
        mine = self.folder / "0b6e5a52-0000-4000-8000-000000000001.jsonl"
        mine.write_text("{}\n", encoding="utf-8")
        newer = self.folder / "0b6e5a52-0000-4000-8000-000000000002.jsonl"
        newer.write_text("{}\n", encoding="utf-8")
        os.utime(mine, (1, 1))
        self.register(mine.stem)
        self.assertEqual(str(mine), chat.transcript("claude", [self.agent], "/srv/app"))

    def test_a_registered_session_without_messages_shows_none_of_another(self):
        (self.folder / "0b6e5a52-0000-4000-8000-000000000002.jsonl").write_text("{}\n", encoding="utf-8")
        self.register("0b6e5a52-0000-4000-8000-000000000001")
        self.assertIsNone(chat.transcript("claude", [self.agent], "/srv/app"))

    def test_a_record_left_by_an_earlier_process_is_ignored(self):
        self.register("0b6e5a52-0000-4000-8000-000000000001", stamp="1")
        self.assertIsNone(chat.claude_session([self.agent]))


class RelayTest(unittest.TestCase):
    """`sudo -i -u dev mydev` opens a PTY of its own and runs tmux as another account."""

    rows = table(row(10, 1, "pts/5", "-bash", tpgid=20),
                 row(20, 10, "pts/5", "sudo -n -i -u dev /usr/local/bin/mydev", tpgid=20),
                 row(21, 20, "pts/9", "sudo -n -i -u dev /usr/local/bin/mydev", tpgid=22),
                 row(22, 21, "pts/9", "/opt/tmux/bin/tmux -L nebula-dev-3.8 new-session -A -s nebula", tpgid=22))

    def test_the_relayed_terminal_is_followed_to_the_tmux_client(self):
        with mock.patch.object(chat, "tmux_pane", lambda client, terminal: None), \
                mock.patch.object(chat, "owner", lambda pid: 1002), mock.patch.object(chat, "link", lambda path: None):
            view = chat.locate(self.rows, {"tty": "/dev/pts/5"})
        self.assertEqual(("pts/9", "tmux", 1002), (view["relay"], view["multiplexer"], view["owner"]))

    def test_another_account_is_asked_through_noninteractive_sudo(self):
        reply = {"terminal": "pts/9", "multiplexer": "tmux", "pane": "%1", "agent": "codex", "program": "codex",
                 "cwd": "/tmp", "home": "/home/dev", "version": 1}
        calls = []

        def run(args, **options):
            calls.append((args, json.loads(chat.base64.b64decode(args[-1]))))
            return mock.Mock(stdout=("PEBREL_SSH_CHAT=" + json.dumps(reply) + "\n").encode())

        with mock.patch.object(chat, "processes", lambda: self.rows), \
                mock.patch.object(chat, "tmux_pane", lambda client, terminal: None), \
                mock.patch.object(chat, "owner", lambda pid: os.getuid() + 1), \
                mock.patch.object(chat, "link", lambda path: None), mock.patch.object(chat.subprocess, "run", run):
            result = chat.main({"mode": "detect", "tty": "/dev/pts/5"})
        self.assertEqual("codex", result["agent"])
        self.assertEqual(["sudo", "-n", "-u", "#%d" % (os.getuid() + 1)], calls[0][0][:4])
        self.assertEqual({"mode": "detect", "tty": "/dev/pts/9", "delegated": True}, calls[0][1])

    def test_a_delegated_question_is_never_delegated_again(self):
        with mock.patch.object(chat, "processes", lambda: self.rows), \
                mock.patch.object(chat, "tmux_pane", lambda client, terminal: None), \
                mock.patch.object(chat, "owner", lambda pid: os.getuid() + 1), \
                mock.patch.object(chat, "link", lambda path: None), \
                mock.patch.object(chat.subprocess, "run", side_effect=AssertionError("delegated twice")):
            result = chat.main({"mode": "detect", "tty": "/dev/pts/9", "delegated": True})
        self.assertIsNone(result["agent"])


if __name__ == "__main__":
    unittest.main()
