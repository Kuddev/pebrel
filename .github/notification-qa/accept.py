"""Verify the current bundled product through macOS's normal notification UI."""
from pathlib import Path
import argparse
import json
import os
import signal
import subprocess
import sys
import time

sys.path.insert(0, str(Path.cwd() / "scripts"))
from conformance.harness import ConformanceContext, ResolvedApp, RuntimeClient, require
from conformance.macos_launch import owned_processes


def nodes(node):
    yield node
    for child in node.get("children", []):
        yield from nodes(child)


def text(node):
    return " | ".join(str(node.get(k, "")) for k in ("AXTitle", "AXDescription", "AXValue"))


def run():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source", required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    report = {"status": "failed", "source": args.source, "permission_actions": []}
    app = ResolvedApp(args.app.resolve())
    private = Path(os.environ["RUNNER_TEMP"]) / "notification-private"
    ctx = ConformanceContext(app, "macos", private / "config", private / "work", private / "logs")
    ctx.prepare()
    launcher = None
    def write(name, value):
        (out / name).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    def snapshot(name):
        value = json.loads(subprocess.check_output([str(args.ui), "snapshot"], text=True, timeout=25))
        write(name + ".json", value)
        return value
    def capture(name):
        subprocess.run(["/usr/sbin/screencapture", "-x", str(out / (name + ".png"))], check=True, timeout=15)
    def find_notice(value, marker):
        for process in value["apps"]:
            if "notification" not in (process["name"] + process["bundle"]).lower():
                continue
            for node in nodes(process["tree"]):
                if marker in text(node) and node.get("rect"):
                    return process, node
        return None
    def permit(value):
        # Only interact with the observed Pebrel permission prompt on this fresh runner.
        for process in value["apps"]:
            if "notification" not in (process["name"] + process["bundle"]).lower():
                continue
            if "Pebrel" not in json.dumps(process["tree"]):
                continue
            for node in nodes(process["tree"]):
                if "AXPress" not in node.get("actions", []):
                    continue
                for label in ("Allow", "Options"):
                    if label not in [node.get(k) for k in ("AXTitle", "AXDescription", "AXValue")]:
                        continue
                    capture("permission-before-" + str(len(report["permission_actions"])))
                    result = subprocess.run([str(args.ui), "press", str(process["pid"]), node["path"], label], timeout=10)
                    report["permission_actions"].append({"label": label, "result": result.returncode})
                    require(result.returncode == 0, "observed notification permission control changed")
                    return True
        return False
    try:
        require(subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() == args.source,
                "checkout differs from candidate")
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(args.app)], check=True)
        launcher = subprocess.Popen(["/usr/bin/open", "-n", "-W", "-a", str(args.app.resolve()),
                                     "--env", f"PEBREL_CONFIG_DIR={ctx.config_dir}",
                                     "--env", f"NEBULA_CONFIG_DIR={ctx.config_dir}"],
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if ctx.port_file.exists():
                try:
                    client = RuntimeClient.from_port_file(ctx.port_file, timeout=2)
                    state = client.request("runtime.snapshot")["result"]
                    require(state["process_id"] in owned_processes(app.executable), "unexpected application owner")
                    ctx.client = client
                    ctx.refresh_targets(state)
                    break
                except (OSError, ValueError, KeyError, RuntimeError):
                    pass
            require(launcher.poll() is None, "application launcher exited before readiness")
            time.sleep(0.25)
        else:
            raise RuntimeError("bundled application did not publish a runtime")
        source_pane, window = ctx.pane_id, ctx.window_id
        report.update(process_id=state["process_id"], pane_id=source_pane, window_id=window)
        before = snapshot("before-notification")
        require(any(p["pid"] == state["process_id"] and p["active"] for p in before["apps"]),
                "product is not foreground before notification")
        capture("product-foreground")
        marker = "PEBREL-NATIVE-NOTIFICATION-1011"
        ctx.api("pane.notify", {"window_id": window, "pane_id": source_pane,
                                "title": "Pebrel native acceptance", "message": marker})
        deadline = time.monotonic() + 50
        notice = None
        while time.monotonic() < deadline:
            observed = snapshot("notification-latest")
            notice = find_notice(observed, marker)
            if notice:
                break
            permit(observed)
            time.sleep(0.5)
        capture("notification-result")
        require(notice is not None, "first foreground notification was not observed in native UI")
        report["foreground_native_delivery"] = True
        other = ctx.api("tab.new", {"window_id": window})
        ctx.refresh_targets()
        require(ctx.pane_id != source_pane, "second tab did not change the focused pane")
        report["other_pane_id"] = ctx.pane_id
        observed = snapshot("before-activation")
        notice = find_notice(observed, marker)
        require(notice is not None, "native notification disappeared before activation")
        process, node = notice
        action = "press" if "AXPress" in node.get("actions", []) else "click"
        subprocess.run([str(args.ui), action, str(process["pid"]), node["path"], marker], check=True, timeout=10)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            ctx.refresh_targets()
            if ctx.pane_id == source_pane:
                break
            time.sleep(0.2)
        require(ctx.pane_id == source_pane, "native click did not return to the originating pane")
        final = snapshot("after-activation")
        require(any(p["pid"] == state["process_id"] and p["active"] for p in final["apps"]),
                "product was not activated by native notification")
        capture("activation-result")
        report.update(status="passed", native_click_activated_origin=True)
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
        try:
            snapshot("failure-ui")
            capture("failure")
        except Exception as diagnostic:
            report["diagnostic_error"] = str(diagnostic)
    finally:
        write("result.json", report)
        # These are only processes of the private installed test copy.
        for pid, birth in owned_processes(app.executable).items():
            if owned_processes(app.executable).get(pid) == birth:
                os.kill(pid, signal.SIGTERM)
        if launcher is not None:
            try:
                launcher.wait(timeout=5)
            except subprocess.TimeoutExpired:
                launcher.terminate()
        app.close()
    print(json.dumps(report, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(run())
