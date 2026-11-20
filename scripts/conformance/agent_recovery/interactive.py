"""Exercise a signed-in CLI on a disposable Linux desktop with two independent chats."""
import argparse
from contextlib import ExitStack
import os
from pathlib import Path
import shutil
import sys
import tomllib
from tempfile import TemporaryDirectory
from unittest.mock import patch
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from conformance.agent_recovery.run import live_id, panes, prepare_split, runtime_pane, saved_id, shell_environment, wait_for_editor
from conformance.harness import ConformanceContext, ResolvedApp


def token():
    return uuid4().hex[:12]


def check_agent(ctx, agent, commands, rounds, timeout):
    ids = []
    secrets = [token(), token()]
    sides = ("first", "second")
    ctx.start(explicit_working_directory=False)
    for index, pane in enumerate(panes(ctx)):
        ctx.poll(lambda: "$" in ctx.read(pane)["text"], bool, "test shell is not ready")
        ctx.prompt(commands[index], pane)
        # An external login, trust prompt or source-only identity cannot satisfy
        # the native-ID check. Leave time for interactive setup on this desktop.
        def ready():
            row = runtime_pane(ctx, pane)
            status = row.get("agent") or {}
            return status.get("kind") == agent and row.get("task_state") in ("idle", "finished", "waiting_input")
        ctx.poll(ready, bool, f"{agent} is not ready for a user turn", timeout=timeout)
        wait_for_editor(ctx, pane, agent)
        prefix = token()
        secret = secrets[index]
        prompt = (f"Our test word is {secret}. Keep it in this conversation only. Do not use tools. "
                  f"Reply with {prefix}, a slash, and the test word joined with no spaces.")
        ctx.prompt(prompt, pane)
        ctx.poll(lambda: f"{prefix}/{secret}" in ctx.read(pane)["text"], bool,
                 f"{agent} did not answer the first challenge", timeout=timeout)
        ids.append(ctx.poll(lambda: saved_id(ctx, sides[index], agent), bool,
                            f"{agent} did not save the conversation ID", timeout=timeout))
    assert ids[0] != ids[1], "both split leaves saved the same conversation ID"
    for attempt in range(rounds):
        ctx.restart()
        for index, pane in enumerate(panes(ctx)):
            # Each challenge prefix is new after restart. Terminal scrollback
            # cannot supply this answer, and the question omits both secrets.
            prefix = token()
            ctx.poll(lambda: live_id(ctx, pane, agent) == ids[index], bool,
                     "provider did not acknowledge the same ID", timeout=timeout)
            wait_for_editor(ctx, pane, agent)
            ctx.prompt(f"Without tools, recall our test word. Reply with {prefix}, a slash, "
                       "and that word joined with no spaces.", pane)
            ctx.poll(lambda: f"{prefix}/{secrets[index]}" in ctx.read(pane)["text"], bool,
                     f"{agent} did not recall its own conversation", timeout=timeout)
            assert f"{prefix}/{secrets[1-index]}" not in ctx.read(pane)["text"]
            assert saved_id(ctx, sides[index], agent) == ids[index]
        print(f"{agent} round {attempt + 1}/{rounds} continued both chats", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    entries_by_source = tomllib.loads(Path(__file__).with_name("entries.toml").read_text(encoding="utf-8"))
    parser.add_argument("--app", required=True)
    parser.add_argument("--source", required=True, choices=entries_by_source)
    parser.add_argument("--test-home", required=True, type=Path, help="Dedicated signed-in test home")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--timeout", type=int, default=90)
    args = parser.parse_args()
    home = args.test_home.resolve()
    if not sys.platform.startswith("linux") or not os.environ.get("DISPLAY"):
        parser.error("requires a Linux desktop")
    if not home.is_dir() or home == Path.home().resolve():
        parser.error("supply a separate test home containing the provider's test profile")
    if args.rounds < 1 or args.timeout < 1:
        parser.error("rounds and timeout must be positive")
    commands = entries_by_source[args.source]
    for entry in commands:
        if not shutil.which(entry):
            parser.error(f"{entry} is missing from PATH")
    with ExitStack() as cleanup:
        test = Path(cleanup.enter_context(TemporaryDirectory(prefix="pebrel-agent-recall-")))
        app = ResolvedApp(args.app)
        cleanup.callback(app.close)
        ctx = ConformanceContext(app, "linux", test / "config", test / "project", test / "artifacts")
        prepare_split(ctx, args.source)
        cleanup.enter_context(patch.dict(os.environ, shell_environment(home), clear=True))
        cleanup.callback(ctx.stop, force=True)
        entries = commands * 2 if len(commands) == 1 else commands
        check_agent(ctx, args.source, entries, args.rounds, args.timeout)


if __name__ == "__main__":
    main()
