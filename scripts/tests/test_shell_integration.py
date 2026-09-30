from __future__ import annotations

import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from urllib.parse import quote

SCRIPTS = Path(__file__).resolve().parents[2] / "nebula_app/res/shell"
# Hosted runners ship group/world-writable fpath directories; a native compinit would
# stop at compaudit's prompt there too. Drop what compaudit rejects (the directory,
# its parent, its .zwc digest or a file inside it) before the login shell's compinit.
SECURE_FPATH_PROFILE = """autoload -Uz compaudit
_insecure=(${(f)"$(compaudit 2>/dev/null)"})
_kept=()
for _dir in $fpath; do
    (( ${_insecure[(Ie)$_dir]} || ${_insecure[(Ie)${_dir:h}]} || ${_insecure[(Ie)$_dir.zwc]} )) && continue
    for _entry in $_insecure; do [[ ${_entry:h} == $_dir ]] && continue 2; done
    _kept+=($_dir)
done
fpath=($_kept)
unset _insecure _kept _dir _entry
"""


class ShellSession:
    def __init__(self, program: str, home: Path, args: list[str], env: dict[str, str]) -> None:
        import pty

        self.master, slave = pty.openpty()
        self.output = b""
        self.process = subprocess.Popen(
            [program, *args], stdin=slave, stdout=slave, stderr=slave, cwd=home,
            env={"PATH": os.environ["PATH"], "HOME": str(home), "TERM": "xterm-256color",
                 "LC_ALL": "en_US.UTF-8" if sys.platform == "darwin" else "C.UTF-8", "PS1": "nebula-test> ",
                 **{name: os.environ[name] for name in ("FPATH", "NEBULA_TEST_ZSH_MODULE_DIR") if name in os.environ},
                 **env},
            start_new_session=True,
        )
        os.close(slave)

    def wait(self, marker: bytes) -> bytes:
        collected = b""
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.1)[0]:
                try:
                    block = os.read(self.master, 65536)
                except OSError:
                    break
                collected += block
                self.output += block
                if marker in collected:
                    return collected
        raise AssertionError(f"shell never emitted {marker!r}: {self.output!r}")

    def command(self, command: str, marker: bytes) -> bytes:
        os.write(self.master, command.encode("utf-8") + b"\n")
        return self.wait(marker)

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=5)
        os.close(self.master)


@unittest.skipUnless(os.name == "posix", "requires Unix PTYs")
class ShellIntegrationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="nebula-shell-test-")
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)

    def start(self, shell: str, rc: str = "", original_zdotdir: Path | None = None,
              user_files: bool = True, global_rcs: bool = False, profile: str = "",
              marker: bytes = b"\x1b]133;A\x07") -> ShellSession:
        program = shutil.which(shell)
        if not program:
            self.skipTest(f"{shell} is not installed; native CI must run this case")
        if shell == "bash":
            if sys.platform == "darwin":
                self.skipTest("macOS Bash keeps native login startup; this rc wrapper is Linux-only")
            (self.home / ".bashrc").write_text(rc, encoding="utf-8")
            args = ["--noprofile", "--rcfile", str(SCRIPTS / "bashrc"), "-i"]
            env = {}
        else:
            dotfiles = original_zdotdir or self.home
            dotfiles.mkdir(exist_ok=True)
            if user_files:
                (dotfiles / ".zshenv").write_text(
                    '[[ -n $NEBULA_TEST_ZSH_MODULE_DIR ]] && module_path=("$NEBULA_TEST_ZSH_MODULE_DIR" $module_path)\n'
                    "export NEBULA_PROFILE_TEST=env\n", encoding="utf-8")
                (dotfiles / ".zprofile").write_text("NEBULA_PROFILE_TEST+=:profile\n" + profile,
                                                    encoding="utf-8")
                (dotfiles / ".zshrc").write_text("NEBULA_PROFILE_TEST+=:rc\n" + rc, encoding="utf-8")
                (dotfiles / ".zlogin").write_text("NEBULA_PROFILE_TEST+=:login\n", encoding="utf-8")
            wrapper = self.home / "integration"
            wrapper.mkdir()
            for source, target in [("zshenv", ".zshenv"), ("zprofile", ".zprofile"), ("zshrc", ".zshrc")]:
                shutil.copyfile(SCRIPTS / source, wrapper / target)
            args = ["-l", "-i"] if global_rcs else ["-d", "-l", "-i"]
            env = {"ZDOTDIR": str(wrapper), "NEBULA_ZSH_INTEGRATION": str(wrapper),
                   "NEBULA_ZDOTDIR_WAS_SET": "1" if original_zdotdir else "0"}
            if original_zdotdir:
                env["NEBULA_ORIGINAL_ZDOTDIR"] = str(original_zdotdir)
        session = ShellSession(program, self.home, args, env)
        self.addCleanup(session.close)
        session.wait(marker)
        return session

    def check_protocol(self, shell: str) -> None:
        session = self.start(shell)
        output = session.command("(exit 7)", b"\x1b]133;D;7\x07")
        self.assertIn(b"\x1b]133;C\x07", output)
        directory = self.home / "目录 % # space"
        directory.mkdir()
        encoded = quote(str(directory), safe="/._~-").encode("ascii")
        session.command(f"cd '{directory}'", b"\x1b]7;file://localhost" + encoded + b"\x07")

    def test_bash_command_status_and_utf8_cwd(self) -> None:
        self.check_protocol("bash")

    def test_zsh_command_status_and_utf8_cwd(self) -> None:
        self.check_protocol("zsh")

    def test_zsh_loads_user_rcs_without_host_global_rcs(self) -> None:
        session = self.start("zsh")
        session.command('print -r -- "RCS=$options[rcs] GLOBAL_RCS=$options[globalrcs]"',
                        b"RCS=on GLOBAL_RCS=off")
        # Without global rc files no system compinit runs, so the bootstrap adds none.
        session.command('print -r -- "COMPINIT=${+functions[compdef]}_END"', b"COMPINIT=0_END")

    def zsh_color_state(self, rc: str) -> bytes:
        session = self.start("zsh", rc)
        return session.command(
            'print -r -- "COLORS=${CLICOLOR-unset}:${LSCOLORS-unset} ALIAS=${aliases[ls]-unset}"',
            b"\x1b]133;D;0\x07")

    def test_zsh_macos_color_defaults_do_not_replace_ls(self) -> None:
        output = self.zsh_color_state("OSTYPE=darwin\n")
        self.assertIn(b"COLORS=1:GxFxCxDxBxegedabagaced ALIAS=unset", output)

    def test_zsh_macos_preserves_user_color_settings_and_alias(self) -> None:
        output = self.zsh_color_state(
            "OSTYPE=darwin\nCLICOLOR=0\nLSCOLORS=exfxcxdxbxegedabagacad\nalias ls='ls -lah'\n")
        self.assertIn(b"COLORS=0:exfxcxdxbxegedabagacad ALIAS=ls -lah", output)

    def test_zsh_macos_preserves_explicit_empty_colors(self) -> None:
        output = self.zsh_color_state("OSTYPE=darwin\nCLICOLOR=''\nLSCOLORS=''\n")
        self.assertIn(b"COLORS=: ALIAS=unset", output)

    def test_zsh_macos_honors_no_color(self) -> None:
        output = self.zsh_color_state("OSTYPE=darwin\nNO_COLOR=1\n")
        self.assertIn(b"COLORS=unset:unset ALIAS=unset", output)

    def test_zsh_macos_dumb_terminal_does_not_enable_colors(self) -> None:
        output = self.zsh_color_state("OSTYPE=darwin\nTERM=dumb\n")
        self.assertIn(b"COLORS=unset:unset ALIAS=unset", output)

    def test_zsh_linux_keeps_color_policy_unchanged(self) -> None:
        output = self.zsh_color_state("OSTYPE=linux-gnu\n")
        self.assertIn(b"COLORS=unset:unset ALIAS=unset", output)

    def test_zsh_macos_preserves_ls_function(self) -> None:
        session = self.start("zsh", "OSTYPE=darwin\nls() { print -r -- CUSTOM_LS; }\n")
        output = session.command("ls", b"\x1b]133;D;0\x07")
        self.assertIn(b"CUSTOM_LS\r\n", output)

    @unittest.skipUnless(sys.platform == "darwin", "requires native BSD ls")
    def test_zsh_macos_bsd_ls_emits_color_in_a_pty(self) -> None:
        (self.home / "colored-directory").mkdir()
        session = self.start("zsh")
        output = session.command("/bin/ls -d colored-directory", b"\x1b]133;D;0\x07")
        self.assertRegex(output, rb"\x1b\[[0-9;]*mcolored-directory")

    def test_bash_preserves_prompt_command(self) -> None:
        session = self.start("bash", "PROMPT_COMMAND=\"printf 'USER_PROMPT\\\\n'\"\n")
        session.command("(exit 7)", b"\x1b]133;D;7\x07")
        self.assertIn(b"USER_PROMPT", session.output)

    def test_bash_does_not_replace_user_debug_trap(self) -> None:
        session = self.start("bash", "trap 'printf USER_DEBUG' DEBUG\n")
        session.command("trap -p DEBUG", b"trap -- 'printf USER_DEBUG' DEBUG")
        self.assertIn(b"USER_DEBUG", session.output)

    def test_bash_preserves_array_prompt_command(self) -> None:
        session = self.start("bash", "PROMPT_COMMAND=('printf FIRST_PROMPT' 'printf SECOND_PROMPT')\n")
        session.command("(exit 7)", b"\x1b]133;D;7\x07")
        self.assertIn(b"FIRST_PROMPT", session.output)
        self.assertIn(b"SECOND_PROMPT", session.output)

    def test_bash_preserves_exported_array_and_failure_status(self) -> None:
        session = self.start("bash", "declare -ax PROMPT_COMMAND=('printf ARRAY_STATUS=%s_END\\n \"$?\"')\n")
        output = session.command("(exit 7)", b"\x1b]133;D;7\x07")
        self.assertIn(b"ARRAY_STATUS=7_END", output)

    def test_bash_repeat_source_does_not_duplicate_startup_or_prompt_hooks(self) -> None:
        session = self.start("bash", "(( user_rc_count += 1 ))\nPROMPT_COMMAND=('printf USER_PROMPT')\n")
        session.command(f"source '{SCRIPTS / 'bashrc'}'", b"\x1b]133;D;0\x07")
        output = session.command('printf "RC_COUNT=%s_END\\n" "$user_rc_count"', b"\x1b]133;D;0\x07")
        self.assertIn(b"RC_COUNT=1_END", output)
        self.assertEqual(output.count(b"\x1b]133;D;0\x07"), 1)

    def test_bash_inherited_guard_does_not_call_missing_functions(self) -> None:
        session = self.start("bash", "export PROMPT_COMMAND='printf INHERITED_USER_PROMPT'\n")
        output = session.command("bash --noprofile --norc -i", b"INHERITED_USER_PROMPT")
        output += session.command("(exit 7)", b"INHERITED_USER_PROMPT")
        self.assertNotIn(b"command not found", output)

    def test_bash_remote_login_sources_profile_and_rc_once(self) -> None:
        if not shutil.which("bash") or sys.platform == "darwin":
            self.skipTest("requires modern Bash")
        for forwards in [False, True]:
            with self.subTest(profile_forwards_to_rc=forwards):
                (self.home / ".bash_profile").write_text(
                    "(( profile_count += 1 ))\n" + ('source "$HOME/.bashrc"\n' if forwards else ""), encoding="utf-8")
                (self.home / ".bashrc").write_text("(( user_rc_count += 1 ))\n", encoding="utf-8")
                session = ShellSession(shutil.which("bash"), self.home,
                                       ["--rcfile", str(SCRIPTS / "bashrc"), "-i"],
                                       {"PEBREL_REMOTE_LOGIN": "1"})
                self.addCleanup(session.close)
                session.wait(b"\x1b]133;A\x07")
                output = session.command('printf "STARTUP=%s:%s_END\\n" "$profile_count" "$user_rc_count"', b"\x1b]133;D;0\x07")
                self.assertIn(b"STARTUP=1:1_END", output)

    def test_readonly_prompt_does_not_emit_unmatched_command_start(self) -> None:
        if not shutil.which("bash"):
            self.skipTest("requires Bash")
        (self.home / ".bashrc").write_text("readonly PROMPT_COMMAND='printf READONLY_PROMPT'\n", encoding="utf-8")
        session = ShellSession(shutil.which("bash"), self.home,
                               ["--noprofile", "--rcfile", str(SCRIPTS / "bashrc"), "-i"], {})
        self.addCleanup(session.close)
        session.wait(b"READONLY_PROMPT")
        output = session.command("true", b"READONLY_PROMPT")
        self.assertNotIn(b"\x1b]133;C", output)

    def test_zsh_sources_login_files_and_restores_zdotdir(self) -> None:
        session = self.start("zsh")
        session.command('printf "PROFILE=%s ZDOTDIR=%s\\n" "$NEBULA_PROFILE_TEST" "${ZDOTDIR-unset}"',
                        b"PROFILE=env:profile:rc:login ZDOTDIR=unset")

    def test_zsh_preserves_custom_zdotdir(self) -> None:
        directory = self.home / "custom dotfiles"
        session = self.start("zsh", original_zdotdir=directory)
        session.command('printf "PROFILE=%s ZDOTDIR=%s_END\\n" "$NEBULA_PROFILE_TEST" "$ZDOTDIR"',
                        f"PROFILE=env:profile:rc:login ZDOTDIR={directory}_END".encode())

    def test_non_interactive_zsh_does_not_leak_the_bootstrap_zdotdir(self) -> None:
        # WSL shape: `wsl <cmd>` runs `zsh -c`, the host cannot know the guest ZDOTDIR,
        # and the user's ~/.zshenv moves ZDOTDIR (XDG layout).
        program = shutil.which("zsh")
        if not program:
            self.skipTest("zsh is not installed; native CI must run this case")
        dotfiles = self.home / ".config/zsh"
        dotfiles.mkdir(parents=True)
        (self.home / ".zshenv").write_text('export ZDOTDIR="$HOME/.config/zsh"\n', encoding="utf-8")
        (dotfiles / ".zshrc").write_text("NEBULA_PROFILE_TEST=rc\n", encoding="utf-8")
        wrapper = self.home / "integration"
        wrapper.mkdir()
        for source, target in [("zshenv", ".zshenv"), ("zprofile", ".zprofile"), ("zshrc", ".zshrc")]:
            shutil.copyfile(SCRIPTS / source, wrapper / target)
        nested = 'zsh -i -c \'print -r -- "RC=${NEBULA_PROFILE_TEST-unset} ZDOTDIR=$ZDOTDIR"\'; env'
        result = subprocess.run(
            [program, "-c", nested], cwd=self.home, capture_output=True, text=True, check=True,
            env={"PATH": os.environ["PATH"], "HOME": str(self.home), "TERM": "dumb",
                 "ZDOTDIR": str(wrapper), "NEBULA_ZSH_INTEGRATION": str(wrapper),
                 "NEBULA_ZDOTDIR_WAS_SET": "0"},
        )
        self.assertIn(f"RC=rc ZDOTDIR={dotfiles}", result.stdout)
        environment = dict(line.split("=", 1) for line in result.stdout.splitlines()[1:] if "=" in line)
        self.assertEqual(environment.get("ZDOTDIR"), str(dotfiles))
        self.assertFalse([name for name in environment if name.startswith("NEBULA_")])

    def test_zsh_user_rc_programs_do_not_inherit_bootstrap_variables(self) -> None:
        # A terminal multiplexer exec'd from a user rc file must not carry the bootstrap state along.
        session = self.start("zsh", 'print -r -- "RC_ENV=$(env | grep -cE "^NEBULA_(Z|ORIGINAL_Z)")_END"\n')
        self.assertIn(b"RC_ENV=0_END", session.output)
        session.command('print -r -- "LEFT=${+NEBULA_ZSH_INTEGRATION}_END"', b"LEFT=0_END")

    def require_ubuntu_global_zshrc(self) -> None:
        try:
            os_release = Path("/etc/os-release").read_text(encoding="utf-8")
            global_rc = Path("/etc/zsh/zshrc").read_text(encoding="utf-8")
        except OSError:
            self.skipTest("requires an Ubuntu global zshrc")
        if "ubuntu" not in os_release or "skip_global_compinit" not in global_rc:
            self.skipTest("requires an Ubuntu global zshrc")

    def test_zsh_global_compinit_keeps_its_dump_out_of_the_bootstrap(self) -> None:
        # Ubuntu's /etc/zsh/zshrc runs compinit while ZDOTDIR still names the bootstrap.
        self.require_ubuntu_global_zshrc()
        session = self.start("zsh", global_rcs=True, profile=SECURE_FPATH_PROFILE)
        session.command('print -r -- "COMPINIT=${+functions[compdef]}_END"', b"COMPINIT=1_END")
        self.assertEqual(sorted(path.name for path in (self.home / "integration").iterdir()),
                         [".zprofile", ".zshenv", ".zshrc"])
        self.assertTrue(list(self.home.glob(".zcompdump*")), "the dump belongs to the user")

    def test_zsh_profile_can_still_skip_the_global_compinit(self) -> None:
        # Ubuntu reads skip_global_compinit after ~/.zprofile on a login shell.
        self.require_ubuntu_global_zshrc()
        session = self.start("zsh", global_rcs=True, profile="skip_global_compinit=1\n")
        session.command('print -r -- "COMPINIT=${+functions[compdef]}_END"', b"COMPINIT=0_END")
        self.assertFalse(list(self.home.glob(".zcompdump*")))

    def test_zsh_offers_the_newuser_wizard_when_the_user_has_no_startup_files(self) -> None:
        # zsh's own check only saw the bootstrap's files.
        probe = subprocess.run([shutil.which("zsh") or "zsh", "-f", "-c",
                                "autoload -U +X zsh-newuser-install 2>/dev/null"],
                               env={"PATH": os.environ["PATH"], **{name: os.environ[name]
                                    for name in ("FPATH",) if name in os.environ}})
        if probe.returncode != 0 or os.geteuid() == 0:
            self.skipTest("requires zsh-newuser-install as a non-root user")
        self.start("zsh", user_files=False, marker=b"zsh-newuser-install")

    def test_zsh_newuser_check_happens_before_the_user_zshenv_moves_zdotdir(self) -> None:
        # Native zsh checks $HOME, where this ~/.zshenv exists; the XDG directory is empty.
        (self.home / ".config/zsh").mkdir(parents=True)
        (self.home / ".zshenv").write_text('export ZDOTDIR="$HOME/.config/zsh"\n', encoding="utf-8")
        session = self.start("zsh", user_files=False)
        self.assertNotIn(b"zsh-newuser-install", session.output)

    def test_zsh_preserves_precmd_hooks_after_failure(self) -> None:
        session = self.start("zsh", """
typeset -gi user_prompt_count=0
_user_precmd() {
    local status_code=$?
    (( ++user_prompt_count ))
    printf 'USER_PRECMD_%s_STATUS=%s_END\\n' "$user_prompt_count" "$status_code"
}
precmd_functions=(_user_precmd)
""")
        output = session.command("(exit 7)", b"USER_PRECMD_2_STATUS=7_END")
        self.assertIn(b"\x1b]133;D;7\x07", output)
        session.command("true", b"USER_PRECMD_3_STATUS=0_END")

    def test_wsl_guest_probe_reports_the_login_shell_and_bootstrap_readability(self) -> None:
        # The host runs this once per guest before handing zsh the bootstrap as
        # ZDOTDIR: `wsl.exe --exec sh -s` with the script on stdin and the bootstrap
        # directory translated into NEBULA_ZSH_INTEGRATION.
        import pwd

        def probe(bootstrap: str) -> dict[str, str]:
            result = subprocess.run(
                ["sh", "-s"], input=(SCRIPTS / "wsl-guest-probe.sh").read_text(encoding="utf-8"),
                capture_output=True, text=True, check=True, cwd=self.home,
                env={"PATH": os.environ["PATH"], "HOME": str(self.home),
                     "NEBULA_ZSH_INTEGRATION": bootstrap, "SHELL": "/bin/false"},
            )
            self.assertEqual(result.stderr, "")
            lines = result.stdout.splitlines()
            self.assertEqual(len(lines), 2, result.stdout)
            return dict(line.split("=", 1) for line in lines)

        wrapper = self.home / "integration dir"
        wrapper.mkdir()
        for source, target in [("zshenv", ".zshenv"), ("zprofile", ".zprofile"), ("zshrc", ".zshrc")]:
            shutil.copyfile(SCRIPTS / source, wrapper / target)
        answer = probe(str(wrapper))
        self.assertEqual(answer["bootstrap"], "readable")
        # The passwd entry, not $SHELL, names what `wsl.exe` starts for this user.
        self.assertEqual(answer["shell"], pwd.getpwuid(os.getuid()).pw_shell)

        (wrapper / ".zprofile").unlink()
        self.assertEqual(probe(str(wrapper))["bootstrap"], "unreadable")
        # Automount off or a failed `/p` translation leaves a path the guest cannot open.
        self.assertEqual(probe(str(self.home / "missing"))["bootstrap"], "unreadable")
        self.assertEqual(probe(r"C:\Users\me\AppData\Roaming\Pebrel\wsl-zsh")["bootstrap"], "unreadable")
        self.assertEqual(probe("")["bootstrap"], "unreadable")


if __name__ == "__main__":
    unittest.main()
