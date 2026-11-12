"""Compare the first local session snapshot from two published Pebrel ZIPs."""

import argparse
import json
import os
from pathlib import Path
import platform
import queue
import subprocess
import tempfile
import threading
import time

from scripts.conformance.harness import ConformanceContext, ResolvedApp, require

FRAME_LIMIT = 2 * 1024 * 1024


def first_frames(ctx):
    env = dict(os.environ, PEBREL_CONFIG_DIR=str(ctx.config_dir), NEBULA_CONFIG_DIR=str(ctx.config_dir))
    env.pop("PEBREL_RUNTIME_ENDPOINT", None)
    started = time.monotonic()
    process = subprocess.Popen(
        [str(ctx.app.executable), "ctl", "subscribe", "--timeout-ms", "5000"],
        cwd=ctx.work_dir, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    result = queue.Queue(maxsize=1)

    def read():
        try:
            frames = []
            for _ in range(2):
                line = process.stdout.readline(FRAME_LIMIT + 1)
                require(0 < len(line) < FRAME_LIMIT, "subscription returned no frame or exceeded the mobile limit")
                frames.append((json.loads(line), len(line), round((time.monotonic() - started) * 1000)))
            result.put(frames)
        except Exception as error:
            result.put(error)

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    try:
        try:
            value = result.get(timeout=10)
        except queue.Empty:
            raise RuntimeError("session subscription did not return both frames within 10 seconds") from None
        if isinstance(value, Exception):
            raise value
        return value
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
        reader.join(timeout=5)
        process.stdout.close()


def run(app, version, root, logs, *, prepare):
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "work", logs)
    if prepare:
        ctx.prepare()
    else:
        logs.mkdir(parents=True)
    try:
        ctx.start()
        snapshot = ctx.snapshot()
        ctx.refresh_targets(snapshot)
        require(ctx.description["app_version"] == version, "runtime version does not match the ZIP")
        (ack, _, ack_ms), (event, event_bytes, snapshot_ms) = first_frames(ctx)
        require(ack.get("ok") is True and ack["result"]["subscription_id"] > 0, "subscription was rejected")
        require(event.get("event") == "runtime.snapshot", "first event was not a session snapshot")
        data = event["data"]
        require(data["process_id"] == snapshot["process_id"], "snapshot came from another application instance")
        require(data["revision"] >= ack["result"]["current_revision"], "snapshot revision predates the acknowledgement")
        panes = [p["id"] for w in data["windows"] for t in w["tabs"] for p in t.get("panes", [])]
        require(ctx.pane_id in panes, "first snapshot omitted the active terminal pane")
        return {"status": "captured", "version": version, "ack_ms": ack_ms,
                "first_snapshot_ms": snapshot_ms, "first_snapshot_bytes": event_bytes,
                "panes": len(panes)}
    finally:
        ctx.stop(force=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old", type=Path, required=True)
    parser.add_argument("--new", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    require(os.name == "nt", "this comparison requires native Windows")
    report = {"windows": platform.version(), "cases": {}}
    with tempfile.TemporaryDirectory(prefix="pebrel-960-") as scratch:
        root = Path(scratch)
        old, new = ResolvedApp(args.old), ResolvedApp(args.new)
        try:
            for label, app, version, profile, prepare in [
                ("2.4.0-fresh", old, "2.4.0", "shared", True),
                ("2.5.3-after-2.4.0", new, "2.5.3", "shared", False),
                ("2.5.3-fresh", new, "2.5.3", "fresh", True),
            ]:
                try:
                    report["cases"][label] = run(app, version, root / profile, root / "logs" / label, prepare=prepare)
                except (OSError, RuntimeError, ValueError, KeyError) as error:
                    report["cases"][label] = {"status": "incomplete", "error": str(error)}
        finally:
            old.close()
            new.close()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(report, indent=2) + "\n"
    args.output.write_text(encoded, encoding="utf-8")
    print(encoded, end="", flush=True)
    return int(any(case["status"] != "captured" for case in report["cases"].values()))


if __name__ == "__main__":
    raise SystemExit(main())
