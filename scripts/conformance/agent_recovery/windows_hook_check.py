"""Observe hook delivery in the disposable Windows CodeBuddy fixture."""
import json
from pathlib import Path
import subprocess


def attach(ctx, home):
    settings = home / ".codebuddy" / "settings.json"
    ctx.poll(settings.exists, bool, "Pebrel did not install CodeBuddy hooks")
    config = json.loads(settings.read_text(encoding="utf-8-sig"))
    observer = home / "hook-observer.cjs"
    observer.write_text(r"""
const fs = require("node:fs");
const chunks = [];
process.stdin.on("data", chunk => chunks.push(chunk));
process.stdin.on("end", () => {
    const bytes = Buffer.concat(chunks);
    let payload;
    try { payload = JSON.parse(bytes.toString("utf8")); } catch {}
    const info = { event: process.argv[2], bytes: bytes.length,
        pane: Boolean(process.env.PEBREL_PANE_ID || process.env.NEBULA_PANE_ID),
        pipe: Boolean(process.env.PEBREL_NOTIFY_PIPE || process.env.NEBULA_NOTIFY_PIPE),
        sessionId: Boolean(payload?.session_id || payload?.sessionId) };
    fs.appendFileSync(process.argv[3], JSON.stringify(info) + "\n");
});
""", encoding="utf-8")
    output = home / "provider-events.jsonl"
    for event in ("SessionStart", "UserPromptSubmit", "Stop"):
        entries = config["hooks"][event]
        cmd = f'node "{observer.as_posix()}" {event} "{output.as_posix()}"'
        entries.append({"hooks": [{"type": "command", "command": cmd}]})
    settings.write_text(json.dumps(config), encoding="utf-8")
    control = config["hooks"]["SessionStart"][0]["hooks"][0]["command"]
    sample = b'{"hook_event_name":"SessionStart","session_id":"control"}'
    subprocess.run(control, input=sample, check=True, timeout=15)


def results(home):
    for name in ("provider-events.jsonl", "helper-outcomes.jsonl"):
        path = Path(home) / name
        if path.exists():
            rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]
            print(name, rows[-12:], flush=True)
        else:
            print(name, "no records", flush=True)
