"""Compare two real Codex TUIs with their native history on an isolated Windows desktop."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import threading
import time
from unittest.mock import patch

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


class LocalModel(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_error(404)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        message = {"type": "message", "role": "assistant",
                   "content": [{"type": "output_text", "text": "Fixture reply."}]}
        events = [
            {"type": "response.created", "response": {"id": "resp_qa"}},
            {"type": "response.output_item.done", "item": {"id": "msg_qa", **message}},
            {"type": "response.completed", "response": {"id": "resp_qa", "usage": {
                "input_tokens": 0, "output_tokens": 0, "total_tokens": 0}}},
        ]
        body = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


def verify_standalone(executable, root):
    app = ResolvedApp(executable)
    ctx = ConformanceContext(app, "windows", root / "config", root / "same-cwd", root / "out")
    ctx.prepare()
    home = root / "provider"
    home.mkdir()
    server = ThreadingHTTPServer(("127.0.0.1", 0), LocalModel)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = (
        'model="gpt-5.4"\nmodel_provider="fixture"\ncheck_for_update_on_startup=false\n'
        '[tui]\nstatus_line=["session-id"]\n'
        '[model_providers.fixture]\nname="fixture"\nwire_api="responses"\n'
        'requires_openai_auth=false\nsupports_websockets=false\n'
        f'base_url="http://127.0.0.1:{server.server_port}/v1"\n'
        f'[projects.{json.dumps(str(ctx.work_dir))}]\ntrust_level="trusted"\n'
        '[windows]\nsandbox="unelevated"\n'
    )
    (home / "config.toml").write_text(config, encoding="utf-8")
    (home / "auth.json").write_text('{"OPENAI_API_KEY":"fixture"}', encoding="utf-8")
    names = ["qa-alpha", "qa-bravo"]
    shell = {"kind": "shell", "name": "Command Prompt", "program": os.environ["ComSpec"], "args": ["/D"]}
    tabs = [{"cwd": str(ctx.work_dir), "custom_name": name, "active_pane": 0,
             "launch": shell} for name in names]
    ctx.session_file.write_text(json.dumps({"version": 4, "active_tab": 0, "tabs": tabs,
                                            "clean_exit": True}), encoding="utf-8")
    try:
        with patch.dict(os.environ, CODEX_HOME=str(home), OPENAI_API_KEY="fixture"):
            ctx.start(explicit_working_directory=False)
            ctx.refresh_targets()
            panes = {t["label"]: t["panes"][0]["id"] for t in ctx.snapshot()["windows"][0]["tabs"]}
            assert set(panes) == set(names)
            def native_history():
                try:
                    records = (home / "history.jsonl").read_text(encoding="utf-8").splitlines()
                    return {r["text"]: r["session_id"] for r in map(json.loads, records)}
                except (FileNotFoundError, PermissionError, json.JSONDecodeError):
                    return {}

            for name, pane in panes.items():
                ctx.poll(lambda: ctx.read(pane)["text"].strip(), bool, "missing command prompt")
                codex = Path(os.environ["PEBREL_QA_CODEX_BIN"]).resolve(strict=True)
                ctx.prompt(f'set "CODEX_HOME={home}" && "{codex}" --no-daemon {name}', pane)
                ctx.poll(native_history, lambda rows: name in rows,
                         "Codex did not record the test prompt in a native thread", timeout=35)
            history = native_history()
            expected = {name: history[name] for name in names}
            assert len(set(expected.values())) == len(names)
            for name, pane in panes.items():
                other = next(i for n, i in expected.items() if n != name)
                row = re.compile(r"(?m)^\s*" + re.escape(expected[name]) + r"\s*$")
                screen = ctx.poll(lambda: ctx.read(pane)["text"], row.search,
                                  "native thread did not appear in its own status line", timeout=30)
                assert other not in screen
            print("Both Codex status rows exactly matched their native threads.", flush=True)

            def saved_ids():
                try:
                    snap = json.loads(ctx.session_file.read_text(encoding="utf-8"))
                except (PermissionError, FileNotFoundError) as error:
                    return {"snapshot_read_error": str(error)}
                return {t["custom_name"]: (t.get("layout", {}).get("agent") or {}).get("session_id")
                        for t in snap["tabs"]}

            ctx.poll(saved_ids, lambda ids: ids == expected,
                     "the Windows snapshot did not identify both standalone threads", timeout=65)
            print("Both Windows snapshot IDs matched their native Codex threads.", flush=True)

            def tab_and_pane():
                return {tab["label"]: (tab["tab_id"], tab["panes"][0]["id"])
                        for tab in ctx.snapshot()["windows"][0]["tabs"]}

            initial = tab_and_pane()
            active = panes["qa-alpha"]
            first = expected["qa-alpha"]
            ctx.prompt("/new", active)
            thread_pattern = re.compile(r"[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}", re.I)

            def footer():
                lines = [line.strip() for line in ctx.read(active)["text"].splitlines() if line.strip()]
                return next((line for line in reversed(lines[-4:]) if thread_pattern.fullmatch(line)), None)

            ctx.poll(footer, lambda value: value and value != first,
                     "Codex did not display a fresh thread after /new", timeout=30)
            ctx.prompt("qa-after-new", active)
            second = ctx.poll(native_history, lambda rows: "qa-after-new" in rows,
                              "no native message after /new", timeout=35)["qa-after-new"]
            assert second != first and footer() == second
            assert tab_and_pane() == initial
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                observed = saved_ids()
                if observed.get("qa-alpha") == second and observed.get("qa-bravo") == expected["qa-bravo"]:
                    break
                time.sleep(0.3)
            assert observed.get("qa-bravo") == expected["qa-bravo"]
            target = observed.get("qa-alpha")
            outcome = "current" if target == second else ("previous" if target == first else "missing-or-other")
            print("Windows /new: tab and pane IDs unchanged, native thread changed, persisted target=" + outcome,
                  flush=True)
            assert outcome == "current", "did not checkpoint the current native Codex thread"
    finally:
        ctx.stop(force=True)
        app.close()
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    from tempfile import TemporaryDirectory
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Requires an isolated hosted Windows runner")
    with TemporaryDirectory(prefix="pebrel-checkpoint-", ignore_cleanup_errors=True) as directory:
        verify_standalone(sys.argv[1], Path(directory))
