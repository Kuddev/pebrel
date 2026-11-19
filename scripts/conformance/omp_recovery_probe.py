"""Reopen a real OMP 18.22.2 conversation after terminating its Pebrel workspace."""
import json
import os
import re
import shutil
import subprocess
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


SID = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c"
MARKER = "PEBREL-OMP-RESTORED-1927"


def write_conversation(path, cwd):
    stamp = "2026-11-19T06:41:00.000Z"
    title = {"type": "title", "v": 1, "title": "Recovery check", "updatedAt": stamp, "pad": ""}
    title["pad"] = " " * (255 - len(json.dumps(title, separators=(",", ":"))))
    records = [
        title, {"type": "session", "version": 3, "id": SID, "cwd": str(cwd), "timestamp": stamp},
        {"type": "message", "id": "0033aabb", "parentId": None, "timestamp": stamp,
         "message": {"role": "user", "content": [{"type": "text", "text": MARKER}],
                     "timestamp": 1795068000000}},
    ]
    path.parent.mkdir()
    path.write_text("".join(json.dumps(item, separators=(",", ":")) + "\n" for item in records),
                    encoding="utf-8", newline="\n")


def run(app, root):
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "project",
                             root / "evidence", startup_timeout=40)
    ctx.prepare()
    settings = ctx.config_dir / "pebrel_settings.txt"
    settings.write_text(settings.read_text().replace("resume_ai=0", "resume_ai=1")
                        + "ai_hooks_omp=1\nshell=pwsh\n", encoding="utf-8")
    profile = root / "fixture-profile"
    agent_dir = profile / ".omp" / "agent"
    agent_dir.mkdir(parents=True)
    os.environ.update(USERPROFILE=str(profile), PI_CODING_AGENT_DIR=str(agent_dir),
                      OMP_SKIP_VERSION_CHECK="1", PI_OFFLINE="1",
                      OPENAI_API_KEY="fixture-only", OPENAI_BASE_URL="http://127.0.0.1:9/v1")
    source = root / "custom sessions" / ("2026-11-19T06-41-00-000Z_" + SID + ".jsonl")
    write_conversation(source, ctx.work_dir)

    def live_agent():
        tab = ctx.tab_for_pane(ctx.snapshot(), ctx.pane_id)
        pane = next(p for p in tab["panes"] if p["id"] == ctx.pane_id)
        agent = pane.get("agent") or {}
        return agent.get("kind"), agent.get("session_id"), agent.get("hook_seen")

    def saved_agent():
        try:
            data = json.loads(ctx.session_file.read_text(encoding="utf-8"))
            return data["tabs"][0]["layout"].get("agent", {})
        except (OSError, ValueError, KeyError, IndexError):
            return {}

    def verify_live():
        ctx.poll(live_agent, lambda agent: agent == ("omp", SID, True),
                 "the running OMP process did not confirm the saved session", timeout=50)
        ctx.poll(lambda: MARKER in ctx.read(lines=100)["text"], bool,
                 "the transcript message was missing from the live terminal", timeout=20)

    binary = shutil.which("omp")
    if not binary:
        raise RuntimeError("Verified OMP executable is missing from the ordinary user's PATH")
    version = subprocess.run([binary, "--version"], capture_output=True, text=True,
                             timeout=25, cwd=ctx.work_dir, errors="replace")
    print("OMP version check:", version.returncode,
          (version.stdout + version.stderr).strip()[:350], flush=True)
    version.check_returncode()
    try:
        ctx.start()
        ctx.refresh_targets()
        extension = agent_dir / "extensions" / "pebrel.ts"
        ctx.poll(extension.is_file, bool, "Pebrel did not install the OMP integration", timeout=20)
        ctx.prompt("Write-Output ('PEBREL-PWSH-' + $PSVersionTable.PSVersion.Major)")
        ctx.poll(lambda: "PEBREL-PWSH-7" in ctx.read()["text"], bool,
                 "the pane did not run PowerShell 7")
        ctx.prompt("omp --resume '" + str(source).replace("'", "''") + "'")
        verify_live()
        ctx.poll(saved_agent, lambda a: a.get("session_id") == SID
                 and Path(a.get("session_file") or "") == source,
                 "autosave did not retain the native file and identity", timeout=25)
        print("PowerShell 7 displayed the message and autosave recorded its exact transcript.", flush=True)
        first_pid = ctx.process.pid
        ctx.restart()
        assert ctx.process.pid != first_pid
        ctx.refresh_targets()
        verify_live()
        print("A fresh Pebrel process restored OMP's confirmed ID and the stored message.", flush=True)
    except Exception:
        if ctx.client and ctx.pane_id:
            screen = ctx.read(lines=45).get("text", "")
            screen = re.sub(r"(?i)(api[_ -]?key|token)(\s*[:=]\s*)[^\s]+", r"\1\2[redacted]", screen)
            print("Isolated test pane:", screen[-3000:], flush=True)
            processes = ctx.api("pane.procs", {"window_id": ctx.window_id, "pane_id": ctx.pane_id},
                                allow_error=True)
            rows = (processes.get("result") or {}).get("processes") or []
            print("Pane processes:", [(row.get("depth"), Path(row.get("executable", "")).name)
                                       for row in rows], flush=True)
        raise
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Run on an isolated hosted Windows runner")
    with tempfile.TemporaryDirectory(prefix="pebrel-omp-", ignore_cleanup_errors=True) as directory:
        run(ResolvedApp(sys.argv[1]), Path(directory))
