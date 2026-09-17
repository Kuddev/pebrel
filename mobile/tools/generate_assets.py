"""Build Android palette assets from Pebrel's single desktop theme authority."""
from pathlib import Path
import argparse
import json
import re
import shutil


def generate(output: Path, resources: Path | None = None) -> None:
    root = Path(__file__).resolve().parents[2]
    source = (root / "nebula_settings/src/themes.rs").read_text(encoding="utf-8")
    names_block = source.split("pub const BUILTIN:", 1)[1].split("];", 1)[0]
    names = re.findall(r"Self::(\w+)", names_block)
    palettes = {}
    for name, body in re.findall(r"Self::(\w+)\s*=>\s*ReviewedPalette\s*\{(.*?)\n\s*\}", source, re.S):
        colors = {}
        for key, values in re.findall(r"(\w+):\s*\[([^\]]+)\]", body):
            colors[key] = [int(value.strip(), 0) for value in values.split(",") if value.strip()]
        if name in names:
            for key in ["shell", "background", "foreground", "accent"]:
                if len(colors.get(key, [])) != 3:
                    raise ValueError(f"Missing authoritative {name}.{key}")
            palettes[name] = colors
    if set(palettes) != set(names):
        raise ValueError(f"Unparsed built-in palettes: {set(names) - set(palettes)}")
    output.mkdir(parents=True, exist_ok=True)
    (output / "themes.json").write_text(json.dumps(palettes, separators=(",", ":")), encoding="utf-8")
    # Keep icon identity, glyph metrics and both labels in the desktop authority.
    icon_source = (root / "nebula_app/src/display/ui/os_icons.rs").read_text(encoding="utf-8")
    catalog = icon_source.split("pub(crate) const CATALOG:", 1)[1].split("];", 1)[0]
    icons = []
    for body in re.findall(r"OsIcon \{ (.*?) \}", catalog):
        def field(name: str) -> str:
            return re.search(rf'{name}: "([^"]+)"', body).group(1)
        codepoint = re.search(r"glyph: '\\u\{([0-9a-f]+)\}'", body).group(1)
        icons.append({"id": field("id"), "glyph": chr(int(codepoint, 16)),
                      "zh": field("zh"), "en": field("en")})
    if len(icons) != catalog.count("OsIcon {") or not any(icon["id"] == "term" for icon in icons):
        raise ValueError("Unparsed desktop host icon catalog")
    (output / "host-icons.json").write_text(json.dumps(icons, ensure_ascii=False), encoding="utf-8")
    shutil.copyfile(root / "assets/fonts/MapleMono-NF-CN-Regular.ttf", output / "terminal.ttf")
    fonts = output / "fonts"
    fonts.mkdir(exist_ok=True)
    shutil.copyfile(root / "assets/fonts/JetBrainsMono-Regular.ttf", fonts / "JetBrainsMono-Regular.ttf")
    if resources is not None:
        drawable = resources / "drawable-nodpi"
        drawable.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(root / "extra/logo/nebula-titanium.png", drawable / "ic_pebrel.png")
    notices = output / "licenses"
    notices.mkdir(exist_ok=True)
    for license_file in (root / "mobile/android/third_party/licenses").glob("*.txt"):
        shutil.copyfile(license_file, notices / license_file.name)
    # Bundle the licenses corresponding to the pinned native dependency.
    native_notices = root / "mobile/android/ghostty/build/upstream/arm64-v8a/licenses"
    if not (native_notices / "Ghostty-MIT.txt").is_file():
        raise FileNotFoundError("Build the pinned Ghostty core and licenses before Android packaging")
    shutil.copytree(native_notices, notices / "Ghostty", dirs_exist_ok=True)
    shutil.copyfile(root / "mobile/android/ghostty/UPSTREAM.json", notices / "Ghostty-UPSTREAM.json")
    shutil.copyfile(root / "mobile/android/third_party/THIRD-PARTY-NOTICES.md", notices / "THIRD-PARTY-NOTICES.md")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--resources", type=Path)
    args = parser.parse_args()
    generate(args.output, args.resources)
