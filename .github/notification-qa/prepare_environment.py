"""Start the notification UI agent disabled by the hosted macOS image."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

assert sys.platform == "darwin"
domain = f"gui/{os.getuid()}"
label = "com.apple.notificationcenterui"
service = f"{domain}/{label}"
plist = Path("/System/Library/LaunchAgents") / f"{label}.plist"
output = Path("tmp/notification-evidence/environment.json")
output.parent.mkdir(parents=True, exist_ok=True)
facts = {"service": service, "commands": [], "status": "failed"}


def run(*args):
    return subprocess.run(args, capture_output=True, text=True, timeout=15)


def state():
    result = run("launchctl", "print", service)
    fields = {"registered": result.returncode == 0}
    for key in ("state", "pid", "last exit code"):
        match = re.search(r"^\s*" + re.escape(key) + r" = ([^\n]+)", result.stdout, re.M)
        if match:
            fields[key] = match.group(1).strip()
    return fields


try:
    disabled = run("launchctl", "print-disabled", domain)
    disabled.check_returncode()
    match = re.search(r'"' + re.escape(label) + r'"\s*=>\s*(true|false)', disabled.stdout)
    facts["disabled_override_before"] = match.group(1) if match else "absent"
    facts["before"] = before = state()
    assert plist.is_file(), "standard Notification Center agent is unavailable"
    if before.get("state") != "running":
        commands = [["launchctl", "enable", service]]
        if not before["registered"]:
            commands.append(["launchctl", "bootstrap", domain, str(plist)])
        commands.append(["launchctl", "kickstart", service])
        for command in commands:
            result = run(*command)
            facts["commands"].append({"argv": command, "exit_code": result.returncode})
            result.check_returncode()
    deadline = time.monotonic() + 10
    while True:
        facts["after"] = current = state()
        if current.get("state") == "running":
            break
        assert time.monotonic() < deadline, "Notification Center agent did not start"
        time.sleep(0.2)
    facts["status"] = "ready"
except Exception as error:
    facts["error"] = f"{type(error).__name__}: {error}"
finally:
    output.write_text(json.dumps(facts, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(facts, indent=2))
raise SystemExit(0 if facts["status"] == "ready" else 1)
