"""Prepare generated iOS resources from the checked-in mobile authorities."""
import hashlib
import json
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "mobile/tools"))
from generate_assets import theme_palettes


def generate():
    out = ROOT / "mobile/ios/.generated"
    out.mkdir(exist_ok=True)
    palettes = theme_palettes((ROOT / "pebrel_settings/src/themes.rs").read_text(),
                              (ROOT / "pebrel_settings/src/lib.rs").read_text())
    (out / "themes.json").write_text(json.dumps(palettes, separators=(",", ":")))
    for font in json.loads((ROOT / "mobile/android/third_party/FONTS.json").read_text()).values():
        src = ROOT / font["file"]
        if hashlib.sha256(src.read_bytes()).hexdigest() != font["sha256"]:
            raise ValueError(f"Font checksum mismatch for {src.name}")
        shutil.copyfile(src, out / src.name)
    licenses = out/"Licenses"
    shutil.copytree(ROOT/"tmp/ios-vt/licenses", licenses/"Ghostty", dirs_exist_ok=True)
    shutil.copytree(ROOT/"mobile/android/third_party/licenses", licenses/"Fonts", dirs_exist_ok=True)
    shutil.copytree(ROOT/"mobile/ssh/licenses", licenses/"SSH", dirs_exist_ok=True)
    icon = out / "Assets.xcassets/AppIcon.appiconset"
    icon.mkdir(parents=True, exist_ok=True)
    import subprocess
    subprocess.run(["sips", "-z", "1024", "1024", str(ROOT / "extra/logo/nebula-titanium.png"),
                    "--out", str(icon/"Icon.png")], check=True)
    (icon/"Contents.json").write_text(json.dumps({"images":[{"filename":"Icon.png","idiom":"universal",
        "platform":"ios","size":"1024x1024"}],"info":{"author":"xcode","version":1}}))


if __name__ == "__main__":
    generate()
