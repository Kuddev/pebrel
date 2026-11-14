"""Derive the documentation colour themes from the desktop application's built-in themes.

The palettes are parsed from the application source rather than copied, so a theme added
or retuned in `nebula_settings` reaches the site on the next build. The build fails when
the source no longer has the expected shape instead of silently dropping a theme.
"""
import html
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
THEMES_RS = REPO / "nebula_settings/src/themes.rs"
LABELS_RS = REPO / "nebula_app/src/display/ui/theme.rs"

# Mirrors the application's factory default (Nord) and its light pair when following the system.
DEFAULT_LIGHT = "NordLight"
DEFAULT_DARK = "Nord"

FIELDS = ("shell", "background", "foreground", "muted", "accent", "selected", "line",
          "red", "green", "yellow", "blue", "purple", "cyan")


def _bytes(raw: str) -> tuple[int, ...]:
    return tuple(int(value.strip(), 0) for value in raw.split(",") if value.strip())


def _hex(channels: tuple[int, ...]) -> str:
    return "#" + "".join(f"{channel:02x}" for channel in channels[:3])


def _mix(front: tuple[int, ...], back: tuple[int, ...], amount: float) -> tuple[int, ...]:
    return tuple(round(f * amount + b * (1 - amount)) for f, b in zip(front[:3], back[:3]))


def luminance(channels: tuple[int, ...]) -> float:
    def linear(value: int) -> float:
        value /= 255
        return value / 12.92 if value <= 0.03928 else ((value + 0.055) / 1.055) ** 2.4
    red, green, blue = (linear(value) for value in channels[:3])
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def contrast(first: tuple[int, ...], second: tuple[int, ...]) -> float:
    high, low = sorted((luminance(first), luminance(second)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def load() -> list[dict]:
    """Return the selectable built-in themes in the order the application lists them."""
    source = THEMES_RS.read_text(encoding="utf-8")
    order = re.search(r"pub const BUILTIN: \[Self; \d+\] = \[(.*?)\];", source, re.S)
    if not order:
        raise ValueError("themes.rs: BUILTIN catalog not found")
    variants = re.findall(r"Self::(\w+)", order.group(1))
    labels_source = LABELS_RS.read_text(encoding="utf-8")
    label_block = re.search(r"pub\(crate\) fn label\(self\) -> &'static str \{(.*?)\n    \}", labels_source, re.S)
    if not label_block:
        raise ValueError("theme.rs: label() not found")
    labels = dict(re.findall(r'Self::(\w+) => "([^"]+)"', label_block.group(1)))
    reviewed = re.search(r"pub const fn reviewed_palette\(self\).*?\n    \}\n", source, re.S)
    if not reviewed:
        raise ValueError("themes.rs: reviewed_palette() not found")
    arms = {
        name: body
        for name, body in re.findall(r"Self::(\w+) => ReviewedPalette \{(.*?)\n            \},", reviewed.group(0), re.S)
    }
    themes = []
    for variant in variants:
        if variant not in arms or variant not in labels:
            raise ValueError(f"theme {variant}: palette or label missing in the application source")
        palette = {}
        for field in FIELDS:
            found = re.search(rf"\b{field}: \[([^\]]*)\]", arms[variant])
            if not found:
                raise ValueError(f"theme {variant}: field {field} missing")
            palette[field] = _bytes(found.group(1))
        themes.append({
            "id": variant, "label": labels[variant], "palette": palette,
            "light": luminance(palette["background"]) > 0.4,
        })
    ids = {theme["id"] for theme in themes}
    if DEFAULT_LIGHT not in ids or DEFAULT_DARK not in ids:
        raise ValueError("default theme pair is not selectable")
    return themes


def _over(front: tuple[int, ...], back: tuple[int, ...]) -> tuple[int, ...]:
    """Composite a colour that may carry an alpha channel, as the application does."""
    alpha = front[3] / 255 if len(front) > 3 else 1
    return _mix(front, back, alpha)


def readable(color: tuple[int, ...], foreground: tuple[int, ...], background: tuple[int, ...]) -> tuple[int, ...]:
    """Keep a theme colour for text, nudged toward the foreground only when too faint to read."""
    for step in range(21):
        candidate = _mix(foreground, color, step / 20)
        if contrast(candidate, background) >= 4.5:
            return candidate
    return foreground


def tokens(theme: dict) -> dict[str, str]:
    palette = theme["palette"]
    background = palette["background"]
    selected = _mix(_over(palette["selected"], background), background, 0.5)
    foreground = palette["foreground"]
    accent = readable(palette["accent"], foreground, selected)
    muted = readable(palette["muted"], foreground, selected)

    def ink(name: str) -> str:
        return _hex(readable(palette[name], foreground, selected))
    return {
        "bg": _hex(background),
        "panel": _hex(palette["shell"]),
        "soft": _hex(selected),
        "code": _hex(selected),
        "tint": _hex(_mix(accent, background, 0.12)),
        "text": _hex(palette["foreground"]),
        "muted": _hex(muted),
        "line": _hex(_over(palette["line"], background)),
        "accent": _hex(accent),
        "red": ink("red"), "green": ink("green"), "yellow": ink("yellow"),
        "blue": ink("blue"), "purple": ink("purple"), "cyan": ink("cyan"),
    }


def _block(selector: str, theme: dict) -> str:
    scheme = "light" if theme["light"] else "dark"
    body = "".join(f"--{name}:{value};" for name, value in tokens(theme).items())
    return f"{selector}{{color-scheme:{scheme};{body}}}\n"


def stylesheet(themes: list[dict]) -> str:
    by_id = {theme["id"]: theme for theme in themes}
    css = "/* Generated by palettes.py from the application's built-in themes. */\n"
    css += _block(":root", by_id[DEFAULT_LIGHT])
    css += "@media (prefers-color-scheme: dark){\n" + _block(":root:not([data-palette])", by_id[DEFAULT_DARK]) + "}\n"
    for theme in themes:
        css += _block(f'html[data-palette="{theme["id"]}"]', theme)
    return css


def code_stylesheet() -> str:
    """Syntax colours that follow the active palette instead of two fixed Pygments styles."""
    groups = [
        ("var(--purple)", "k kd kn kp kr kt"),
        ("var(--green)", "s s1 s2 sa sb sc sd se sh si sx sr ss"),
        ("var(--yellow)", "m mb mf mh mi mo il na"),
        ("var(--blue)", "nf nc fm nd ne nt"),
        ("var(--cyan)", "nb bp o ow"),
        ("var(--red)", "err gd gr"),
        ("var(--green)", "gi"),
    ]
    lines = []
    for color, classes in groups:
        selector = ",".join(f".code-block .{name}" for name in classes.split())
        lines.append(f"{selector}{{color:{color}}}")
    comments = ",".join(f".code-block .{name}" for name in "c c1 cm cp cs ch cpf".split())
    lines.append(f"{comments}{{color:var(--muted);font-style:italic}}")
    lines.append(".code-block .gh,.code-block .gu,.code-block .gs{font-weight:600}")
    return "\n".join(lines) + "\n"


def dark_ids(themes: list[dict]) -> str:
    return ",".join(f"'{theme['id']}'" for theme in themes if not theme["light"])


def menu(themes: list[dict]) -> str:
    def item(theme: dict) -> str:
        values = tokens(theme)
        return (
            f'<button type="button" role="menuitemradio" aria-checked="false" data-palette-option="{theme["id"]}">'
            f'<span class="swatch" style="--a:{values["bg"]};--b:{values["accent"]}" aria-hidden="true"></span>'
            f'{html.escape(theme["label"])}</button>')

    def group(title: str, light: bool) -> str:
        entries = "".join(item(theme) for theme in themes if theme["light"] is light)
        return f'<div class="palette-group" role="group" aria-label="{title}"><span>{title}</span>{entries}</div>'

    system = ('<button type="button" role="menuitemradio" aria-checked="false" data-palette-option="system">'
              '<span class="swatch swatch-system" aria-hidden="true"></span>跟随系统</button>')
    return system + group("浅色", True) + group("深色", False)
