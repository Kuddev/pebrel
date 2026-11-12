"""Compare the first local session snapshot from two published Pebrel ZIPs."""

import argparse
import json
import os
from pathlib import Path
import platform
import queue
import re
import subprocess
import tempfile
import threading
import time

from scripts.conformance.harness import ConformanceContext, ResolvedApp, RuntimeClient, require

FRAME_LIMIT = 2 * 1024 * 1024


def probe_environment(ctx):
    env = dict(os.environ, PEBREL_CONFIG_DIR=str(ctx.config_dir), NEBULA_CONFIG_DIR=str(ctx.config_dir))
    for name in ("PEBREL_RUNTIME_ENDPOINT", "PEBREL_PROCESS_ID", "PEBREL_PANE_ID",
                 "NEBULA_PANE_ID", "PEBREL_CONFIG_FILE", "NEBULA_CONFIG_FILE",
                 "PEBREL_GPUI_CONFIG", "NEBULA_GPUI_CONFIG"):
        env.pop(name, None)
    return env


def start_without_snapshot(ctx):
    # The shared harness uses runtime.snapshot as a readiness check. Here that
    # would warm the publisher before the first subscriber reaches it.
    ctx._log_handle = (ctx.artifact_dir / "pebrel.log").open("wb")
    ctx.process = ctx._spawn_process(
        [str(ctx.app.executable), "--working-directory", str(ctx.work_dir)], probe_environment(ctx),
    )

    def discover():
        require(ctx.process.poll() is None, "Pebrel exited during cold startup")
        if not ctx.port_file.is_file():
            return None
        try:
            client = RuntimeClient.from_port_file(ctx.port_file)
            description = client.request("runtime.describe", timeout=1.0)["result"]
        except (OSError, RuntimeError):
            return None
        return client, description

    ctx.client, ctx.description = ctx.poll(
        discover, lambda value: value is not None, "runtime description unavailable", timeout=20,
    )


def first_frames(ctx):
    env = probe_environment(ctx)
    started = time.monotonic()
    process = subprocess.Popen(
        [str(ctx.app.executable), "ctl", "subscribe", "--timeout-ms", "5000"],
        cwd=ctx.work_dir, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    result = queue.Queue(maxsize=3)

    def read():
        try:
            for _ in range(2):
                line = process.stdout.readline(FRAME_LIMIT + 1)
                require(0 < len(line) < FRAME_LIMIT, "subscription returned no frame or exceeded the mobile limit")
                result.put((json.loads(line), len(line), round((time.monotonic() - started) * 1000)))
        except Exception as error:
            result.put(error)

    def next_frame(timeout):
        try:
            value = result.get(timeout=timeout)
        except queue.Empty:
            return None
        if isinstance(value, Exception):
            raise value
        return value

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    try:
        ack = next_frame(6)
        require(ack is not None, "session subscription did not acknowledge within 6 seconds")
        event = next_frame(6)
        needed_snapshot = event is None
        snapshot = ctx.snapshot()
        if needed_snapshot:
            event = next_frame(3)
        require(event is not None, "subscriber did not receive a session after an explicit snapshot")
        return ack, event, snapshot, needed_snapshot
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
        start_without_snapshot(ctx)
        reported_version = ctx.description["app_version"]
        expected = re.fullmatch(r"(\d+\.\d+\.\d+)(?: \([0-9a-f]{7,40}\))?", reported_version)
        require(expected and expected[1] == version, f"unexpected runtime version: {reported_version}")
        (ack, _, ack_ms), (event, event_bytes, snapshot_ms), snapshot, needed_snapshot = first_frames(ctx)
        ctx.refresh_targets(snapshot)
        require(ack.get("ok") is True and ack["result"]["subscription_id"] > 0, "subscription was rejected")
        require(event.get("event") == "runtime.snapshot", "first event was not a session snapshot")
        data = event["data"]
        require(data["process_id"] == snapshot["process_id"], "snapshot came from another application instance")
        require(data["revision"] >= ack["result"]["current_revision"], "snapshot revision predates the acknowledgement")
        panes = [p["id"] for w in data["windows"] for t in w["tabs"] for p in t.get("panes", [])]
        require(ctx.pane_id in panes, "first snapshot omitted the active terminal pane")
        return {"status": "captured", "version": reported_version, "ack_ms": ack_ms,
                "ack_revision": ack["result"]["current_revision"],
                "needed_explicit_snapshot": needed_snapshot,
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
