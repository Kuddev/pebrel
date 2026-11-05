#!/usr/bin/env python3
"""Split the desktop logo into Android adaptive launcher layers.

Requires Pillow. Run after the logo changes and commit the two PNGs:
python3 mobile/tools/launcher_layers.py
"""

from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
LOGO = ROOT / "extra/logo/nebula-titanium.png"
OUTPUT = ROOT / "mobile/android/app/src/main/res/drawable-nodpi"
CANVAS = 432          # 108 dp at 4x
TILE = (32, 992)      # the logo's rounded tile, border included
VISIBLE = 288         # 72 dp: what every launcher mask keeps


def luminance(pixel):
    red, green, blue = pixel[:3]
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def main():
    logo = Image.open(LOGO).convert("RGBA")
    width, height = logo.size
    pixels = logo.load()
    tile = pixels[width // 2, TILE[0] + 40][:3]
    blob = pixels[width // 2 + 20, height // 2 - 40][:3]
    light, dark = luminance(tile), luminance(blob)
    threshold = (light + dark) / 2
    # Light pixels reachable from the tile edge are background; enclosed ones are the glyph's strokes.
    outside = bytearray(width * height)
    queue = deque([(width // 2, TILE[0] + 40)])
    while queue:
        x, y = queue.popleft()
        index = y * width + x
        if outside[index] or not (TILE[0] + 20 <= x < TILE[1] - 20 and TILE[0] + 20 <= y < TILE[1] - 20):
            continue
        if pixels[x, y][3] == 255 and luminance(pixels[x, y]) < threshold:
            continue
        outside[index] = 1
        queue.extend(((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)))
    foreground = Image.new("RGBA", (width, height))
    monochrome = Image.new("RGBA", (width, height))
    front, mono = foreground.load(), monochrome.load()
    # Only the glyph's neighbourhood: the tile's border and shading must not leave a faint ring.
    dark_pixels = [(x, y) for y in range(TILE[0] + 20, TILE[1] - 20) for x in range(TILE[0] + 20, TILE[1] - 20)
                   if pixels[x, y][3] == 255 and luminance(pixels[x, y]) < threshold]
    left, right = min(x for x, _ in dark_pixels) - 6, max(x for x, _ in dark_pixels) + 7
    top, bottom = min(y for _, y in dark_pixels) - 6, max(y for _, y in dark_pixels) + 7
    for y in range(top, bottom):
        for x in range(left, right):
            pixel = pixels[x, y]
            if pixel[3] < 255:
                continue  # outside the rounded tile
            coverage = max(0.0, min(1.0, (light - luminance(pixel)) / (light - dark)))
            mono[x, y] = (255, 255, 255, round(255 * coverage))
            if outside[y * width + x]:
                front[x, y] = (*blob, round(255 * coverage))
            else:
                front[x, y] = (*pixel[:3], 255)
    scale = VISIBLE / (TILE[1] - TILE[0])
    size = round(width * scale)
    offset = round(CANVAS / 2 - width / 2 * scale)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for name, layer in (("ic_launcher_foreground.png", foreground), ("ic_launcher_monochrome.png", monochrome)):
        resized = layer.convert("RGBa").resize((size, size), Image.LANCZOS).convert("RGBA")
        canvas = Image.new("RGBA", (CANVAS, CANVAS))
        canvas.alpha_composite(resized, (offset, offset))
        canvas.save(OUTPUT / name, optimize=True)
    print("background #%02X%02X%02X" % tile)


if __name__ == "__main__":
    main()
