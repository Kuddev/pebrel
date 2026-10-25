"""Rebuild Pebrel's independent symbol face from the pinned bundled font.

Requires fontTools (the regeneration tool, not an application dependency).
The original font remains unchanged and available for terminal text.
"""

from hashlib import sha256
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets/fonts/MapleMonoNormal-NF-CN-Regular.ttf"
OUTPUT = ROOT / "assets/fonts/PebrelSymbols-Regular.ttf"
SOURCE_SHA256 = "9ae0c348a44a681fa8fa20a34e782e95316cec880102295a128b65760944c0f5"


def main() -> None:
    if sha256(SOURCE.read_bytes()).hexdigest() != SOURCE_SHA256:
        raise ValueError("Review the source font and its license before regenerating symbols")
    font = TTFont(SOURCE, recalcTimestamp=False)
    cmap = font.getBestCmap()
    # 保留完整私用区图标，不能只取当前 UI 用到的几个码点而破坏用户提示符。
    symbols = {
        cp for cp in cmap
        if 0xE000 <= cp <= 0xF8FF or 0xF0000 <= cp <= 0xFFFFD
        or 0x100000 <= cp <= 0x10FFFD or 0x23FB <= cp <= 0x23FE or cp == 0x2B58
    }
    options = subset.Options()
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.name_legacy = True
    options.layout_features = []
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=symbols)
    subsetter.subset(font)
    names = {1: "Pebrel Symbols", 2: "Regular", 3: "PebrelSymbols-Regular",
             4: "Pebrel Symbols Regular", 6: "PebrelSymbols-Regular",
             16: "Pebrel Symbols", 17: "Regular", 18: "Pebrel Symbols Regular",
             21: "Pebrel Symbols", 22: "Regular"}
    # OFL 保留名称不用于派生字体；保留原版权和许可证记录。
    for record in list(font["name"].names):
        if record.nameID in names:
            font["name"].setName(names[record.nameID], record.nameID,
                                 record.platformID, record.platEncID, record.langID)
    font.save(OUTPUT, reorderTables=True)
    print(f"{OUTPUT.name}: {len(symbols)} code points, {OUTPUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
