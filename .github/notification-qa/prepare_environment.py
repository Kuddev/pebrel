"""Start the notification UI agent disabled by the hosted macOS image."""
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import time

assert sys.platform == "darwin"
domain = f"gui/{os.getuid()}"
plist = Path("/System/Library/LaunchAgents/com.apple.notificationcenterui.plist")
output = Path("tmp/notification-evidence/environment.json")
output.parent.mkdir(parents=True, exist_ok=True)
facts = {"commands": [], "status": "failed"}


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
    assert plist.is_file(), "standard Notification Center agent is unavailable"
    metadata = plistlib.loads(plist.read_bytes())
    label = metadata["Label"]
    assert label.startswith("com.apple.notificationcenterui"), "unexpected system agent label"
    service = f"{domain}/{label}"
    facts.update(service=service, plist=str(plist), session=metadata.get("LimitLoadToSessionType"))
    disabled = run("launchctl", "print-disabled", domain)
    disabled.check_returncode()
    match = re.search(r'"' + re.escape(label) + r'"\s*=>\s*(true|false)', disabled.stdout)
    facts["disabled_override_before"] = match.group(1) if match else "absent"
    facts["before"] = before = state()
    if before.get("state") != "running":
        commands = [["launchctl", "enable", service]]
        if not before["registered"]:
            commands.append(["launchctl", "bootstrap", domain, str(plist)])
        commands.append(["launchctl", "kickstart", service])
        for command in commands:
            result = run(*command)
            facts["commands"].append({"argv": command, "exit_code": result.returncode,
                                      "error": result.stderr.strip() if result.returncode else ""})
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
