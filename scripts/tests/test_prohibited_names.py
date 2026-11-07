from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scripts import check_prohibited_names


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts" / "check_prohibited_names.py"
FIXTURES = ROOT / "scripts" / "tests" / "fixtures" / "prohibited_names"


def run_git(repository: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repository, check=True, stdout=subprocess.DEVNULL)


class ProhibitedNamesTests(unittest.TestCase):
    def test_graphics_protocol_names_preserve_the_exact_technical_reference(self) -> None:
        for text in ("Support the Kitty graphics protocol.", "支持 Kitty 图形协议。"):
            with self.subTest(text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line("CHANGELOG.md", text))
                self.assertTrue(check_prohibited_names.prohibited_source_line(
                    "CHANGELOG.md", text + " Compare with Kitty."
                ))
                self.assertTrue(check_prohibited_names.prohibited_source_line(
                    "CHANGELOG.md", text + " https://example.invalid/kitty"
                ))
        for text in ("Kitty graphics are beautiful", "Kitty 图像", "Use Kitty instead",
                     "Kitty graphics protocol_adapter"):
            self.assertTrue(check_prohibited_names.prohibited_source_line("CHANGELOG.md", text))

    def test_activity_fixtures_preserve_real_process_names_without_exempting_prose(self) -> None:
        path = "nebula_app/src/gpui_shell/terminal/view/activity_tests.rs"
        cases = ('view.running_program = Some("tmux".into());',
                 'for program in ["tmux", "screen"] {')
        for text in cases:
            with self.subTest(text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))
                self.assertTrue(check_prohibited_names.prohibited_source_line(
                    path.replace("activity_tests", "other_tests"), text
                ))
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + " // use tmux"))
                self.assertTrue(check_prohibited_names.prohibited_source_line(
                    path, text + " // https://example.invalid/tmux"
                ))
        for text in ('let label = "tmux";', '// tmux 里的 Agent 经 hook 报告回合',
                     'hook("inside-tmux", "UserPromptSubmit", 1)', 'println!("tmux");'):
            self.assertTrue(check_prohibited_names.prohibited_source_line(path, text))

    def test_desktop_endpoint_protocol_references_do_not_allow_prose(self) -> None:
        cases = (
            ("resume.rs", 'Self { executable: "tmux".into(), socket: String::new() }'),
            ("resume/identity.rs", 'command == "tmux" || command == "tmux:" || command.ends_with("/tmux")'),
            ("resume/discover.sh", r'$3 == "tmux" || $3 == "tmux:" || $3 ~ /\/tmux$/'),
            ("resume/discover.sh", r"sed -n 's/^n\(.*\/tmux\)$/\1/p'"),
            ("resume/tests.rs", r'identity.has_client("42 pts/3 bash\n43 pts/3 tmux: client\n")'),
        )
        for suffix, text in cases:
            path = "nebula_app/src/ssh_session/" + suffix
            with self.subTest(path=path, text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + " // use tmux"))
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + " // use Ghostty"))
        for path, text in (
            ("resume.rs", 'let description = "tmux";'),
            ("resume/identity.rs", '// use tmux for everything'),
            ("resume/discover.sh", "tmux unknown-command"),
            ("resume/tests.rs", 'println!("tmux");'),
            ("other.rs", 'Self { executable: "tmux".into(), socket: String::new() }'),
        ):
            self.assertTrue(check_prohibited_names.prohibited_source_line("nebula_app/src/ssh_session/" + path, text))

    def test_truncated_unicode_hunk_context_preserves_added_source_checks(self) -> None:
        context = "managed generation 改变时，旧回调".encode("utf-8")[:-1]
        sample = "++" + check_prohibited_names.PROHIBITED_EXAMPLES[0]
        for parents, header, prefix in ((None, b"@@ -1 +1 @@ ", b"+"), (2, b"@@@ -1 -1 +1 @@@ ", b"++")):
            with self.subTest(parents=parents):
                diff = (
                    b"+++ b/source.rs\n" + header + context + b"\n"
                    + prefix + b"safe\n" + prefix + sample.encode("utf-8") + b"\n"
                )
                lines = list(check_prohibited_names.added_patch_lines(diff, parents))
                self.assertEqual(lines, [(3, "safe"), (4, sample)])
                self.assertTrue(check_prohibited_names.prohibited_source_line("source.rs", lines[1][1]))
                if parents is None:
                    with patch.object(check_prohibited_names, "git", return_value=diff):
                        self.assertEqual(list(check_prohibited_names.added_lines("source.rs", "--cached")), lines)

    def test_invalid_added_utf8_is_not_hidden_by_a_truncated_hunk_header(self) -> None:
        for parents, prefix in ((None, b"+"), (2, b"++")):
            with self.subTest(parents=parents):
                diff = b"@@@ context \xe8\xb0\n" + prefix + b"bad \xff\n"
                with self.assertRaises(UnicodeDecodeError):
                    list(check_prohibited_names.added_patch_lines(diff, parents))

    def check_staged(self, repository: Path):
        return subprocess.run(
            [sys.executable, str(CHECKER), "staged"], cwd=repository,
            text=True, encoding="utf-8", capture_output=True,
            env={**os.environ, "PYTHONUTF8": "1"},
        )

    def test_invalid_utf8_path_cannot_turn_into_an_empty_diff(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            # Git trees can contain these bytes even when the host filesystem
            # cannot. Never require a checkout or create an invalid OS filename.
            def write_object(kind, contents):
                return subprocess.check_output(
                    ["git", "hash-object", "-w", "-t", kind, "--stdin"],
                    input=contents, cwd=repository,
                ).decode("ascii").strip()

            empty_tree = write_object("tree", b"")
            base = subprocess.check_output(
                ["git", "commit-tree", empty_tree], input=b"baseline\n", cwd=repository,
            ).decode("ascii").strip()
            blob = write_object("blob", b"Ghostty\n")
            tree = write_object("tree", b"100644 bad-\xff.rs\0" + bytes.fromhex(blob))
            head = subprocess.check_output(
                ["git", "commit-tree", tree, "-p", base],
                input=b"invalid path fixture\n", cwd=repository,
            ).decode("ascii").strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository, text=True, encoding="utf-8", capture_output=True,
                env={**os.environ, "PYTHONUTF8": "1"},
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("utf-8", result.stderr.lower())

    def test_invalid_utf8_text_and_message_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            path = repository / "source.rs"
            path.write_bytes(b"safe \xff\n")
            run_git(repository, "add", "source.rs")
            staged = self.check_staged(repository)
            message = subprocess.run(
                [sys.executable, str(CHECKER), "message", str(path)], cwd=repository,
                text=True, encoding="utf-8", capture_output=True,
                env={**os.environ, "PYTHONUTF8": "1"},
            )
            for result in (staged, message):
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("utf-8", result.stderr.lower())

    def test_valid_unicode_and_control_character_paths_are_scanned(self) -> None:
        names = ["路径.rs"]
        if os.name != "nt":
            names.append("control\tline\n.rs")
        for name in names:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                run_git(repository, "init", "-q")
                path = repository / name
                path.write_text("safe\n", encoding="utf-8")
                run_git(repository, "add", "--", name)
                self.assertEqual(self.check_staged(repository).returncode, 0)
                path.write_text("Ghostty\n", encoding="utf-8")
                run_git(repository, "add", "--", name)
                result = self.check_staged(repository)
                self.assertEqual(result.returncode, 1)
                self.assertIn("Ghostty", result.stderr)

    def test_names_do_not_match_inside_unrelated_words(self) -> None:
        self.assertFalse(check_prohibited_names.prohibited("spotty network; tty72; netcatty_adapter"))
        self.assertTrue(check_prohibited_names.prohibited("Use Otty or TTY-7"))

    def test_ordinary_competitor_comparison_is_rejected(self) -> None:
        text = (FIXTURES / "ordinary-comparison.md").read_text(encoding="utf-8")
        self.assertTrue(check_prohibited_names.prohibited_source_line("comparison.md", text))

    def test_inline_legal_attribution_is_allowed_but_comparison_is_not(self) -> None:
        text = (FIXTURES / "legal-notice.txt").read_text(encoding="utf-8")
        self.assertFalse(check_prohibited_names.prohibited_source_line("source.rs", text))
        self.assertTrue(
            check_prohibited_names.prohibited_source_line("README.md", "Compare with Alacritty.")
        )
        self.assertTrue(
            check_prohibited_names.prohibited_source_line(
                "source.rs", "Copyright (c) Alacritty contributors; compare with Ghostty."
            )
        )

    def test_protocol_identifiers_are_allowed(self) -> None:
        text = (FIXTURES / "protocol.rs").read_text(encoding="utf-8")
        self.assertFalse(check_prohibited_names.prohibited_source_line("src/protocol.rs", text))

    def test_github_cli_text_is_not_a_competitor_name(self) -> None:
        command = (FIXTURES / "github-cli.sh").read_text(encoding="utf-8")
        documentation = (FIXTURES / "github-cli-docs.sh").read_text(encoding="utf-8")
        self.assertFalse(check_prohibited_names.prohibited_source_line("check.sh", command))
        self.assertFalse(check_prohibited_names.prohibited_source_line("check.sh", documentation))

    def test_real_cargo_git_dependency_is_allowed_but_readme_reference_is_not(self) -> None:
        dependency = 'gpui = { git = "https://github.com/ghostty/ghostty", rev = "deadbeef" }'
        self.assertFalse(check_prohibited_names.prohibited_source_line("Cargo.toml", dependency))
        self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", dependency))

    def test_mobile_build_and_session_references_preserve_their_interfaces(self) -> None:
        cases = (
            ("mobile/android/third_party/reader-build/package-lock.json", '"node_modules/@types/d3-contour": {'),
            ("mobile/android/app/build.gradle.kts", 'implementation(project(":ghostty"))'),
            ("mobile/android/settings.gradle.kts", 'include(":app", ":ghostty")'),
            ("mobile/tools/generate_assets.py", 'root / "mobile/android/ghostty/UPSTREAM.json"'),
            (".github/workflows/android-release.yml", "path: 'mobile/android/ghostty/build/upstream'"),
            (".github/workflows/android-release.yml", "key: native-${{ hashFiles('mobile/android/ghostty/UPSTREAM.json') }}"),
            ("mobile/tools/generate_assets.py", 'notices / "Ghostty"'),
            ("mobile/tools/test_apk_audit.py", '"assets/licenses/Ghostty/Ghostty-MIT.txt": b"fixture"'),
            ("mobile/android/ghostty/src/main/cpp/bridge.h", '#include <ghostty/vt.h>'),
            ("mobile/android/ghostty/UPSTREAM.json", '"project": "https://github.com/ghostty-org/ghostty",'),
            ("mobile/ssh/licenses/SOURCE.json", '"url": "https://raw.githubusercontent.com/warp-tech/russh/revision/LICENSE.txt",'),
            ("mobile/android/app/src/main/java/connection/SshSessionMode.kt", 'TMUX("tmux"), HERDR("herdr")'),
            ("mobile/android/app/src/main/java/connection/SshSessionMode.kt", 'SshSessionMode.TMUX -> "exec tmux new-session -A -s \'pebrel\'"'),
            ("mobile/android/app/src/main/java/connection/SshSessionMode.kt", 'SshSessionMode.HERDR -> "exec herdr"'),
            ("mobile/android/app/src/androidTest/SshIntegrationTest.kt", '"tmux kill-session -t \'session\'" else "herdr session stop \'session\'"'),
        )
        for path, text in cases:
            with self.subTest(path=path, text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
                # 合法引用仅保护自身，不能让同行追加的比较文案一起逃逸。
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + '; compare with Ghostty'))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))

    def test_remote_discovery_commands_and_wire_tags_are_not_promotion(self) -> None:
        path = "mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/connection/RemoteSessions.kt"
        cases = (
            "if command -v tmux >/dev/null; then",
            "if command -v herdr >/dev/null; then herdr session list --json; fi",
            "tmux list-sessions -F '#{session_id}'",
            "tmux list-windows -a -F '#{window_id}'",
            "if (Get-Command herdr -ErrorAction SilentlyContinue) { & herdr session list --json }",
            '"tmux select-window -t ${quote(window.id)}"',
            '"$(tmux display-message -p -t ${quote(session.id)})"',
            '"exec tmux attach-session -t ${quote(session.id)}"',
            '"& herdr session attach ${psQuote(session.id)}"',
            '"herdr --session ${quote(session.id)} tab focus ${quote(window.id)}"',
            '"& herdr --session ${psQuote(session.id)} workspace list"',
            '"& herdr --session ${psQuote(session.id)} tab list"',
            'val sessions = data["tmux"].orEmpty()',
            'data["herdr"]?.let { body ->',
            'RemoteSession("tmux", id, name)',
            'RemoteSession("herdr", id, name)',
            'if (session.kind != "herdr") return session.windows',
            'require(session.kind == "herdr")',
            '    "tmux" -> {',
            '    "herdr" -> {',
            '}.onFailure { warnings += "herdr" }',
            "[Console]::Write([char]30 + 'herdr' + [char]10)",
        )
        for text in cases:
            with self.subTest(text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))
                for name in ("tmux", "herdr", "Ghostty"):
                    self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + "; compare with " + name))
        for text in ('val label = "herdr"', '"tmux is better"', 'herdr unknown-command', 'tmux unknown-command'):
            with self.subTest(text=text):
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text))
        self.assertTrue(check_prohibited_names.prohibited_source_line(
            path.replace("RemoteSessions.kt", "Other.kt"), 'RemoteSession("tmux", id, name)',
        ))

    def test_mobile_reference_paths_do_not_exempt_ordinary_mentions(self) -> None:
        for path in (
            "mobile/tools/generate_assets.py", "mobile/android/app/build.gradle.kts",
            ".github/workflows/android-release.yml",
            "mobile/android/ghostty/UPSTREAM.json", "mobile/ssh/licenses/SOURCE.json",
            "mobile/android/app/src/main/java/connection/SshSessionMode.kt",
        ):
            with self.subTest(path=path):
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, '"description": "Inspired by Ghostty"'))
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, '"url": "https://example.invalid/ghostty"'))

    def test_remote_endpoint_metadata_and_isolated_recovery_commands_are_scoped(self) -> None:
        root = "mobile/android/app/src/"
        package = "java/io/github/kuddev/pebrel/mobile/connection/"
        cases = (
            ("main/" + package + "TmuxEndpoint.kt", 'data class TmuxEndpoint(val executable: String = "tmux", val socket: String = "")'),
            ("main/" + package + "TmuxEndpoint.kt", 'require(executable == "tmux" || validPath(executable))'),
            ("main/" + package + "TmuxEndpoint.kt", r'''${'$'}3 == "tmux:" || ${'$'}3 ~ /\/tmux${'$'}/'''),
            ("main/" + package + "TmuxEndpoint.kt", r"sed -n 's/^n\(.*\/tmux\)$/\1/p'"),
            ("main/" + package + "SshPtyIdentity.kt", 'it.command == "tmux" || it.command.startsWith("tmux:")'),
            ("main/" + package + "SshSessionMode.kt", "tmux has-session -t '=original'"),
            ("test/" + package + "TmuxSessionControlTest.kt", 'TmuxEndpoint("/opt/server 3.8/bin/tmux", "/tmp/tmux-123/custom socket")'),
            ("test/" + package + "TmuxSessionControlTest.kt", r'"20 10 pts/7 sh\n21 20 pts/7 tmux: client\n50 10 ? sh\n"'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'TmuxEndpoint("tmux", socket)'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'ProcessBuilder("tmux", "-V")'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'environment().remove("TMUX")'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'run("tmux", "-S", socket, "-f", "/dev/null", "new-session", "-d")'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'run("tmux", "-S", socket, "has-session", "-t", "original")'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'run("tmux", "-S", socket, "display-message", "-p")'),
            ("test/" + package + "TmuxRecoveryCommandTest.kt", 'run("tmux", "-S", socket, "kill-server")'),
        )
        for relative, text in cases:
            path = root + relative
            with self.subTest(path=path, text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + '; compare with tmux'))
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + '; compare with Ghostty'))
        for filename, text in (
            ("TmuxEndpoint.kt", 'val label = "tmux"'),
            ("TmuxEndpoint.kt", '// tmux is better'),
            ("SshPtyIdentity.kt", 'val label = "tmux"'),
            ("Other.kt", 'it.command == "tmux"'),
        ):
            self.assertTrue(check_prohibited_names.prohibited_source_line(root + "main/" + package + filename, text))
        self.assertTrue(check_prohibited_names.prohibited_source_line(
            root + "test/" + package + "TmuxRecoveryCommandTest.kt", 'run("tmux", "-S", socket, "unknown-command")'))
        for text in ('TmuxEndpoint("/opt/Ghostty/tmux", socket)', 'val socket = "/tmp/tmux-123/Ghostty"'):
            self.assertTrue(check_prohibited_names.prohibited_source_line(
                root + "test/" + package + "TmuxSessionControlTest.kt", text))

    def test_remote_history_commands_allow_only_the_supported_cli_forms(self) -> None:
        root = "mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/connection/"
        cases = (
            ("RemoteSessions.kt", "tmux -u list-sessions -F '#{session_id}'"),
            ("RemoteSessions.kt", "tmux -u list-windows -a -F '#{window_id}'"),
            ("RemoteSessions.kt", '"tmux select-pane -t ${quote(pane)} && "'),
            ("TmuxSessionControl.kt", "tmux list-clients -F '#{client_tty}'"),
            ("TmuxSessionControl.kt", "tmux -u list-clients -F '#{client_tty}'"),
            ("TmuxSessionControl.kt", 'else "tmux copy-mode -t \'$target\' && "'),
            ("TmuxSessionControl.kt", '"tmux send-keys -X -N ${count} -t \'$target\' $action"'),
            ("TmuxSessionControl.kt", '"tmux send-keys -X -t \'${target(attachment)}\' cancel"'),
        )
        for filename, text in cases:
            path = root + filename
            with self.subTest(path=path, text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))
                for name in ("tmux", "herdr", "Ghostty"):
                    self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + "; compare with " + name))
        for text in ("tmux unknown-command", "tmux -u unknown-command", "tmux send-keys -l text", "tmux copy-mode advice"):
            self.assertTrue(check_prohibited_names.prohibited_source_line(root + "TmuxSessionControl.kt", text))
        self.assertTrue(check_prohibited_names.prohibited_source_line(root + "Other.kt", "tmux copy-mode -t '$target'"))

    def test_remote_model_tags_follow_typed_consumers_and_their_tests(self) -> None:
        root = "mobile/android/app/src/"
        package = "java/io/github/kuddev/pebrel/mobile/"
        paths = (
            "main/" + package + "connection/RemoteSessions.kt",
            "main/" + package + "connection/TmuxSessionControl.kt",
            "main/" + package + "session/SessionRepository.kt",
            "test/" + package + "connection/SshReconnectTest.kt",
            "test/" + package + "connection/TmuxDiscoveryTest.kt",
            "test/" + package + "session/SshRecoveryTest.kt",
        )
        for path in paths:
            for text in ('RemoteSession("tmux", id, name)', 'RemoteSession("herdr", id, name)',
                         'attachment?.session?.kind == "tmux"', 'it.kind == "tmux"'):
                with self.subTest(path=path, text=text):
                    self.assertFalse(check_prohibited_names.prohibited_source_line(root + path, text))
                    self.assertTrue(check_prohibited_names.prohibited_source_line(root + path, text + '; compare with tmux'))
            self.assertTrue(check_prohibited_names.prohibited_source_line(root + path, 'val label = "tmux"'))
        path = root + paths[0]
        for text in ('SshSessionMode.TMUX -> "tmux"', 'SshSessionMode.HERDR -> "herdr"'):
            self.assertFalse(check_prohibited_names.prohibited_source_line(path, text))
            self.assertTrue(check_prohibited_names.prohibited_source_line(path, text + '; compare with herdr'))
        self.assertTrue(check_prohibited_names.prohibited_source_line(path, 'SshSessionMode.TMUX -> "herdr"'))
        for path in ("README.md", root + "main/" + package + "connection/Other.kt",
                     root + "test/" + package + "ui/PromotionTest.kt"):
            self.assertTrue(check_prohibited_names.prohibited_source_line(path, 'RemoteSession("tmux", id, name)'))

    def test_real_remote_process_fixtures_keep_executable_and_environment_names(self) -> None:
        root = "mobile/android/app/src/test/java/io/github/kuddev/pebrel/mobile/connection/"
        cases = (
            ("TmuxDiscoveryTest.kt", 'ProcessBuilder("tmux", "-V").start()'),
            ("TmuxDiscoveryTest.kt", 'command("tmux", "-f", "/dev/null", "new-session", "-d", "-s", "fixture")'),
            ("TmuxDiscoveryTest.kt", 'command("tmux", "kill-server")'),
            ("TmuxDiscoveryTest.kt", 'remove("TMUX"); remove("LANG")'),
            ("SshPtyIdentityTest.kt", "            111 110 pts/7 tmux: client"),
            ("SshPtyIdentityTest.kt", "            300 1 ? tmux: server"),
        )
        for filename, text in cases:
            with self.subTest(filename=filename, text=text):
                self.assertFalse(check_prohibited_names.prohibited_source_line(root + filename, text))
                self.assertTrue(check_prohibited_names.prohibited_source_line(root + filename, text + "; compare with tmux"))
                self.assertTrue(check_prohibited_names.prohibited_source_line(root + "OtherTest.kt", text))
                self.assertTrue(check_prohibited_names.prohibited_source_line("README.md", text))
        for text in ('command("tmux", "buy-now")', 'ProcessBuilder("tmux", "sponsor")',
                     'val label = "tmux"', 'remove("TMUX premium")', "tmux: client", "111 110 pts/7 tmux: sponsor"):
            for filename in ("TmuxDiscoveryTest.kt", "SshPtyIdentityTest.kt"):
                self.assertTrue(check_prohibited_names.prohibited_source_line(root + filename, text))

    def test_technical_notes_and_fix_subjects_do_not_exempt_ordinary_mentions(self) -> None:
        paths = (
            "architecture/notes/mobile/android/app/connection/2026-10-22-recovery.md",
            "mobile/android/app/src/main/java/connection/TmuxSessionControl.kt",
            "mobile/android/app/src/test/java/connection/TmuxDiscoveryTest.kt",
            "README.md",
        )
        for path in paths:
            for text in ("Compare with tmux.", "Typed tmux commands use an independent channel.",
                         "Promote herdr.", "Use Ghostty instead."):
                self.assertTrue(check_prohibited_names.prohibited_source_line(path, text))
        with tempfile.TemporaryDirectory() as directory:
            message = Path(directory) / "message.txt"
            for text in ("fix(android): scroll tmux history", "fix(android): compare with herdr"):
                message.write_text(text, encoding="utf-8")
                self.assertTrue(check_prohibited_names.scan_message(message))

    def test_range_and_staged_checks_keep_remote_commands_but_reject_appended_promotion(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            run_git(repository, "commit", "--allow-empty", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            path = "mobile/android/app/src/main/java/connection/TmuxSessionControl.kt"
            source = repository / path
            source.parent.mkdir(parents=True)
            line = 'val command = "tmux send-keys -X -t \'$target\' cancel"'
            source.write_text(line + "\n", encoding="utf-8")
            run_git(repository, "add", path)
            self.assertEqual(self.check_staged(repository).returncode, 0)
            run_git(repository, "commit", "-qm", "fix(android): return the remote client to live input")
            check = [sys.executable, str(CHECKER), "range", "--base", base, "--head", "HEAD"]
            clean = subprocess.run(check, cwd=repository, text=True, capture_output=True)
            self.assertEqual(clean.returncode, 0, clean.stderr)
            source.write_text(line + " // Sponsor https://example.invalid/tmux\n", encoding="utf-8")
            run_git(repository, "add", path)
            self.assertEqual(self.check_staged(repository).returncode, 1)
            run_git(repository, "commit", "-qm", "append a forbidden promotion")
            bad = subprocess.run(check, cwd=repository, text=True, capture_output=True)
            self.assertEqual(bad.returncode, 1, bad.stderr)
            self.assertIn("example.invalid/tmux", bad.stderr)

    def test_staged_hook_configuration_is_not_exempt(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            (repository / ".pre-commit-config.yaml").write_text(
                "# Compare with Ghostty.\nrepos: []\n", encoding="utf-8"
            )
            run_git(repository, "add", ".pre-commit-config.yaml")
            result = subprocess.run(
                [sys.executable, str(CHECKER), "staged"], cwd=repository,
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn(".pre-commit-config.yaml", result.stderr)

    def test_theme_format_message_does_not_exempt_an_appended_comparison(self) -> None:
        message = (
            "unsupported theme format; use Pebrel JSON, Windows Terminal JSON, "
            "Kitty, Ghostty, WezTerm or Alacritty"
        )
        self.assertFalse(check_prohibited_names.prohibited_source_line("src/theme.rs", message))
        self.assertTrue(check_prohibited_names.prohibited_source_line(
            "src/theme.rs", message + "; copied from Ghostty"
        ))

    def test_range_scans_added_text_and_new_commit_messages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "README.md").write_text("safe\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            (repository / "comparison.md").write_text("Compare with Ghostty.\n", encoding="utf-8")
            run_git(repository, "add", "comparison.md")
            run_git(repository, "commit", "-qm", "ordinary comparison")
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository,
                text=True,
                capture_output=True,
                env={**os.environ, "PYTHONUTF8": "1"},
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Ghostty", result.stderr)

    def test_range_scans_a_commit_message_even_when_text_is_legal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "README.md").write_text("safe\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            (repository / "README.md").write_text("still safe\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "Compare with Ghostty")
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("commit-message", result.stderr)

    def test_range_uses_merge_base_and_keeps_added_then_deleted_text(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q", "-b", "fixture-base")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "README.md").write_text("Historical Ghostty attribution\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "baseline")
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            run_git(repository, "branch", "main")
            run_git(repository, "switch", "main")
            (repository / "README.md").write_text("safe on main\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "advance main")

            run_git(repository, "switch", "-c", "feature", base)
            (repository / "README.md").write_text("safe on feature\n", encoding="utf-8")
            run_git(repository, "add", "README.md")
            run_git(repository, "commit", "-qm", "advance feature")
            clean_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            clean = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", "main", "--head", clean_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertEqual(clean.returncode, 0, clean.stderr)

            (repository / "temporary.md").write_text("Ghostty\n", encoding="utf-8")
            run_git(repository, "add", "temporary.md")
            run_git(repository, "commit", "-qm", "temporary comparison")
            (repository / "temporary.md").unlink()
            run_git(repository, "add", "-u")
            run_git(repository, "commit", "-qm", "remove temporary comparison")
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", base, "--head", head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("temporary.md", result.stderr)

            unrelated_tree = subprocess.check_output(["git", "write-tree"], cwd=repository, text=True).strip()
            unrelated = subprocess.check_output(
                ["git", "commit-tree", unrelated_tree, "-m", "unrelated"],
                cwd=repository,
                text=True,
            ).strip()
            unrelated_result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", unrelated, "--head", clean_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(unrelated_result.returncode, 0)
            self.assertIn("cannot scan prohibited-name range", unrelated_result.stderr)

    def test_range_scans_merge_resolution_without_replaying_second_parent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q", "-b", "fixture-base")
            run_git(repository, "config", "user.name", "CI test")
            run_git(repository, "config", "user.email", "ci@example.invalid")
            (repository / "shared.md").write_text("base\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "baseline")
            run_git(repository, "branch", "main")

            run_git(repository, "switch", "-c", "feature")
            (repository / "shared.md").write_text("feature\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "feature change")
            feature_parent = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            run_git(repository, "switch", "main")
            (repository / "shared.md").write_text("main Ghostty\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "main change")
            main_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            run_git(repository, "switch", "feature")
            merge = subprocess.run(
                ["git", "merge", "--no-ff", "main", "-m", "merge main"],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(merge.returncode, 0, merge.stdout + merge.stderr)
            (repository / "shared.md").write_text("resolved safe\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "resolve safe merge")
            safe_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()

            safe_result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", main_head, "--head", safe_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertEqual(safe_result.returncode, 0, safe_result.stderr)

            run_git(repository, "switch", "-c", "feature-bad", feature_parent)
            merge_bad = subprocess.run(
                ["git", "merge", "--no-ff", "main", "-m", "merge main with bad resolution"],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(merge_bad.returncode, 0, merge_bad.stdout + merge_bad.stderr)
            (repository / "shared.md").write_text("resolved Ghostty\n", encoding="utf-8")
            run_git(repository, "add", "shared.md")
            run_git(repository, "commit", "-qm", "resolve bad merge")
            bad_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
            bad_result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", main_head, "--head", bad_head],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(bad_result.returncode, 0)
            self.assertIn("shared.md", bad_result.stderr)

    def test_range_fails_closed_for_an_unknown_base(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            run_git(repository, "init", "-q")
            result = subprocess.run(
                [sys.executable, str(CHECKER), "range", "--base", "missing", "--head", "HEAD"],
                cwd=repository,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("cannot scan prohibited-name range", result.stderr)


if __name__ == "__main__":
    unittest.main()
