"""Cold-restart two same-directory native CLIs in a disposable Linux desktop profile."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import shutil
import sys
from tempfile import TemporaryDirectory
import time
from unittest.mock import patch
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from conformance.agent_recovery.model import local_model
from conformance.agent_recovery.providers import PROFILES, command_for, configure, provider_confirmed
from conformance.harness import ConformanceContext, ResolvedApp


def marker():
    return "pebrel-qa-" + uuid4().hex[:16]


def shell_environment(home):
    env = {name: os.environ[name] for name in
           ("PATH", "DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR")
           if name in os.environ}
    env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
               XDG_DATA_HOME=str(home / ".local/share"), XDG_STATE_HOME=str(home / ".local/state"),
               XDG_CACHE_HOME=str(home / ".cache"), SHELL="/bin/bash", LANG="C.UTF-8",
               TERM="xterm-256color")
    return env


def prepare_split(ctx, agent):
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    content = settings.read_text(encoding="utf-8")
    assert "resume_ai=0" in content
    settings.write_text(content.replace("resume_ai=0", "resume_ai=1") + f"ai_hooks_{agent}=1\n",
                        encoding="utf-8")
    shell = {"kind": "shell", "name": "Bash", "program": "/bin/bash", "args": []}
    leaf = {"kind": "pane", "cwd": str(ctx.work_dir), "launch": shell}
    layout = {"kind": "split", "axis": "top_bottom", "ratio_permille": 500, "first": leaf, "second": leaf}
    tab = {"cwd": str(ctx.work_dir), "launch": shell, "layout": layout}
    ctx.session_file.write_text(json.dumps({"version": 4, "tabs": [tab], "active_tab": 0,
                                            "clean_exit": True}), encoding="utf-8")


def panes(ctx):
    ctx.refresh_targets()
    tabs = ctx.snapshot()["windows"][0]["tabs"]
    assert len(tabs) == 1 and len(tabs[0]["panes"]) == 2
    tree = tabs[0]["layout"]
    assert tree["direction"] == "top_bottom"
    return [tree[side]["pane_id"] for side in ("first", "second")]


def runtime_pane(ctx, pane):
    tab = ctx.tab_for_pane(ctx.snapshot(), pane)
    return next(row for row in tab["panes"] if row["id"] == pane)


def live_id(ctx, pane, source):
    agent = runtime_pane(ctx, pane).get("agent") or {}
    return agent.get("session_id") if agent.get("kind") == source else None


def saved_id(ctx, side, source):
    try:
        with ctx.session_file.open(encoding="utf-8") as stream:
            tree = json.load(stream)["tabs"][0]["layout"]
        agent = tree[side].get("agent") or {}
        return agent.get("session_id") if agent.get("source") == source else None
    except (ValueError, OSError, KeyError, IndexError):
        return None


def wait_for_editor(ctx, pane, agent):
    if agent == "claude":
        # SessionStart can arrive while the grid still shows the shell command.
        ctx.poll(lambda: any(row.strip() == "❯" for row in ctx.read(pane)["text"].splitlines()[-8:]),
                 bool, "Claude did not display its empty composer", timeout=45)
    elif agent == "codebuddy":
        def ready():
            lines = ctx.read(pane)["text"].splitlines()[-8:]
            return any(line.strip() == ">" for line in lines) and any(
                "for shortcuts" in line for line in lines
            )

        ctx.poll(ready, bool, "CodeBuddy editor is not ready", timeout=30)


def submit_turn(ctx, pane, agent, text):
    if agent != "codebuddy":
        ctx.prompt(text, pane)
        return
    ctx.paste(text, pane, submit=False)

    def composer_contains_turn():
        lines = ctx.read(pane)["text"].splitlines()[-8:]
        return any(line.strip() == "> " + text for line in lines)

    ctx.poll(composer_contains_turn, bool, "CodeBuddy did not display the input before submission")
    ctx.send_key("enter", pane)


def codebuddy_project_prompt(ctx, screen):
    return (f"> 1. trust folder only ({ctx.work_dir.name.lower()})" in screen
            and "enter to confirm" in screen)


def confirm_codebuddy_restore(ctx, targets, expected):
    accepted = set()

    def confirmed():
        ids = [live_id(ctx, pane, "codebuddy") for pane in targets]
        for pane, session, target_id in zip(targets, ids, expected):
            if session == target_id or pane in accepted:
                continue
            if codebuddy_project_prompt(ctx, ctx.read(pane)["text"].lower()):
                ctx.send_key("enter", pane)
                accepted.add(pane)
        return ids

    ctx.poll(confirmed, lambda ids: ids == expected, "CodeBuddy did not restore both native IDs", timeout=60)


def accept_project_prompt(ctx, pane, agent):
    if agent not in ("grok", "codebuddy"):
        return
    # Only the disposable project is approved. Wait for the native prompt or
    # an input footer if a sibling has already trusted this directory.
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        screen = ctx.read(pane)["text"].lower()
        if "yes, proceed" in screen and "contents of this directory" in screen:
            ctx.prompt("y", pane, submit=False)
            return
        if agent == "codebuddy" and codebuddy_project_prompt(ctx, screen):
            ctx.send_key("enter", pane)
            return
        if "for shortcuts" in screen or "ctrl+" in screen:
            return
        time.sleep(0.2)


def run_agent(agent, executable, rounds):
    entries = [agent, "cbc"] if agent == "codebuddy" else [agent]
    for binary in entries:
        if not shutil.which(binary):
            raise RuntimeError(f"{binary} is missing from PATH")
    with ExitStack() as cleanup:
        test = Path(cleanup.enter_context(TemporaryDirectory(prefix="pebrel-native-recovery-")))
        home = test / "home"
        app = ResolvedApp(executable)
        cleanup.callback(app.close)
        ctx = ConformanceContext(app, "linux", test / "config", test / "project", test / "artifacts")
        prepare_split(ctx, agent)
        model = cleanup.enter_context(local_model())
        env = shell_environment(home)
        env.update(configure(agent, home, ctx.work_dir, f"http://127.0.0.1:{model.server_port}"))
        cleanup.enter_context(patch.dict(os.environ, env, clear=True))
        cleanup.callback(ctx.stop, force=True)
        ctx.start(explicit_working_directory=False)
        first = [marker(), marker()]
        expected = []
        sides = ("first", "second")
        for index, pane in enumerate(panes(ctx)):
            ctx.poll(lambda: "$" in ctx.read(pane)["text"], bool, "shell is not ready")
            command = command_for(agent, index)
            if agent in ("pi", "omp"):
                command += " " + first[index]
            ctx.prompt(command, pane)
            # A source-only live status identifies the TUI before any ID exists.
            def running():
                return (runtime_pane(ctx, pane).get("agent") or {}).get("kind") == agent
            ctx.poll(running, bool, f"{agent} did not start", timeout=30)
            accept_project_prompt(ctx, pane, agent)
            wait_for_editor(ctx, pane, agent)
            if agent not in ("pi", "omp"):
                submit_turn(ctx, pane, agent, first[index])
            ctx.poll(lambda: any(first[index] in turn for turn in model.turns), bool,
                     f"{agent} initial turn did not reach the local model", timeout=40)
            def initial_id():
                native_id = saved_id(ctx, sides[index], agent)
                return native_id if provider_confirmed(home, agent, native_id, first[index]) else None
            expected.append(ctx.poll(initial_id, bool, f"{agent} native conversation did not match", timeout=40))
        assert expected[0] != expected[1]
        for round_index in range(rounds):
            ctx.restart()
            targets = panes(ctx)
            if agent == "codebuddy":
                confirm_codebuddy_restore(ctx, targets, expected)
            for index, pane in enumerate(targets):
                ctx.poll(lambda: live_id(ctx, pane, agent) == expected[index], bool,
                         f"{agent} did not acknowledge its saved ID in {sides[index]}", timeout=45)
                wait_for_editor(ctx, pane, agent)
                after = marker()
                submit_turn(ctx, pane, agent, after)
                turns = ctx.poll(lambda: [turn for turn in model.turns if after in turn], bool,
                                 f"{agent} did not submit the restored turn in {sides[index]}", timeout=40)
                assert any(first[index] in turn and first[1-index] not in turn for turn in turns), (
                    f"{agent} restored turn has the wrong history in {sides[index]}"
                )
                ctx.poll(lambda: saved_id(ctx, sides[index], agent) == expected[index]
                         and provider_confirmed(home, agent, expected[index], after), bool,
                         "new message did not enter the same native conversation", timeout=40)
            print(f"{agent} round {round_index + 1}/{rounds} passed for both panes", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, help="Packaged Pebrel executable")
    parser.add_argument("--agent", choices=PROFILES, action="append", help="Repeat to select several CLIs")
    parser.add_argument("--rounds", type=int, default=3)
    args = parser.parse_args()
    if not sys.platform.startswith("linux") or not os.environ.get("DISPLAY") or args.rounds < 1:
        parser.error("requires a Linux desktop and at least one cold restart")
    failures = []
    for source in args.agent or list(PROFILES):
        try:
            run_agent(source, args.app, args.rounds)
        except (OSError, RuntimeError, AssertionError) as error:
            failures.append(source)
            print(f"{source}: {type(error).__name__}: {error}", file=sys.stderr)
    sys.exit(bool(failures))
