"""The Android palette asset must carry the desktop theme tables, including bright ANSI."""
from pathlib import Path
import unittest

from generate_assets import terminal_ansi, theme_palettes

ROOT = Path(__file__).resolve().parents[2]
THEMES = (ROOT / "pebrel_settings/src/themes.rs").read_text(encoding="utf-8")
SETTINGS = (ROOT / "pebrel_settings/src/lib.rs").read_text(encoding="utf-8")


class TerminalPaletteTest(unittest.TestCase):
    def test_every_builtin_has_sixteen_distinct_slots(self):
        palettes = theme_palettes(THEMES, SETTINGS)
        self.assertEqual(16, len(palettes))
        for name, palette in palettes.items():
            ansi = palette["ansi"]
            self.assertEqual(16, len(ansi), name)
            self.assertTrue(all(len(color) == 3 and all(0 <= part <= 255 for part in color) for color in ansi), name)
            # Bright black always differs; the mobile table used to repeat 0-7 for 8-15.
            self.assertNotEqual(ansi[0], ansi[8], name)

    def test_each_dispatch_path_reads_its_own_table(self):
        # An exact arm, the light and dark defaults, the Nord exact table and LIGHT_ANSI.
        self.assertEqual([0x58, 0x5b, 0x70], terminal_ansi("CatppuccinMocha", THEMES, SETTINGS)[8])
        self.assertEqual([0x66, 0x77, 0x88], terminal_ansi("MintLight", THEMES, SETTINGS)[8])
        self.assertEqual([0x90, 0xa5, 0xb5], terminal_ansi("MintDark", THEMES, SETTINGS)[8])
        self.assertEqual([0x4c, 0x56, 0x6a], terminal_ansi("Nord", THEMES, SETTINGS)[8])
        self.assertEqual([87, 96, 106], terminal_ansi("SilverLight", THEMES, SETTINGS)[8])

    def test_a_missing_table_fails_instead_of_repeating_colors(self):
        without_light = SETTINGS.replace("pub const LIGHT_ANSI", "pub const LIGHT_TABLE")
        with self.assertRaises(ValueError):
            terminal_ansi("SilverLight", THEMES, without_light)
        with self.assertRaises(ValueError):
            terminal_ansi("MintLight", THEMES.replace("let ansi = match name", "let colors = match name"), SETTINGS)


if __name__ == "__main__":
    unittest.main()
