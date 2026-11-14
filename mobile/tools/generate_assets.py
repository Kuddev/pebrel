"""Build Android palette assets from Pebrel's single desktop theme authority."""
from pathlib import Path
import argparse
import json
import re
import shutil
from package_relay import package_relay


def theme_palettes(source: str, settings: str) -> dict:
    """Reviewed palettes plus each built-in's real ANSI 0-15, from the Rust theme tables."""
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
    for name in names:
        palettes[name]["ansi"] = terminal_ansi(name, source, settings)
    return palettes


def _triples(text: str) -> list[list[int]]:
    table = [[int(part, 0) for part in row] for row in re.findall(r"\[\s*(\w+),\s*(\w+),\s*(\w+),?\s*\]", text)]
    if len(table) != 16:
        raise ValueError(f"Expected ANSI 0-15, found {len(table)} entries")
    return table


def terminal_ansi(name: str, source: str, settings: str) -> list[list[int]]:
    """Follow ThemeName::term_theme: fresh_terminal, an exact table, or the light replacement."""
    dispatch = re.search(r"pub fn term_theme\(self\).*?match self \{(.*?)=> themes::fresh_terminal\(self\)", settings, re.S)
    if dispatch is None:
        raise ValueError("Unparsed ThemeName::term_theme dispatch")
    if name not in re.findall(r"Self::(\w+)", dispatch.group(1)):
        block = re.search(rf"Self::{name} => TermTheme \{{(.*?)\n {{12}}\}},", settings, re.S)
        if block is None:
            raise ValueError(f"Missing terminal theme for {name}")
        exact = re.search(r"\bansi: \[(.*?)\n {20}\],", block.group(1), re.S)
        if exact is not None:
            return _triples(exact.group(1))
        if "exact: None" not in block.group(1) or "is_light: true" not in block.group(1):
            raise ValueError(f"Dark theme {name} declares no ANSI table")
        table = re.search(r"pub const LIGHT_ANSI: \[Rgb8; 16\] = \[(.*?)\n\];", settings, re.S)
        if table is None:
            raise ValueError("Missing LIGHT_ANSI")
        return _triples(table.group(1))
    table = re.search(r"let ansi = match name \{(.*?)\n    \}\s*\.map\(rgb\)", source, re.S)
    light = re.search(r"is_light:\s*matches!\(\s*self\.available\(\),(.*?)\),", source, re.S)
    if table is None or light is None:
        raise ValueError("Unparsed fresh_terminal ANSI table")
    arms = dict(re.findall(r"(ThemeName::\w+|_ if palette\.is_light|_)\s*=>\s*\[([^\]]+)\]", table.group(1)))
    is_light = name in re.findall(r"Self::(\w+)", light.group(1))
    row = arms.get(f"ThemeName::{name}") or arms.get("_ if palette.is_light" if is_light else "_")
    if row is None:
        raise ValueError(f"Missing ANSI table for {name}")
    values = [int(value.strip(), 0) for value in row.split(",") if value.strip()]
    if len(values) != 16:
        raise ValueError(f"Expected ANSI 0-15 for {name}, found {len(values)}")
    return [[value >> 16 & 255, value >> 8 & 255, value & 255] for value in values]


def generate(output: Path, resources: Path | None = None) -> None:
    root = Path(__file__).resolve().parents[2]
    palettes = theme_palettes((root / "nebula_settings/src/themes.rs").read_text(encoding="utf-8"),
                              (root / "nebula_settings/src/lib.rs").read_text(encoding="utf-8"))
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
    # AAPT transparently expands .gz assets and removes the suffix. Keep gzip bytes
    # under a neutral extension so Android and the remote tar reader agree.
    (output / "relay-kit.tar.gz").unlink(missing_ok=True)
    (output / "relay-kit.tar").unlink(missing_ok=True)
    package_relay(root, output / "relay-kit.bin")
    # Bundle the licenses corresponding to the pinned native dependency.
    native_notices = root / "mobile/android/ghostty/build/upstream/arm64-v8a/licenses"
    if not (native_notices / "Ghostty-MIT.txt").is_file():
        raise FileNotFoundError("Build the pinned terminal core and licenses before Android packaging")
    shutil.copytree(native_notices, notices / "Ghostty", dirs_exist_ok=True)
    shutil.copyfile(root / "mobile/android/ghostty/UPSTREAM.json", notices / "Ghostty-UPSTREAM.json")
    shutil.copyfile(root / "mobile/android/third_party/THIRD-PARTY-NOTICES.md", notices / "THIRD-PARTY-NOTICES.md")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--resources", type=Path)
    args = parser.parse_args()
    generate(args.output, args.resources)
