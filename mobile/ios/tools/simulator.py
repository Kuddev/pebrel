"""Select an installed iPhone and retain named XCTest screenshot attachments."""
import json
from pathlib import Path
import shutil
import subprocess
import sys


def device():
    listing = json.loads(subprocess.check_output(["xcrun", "simctl", "list", "devices", "available", "-j"]))
    for runtime, devices in reversed(list(listing["devices"].items())):
        if ".iOS-" in runtime:
            phones = [d for d in devices if d["name"].startswith("iPhone")]
            if phones:
                return next((d["udid"] for d in phones if d["name"] in ("iPhone 17", "iPhone 16")),
                            phones[0]["udid"])
    raise RuntimeError("No available iPhone simulator")


def export(result, dest):
    raw = dest.parent / ("attachments-" + result.stem)
    subprocess.run(["xcrun", "xcresulttool", "export", "attachments", "--path", result,
                    "--output-path", raw], check=True)
    dest.mkdir(parents=True, exist_ok=True)
    count = 0
    for test in json.loads((raw/"manifest.json").read_text()):
        for attachment in test["attachments"]:
            name = attachment.get("suggestedHumanReadableName", "")
            src = raw / attachment["exportedFileName"]
            if "terminal-" in name and src.suffix.lower() == ".png":
                name = Path(name[name.index("terminal-"):]).name
                shutil.copyfile(src, dest / (name if name.endswith(".png") else name + ".png"))
                count += 1
    if not count:
        raise RuntimeError("No named terminal screenshot: " + (raw/"manifest.json").read_text())


if __name__ == "__main__":
    if len(sys.argv) == 1:
        print(device())
    else:
        export(Path(sys.argv[1]), Path(sys.argv[2]))
