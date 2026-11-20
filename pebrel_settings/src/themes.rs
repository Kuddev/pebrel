//! Static built-in theme catalog, shared by terminal and chrome adapters.
use crate::{ExactTermColors, Rgb8, TermTheme, ThemeName};

#[derive(Clone, Copy, Debug)]
pub struct FreshPalette {
    pub shell: Rgb8,
    pub surface: Rgb8,
    pub accent: Rgb8,
    pub foreground: Rgb8,
    pub muted: Rgb8,
    pub is_light: bool,
}

impl ThemeName {
    /// The only catalog of selectable built-ins. Retired identifiers remain readable.
    pub const BUILTIN: [Self; 16] = [
        Self::BreezeLight,
        Self::BreezeDark,
        Self::MintLight,
        Self::MintDark,
        Self::SilverLight,
        Self::Nord,
        Self::NordLight,
        Self::Paper,
        Self::LimestoneLight,
        Self::LinenLight,
        Self::CatppuccinMocha,
        Self::CatppuccinLatte,
        Self::CatppuccinFrappe,
        Self::CatppuccinMacchiato,
        Self::GlassLight,
        Self::GlassDark,
    ];

    pub const BUILTIN_NAMES: [&'static str; Self::BUILTIN.len()] = {
        let mut names = [""; Self::BUILTIN.len()];
        let mut index = 0;
        while index < Self::BUILTIN.len() {
            names[index] = Self::BUILTIN[index].prompt_name();
            index += 1;
        }
        names
    };

    /// Retired names keep existing settings readable without reappearing in the picker.
    pub const fn available(self) -> Self {
        match self {
            Self::Nebula => Self::CatppuccinMocha,
            Self::SteelDark | Self::CoalDark => Self::Nord,
            Self::MossDark => Self::MintDark,
            other => other,
        }
    }

    pub fn fresh_palette(self) -> Option<FreshPalette> {
        let palette = self.reviewed_palette();
        Some(FreshPalette {
            shell: palette.shell,
            surface: palette.background,
            accent: palette.accent,
            foreground: palette.foreground,
            muted: palette.muted,
            is_light: matches!(
                self.available(),
                Self::BreezeLight
                    | Self::MintLight
                    | Self::SilverLight
                    | Self::NordLight
                    | Self::Paper
                    | Self::LimestoneLight
                    | Self::LinenLight
                    | Self::CatppuccinLatte
                    | Self::GlassLight
            ),
        })
    }
}

const fn rgb(value: u32) -> Rgb8 {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

/// Exact semantic colors of the reviewed terminal HTML. These values are static;
/// color adapters must not desaturate accents or synthesize a second selected ramp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewedPalette {
    pub shell: Rgb8,
    pub background: Rgb8,
    pub foreground: Rgb8,
    pub muted: Rgb8,
    pub accent: Rgb8,
    pub selected: [u8; 4],
    pub line: [u8; 4],
    pub red: Rgb8,
    pub green: Rgb8,
    pub yellow: Rgb8,
    pub blue: Rgb8,
    pub purple: Rgb8,
    pub cyan: Rgb8,
    pub frame: Rgb8,
}

impl ReviewedPalette {
    /// CSS-style alpha composition of the reviewed secondary surface on the pane.
    pub const fn code_background(self) -> Rgb8 {
        let alpha = self.selected[3] as u32;
        let mut color = [0; 3];
        let mut index = 0;
        while index < 3 {
            color[index] = ((self.selected[index] as u32 * alpha
                + self.background[index] as u32 * (255 - alpha)
                + 127)
                / 255) as u8;
            index += 1;
        }
        color
    }
}

impl ThemeName {
    pub const fn reviewed_palette(self) -> ReviewedPalette {
        match self.available() {
            Self::BreezeLight => ReviewedPalette {
                shell: [0xea, 0xf0, 0xf6],
                background: [0xf9, 0xfb, 0xff],
                foreground: [0x26, 0x38, 0x4a],
                muted: [0x57, 0x6c, 0x80],
                accent: [0x30, 0x65, 0x92],
                selected: [0xdf, 0xe9, 0xf2, 255],
                line: [0xd4, 0xdf, 0xe9, 255],
                red: [0xa3, 0x42, 0x48],
                green: [0x28, 0x74, 0x5e],
                yellow: [0x88, 0x62, 0x1f],
                blue: [0x30, 0x65, 0x92],
                purple: [0x77, 0x56, 0x9b],
                cyan: [0x24, 0x77, 0x82],
                frame: [0x95, 0xa7, 0xb8],
            },
            Self::BreezeDark => ReviewedPalette {
                shell: [0x18, 0x23, 0x2e],
                background: [0x20, 0x2e, 0x3b],
                foreground: [0xe0, 0xea, 0xf3],
                muted: [0xa5, 0xb6, 0xc7],
                accent: [0x91, 0xbc, 0xdf],
                selected: [0x2b, 0x3c, 0x4b, 255],
                line: [0x34, 0x46, 0x54, 255],
                red: [0xe4, 0x9a, 0x9d],
                green: [0x9b, 0xc5, 0xa6],
                yellow: [0xe0, 0xc3, 0x8b],
                blue: [0x91, 0xbc, 0xdf],
                purple: [0xc5, 0xad, 0xdd],
                cyan: [0x91, 0xce, 0xcf],
                frame: [0x50, 0x65, 0x79],
            },
            Self::MintLight => ReviewedPalette {
                shell: [0xea, 0xf2, 0xee],
                background: [0xf8, 0xfc, 0xf9],
                foreground: [0x26, 0x3c, 0x33],
                muted: [0x58, 0x73, 0x65],
                accent: [0x28, 0x74, 0x5e],
                selected: [0xdc, 0xeb, 0xe2, 255],
                line: [0xd0, 0xe0, 0xd6, 255],
                red: [0xa3, 0x42, 0x48],
                green: [0x28, 0x74, 0x5e],
                yellow: [0x88, 0x62, 0x1f],
                blue: [0x30, 0x65, 0x92],
                purple: [0x77, 0x56, 0x9b],
                cyan: [0x24, 0x77, 0x82],
                frame: [0x94, 0xaa, 0x9d],
            },
            Self::MintDark => ReviewedPalette {
                shell: [0x19, 0x2a, 0x26],
                background: [0x21, 0x37, 0x30],
                foreground: [0xdf, 0xee, 0xe7],
                muted: [0xa6, 0xbf, 0xb2],
                accent: [0x8b, 0xcb, 0xb3],
                selected: [0x2d, 0x44, 0x3b, 255],
                line: [0x3a, 0x50, 0x46, 255],
                red: [0xe4, 0x9a, 0x9d],
                green: [0x9b, 0xc5, 0xa6],
                yellow: [0xe0, 0xc3, 0x8b],
                blue: [0x91, 0xbc, 0xdf],
                purple: [0xc5, 0xad, 0xdd],
                cyan: [0x91, 0xce, 0xcf],
                frame: [0x52, 0x6f, 0x62],
            },
            Self::SilverLight => ReviewedPalette {
                shell: [0xf3, 0xf4, 0xf6],
                background: [0xff, 0xff, 0xff],
                foreground: [0x24, 0x29, 0x2f],
                muted: [0x60, 0x67, 0x71],
                accent: [0x49, 0x50, 0x57],
                selected: [0xe6, 0xe8, 0xec, 255],
                line: [0xdf, 0xe2, 0xe6, 255],
                red: [0xcf, 0x22, 0x2e],
                green: [0x1a, 0x7f, 0x37],
                yellow: [0x9a, 0x67, 0x00],
                blue: [0x09, 0x69, 0xda],
                purple: [0x82, 0x50, 0xdf],
                cyan: [0x1b, 0x7c, 0x83],
                frame: [0xa7, 0xac, 0xb4],
            },
            Self::Nord => ReviewedPalette {
                shell: [0x2e, 0x34, 0x40],
                background: [0x2e, 0x34, 0x40],
                foreground: [0xe5, 0xe9, 0xf0],
                muted: [0xab, 0xb5, 0xc7],
                accent: [0x88, 0xc0, 0xd0],
                selected: [0x3b, 0x42, 0x52, 255],
                line: [0x43, 0x4c, 0x5e, 255],
                red: [0xbf, 0x61, 0x6a],
                green: [0xa3, 0xbe, 0x8c],
                yellow: [0xeb, 0xcb, 0x8b],
                blue: [0x81, 0xa1, 0xc1],
                purple: [0xb4, 0x8e, 0xad],
                cyan: [0x88, 0xc0, 0xd0],
                frame: [0x65, 0x72, 0x86],
            },
            // Snow Storm / Polar Night from nordtheme/nord; accent and ANSI
            // inks are darkened for light surfaces, not an official Nord port.
            Self::NordLight => ReviewedPalette {
                shell: [0xe5, 0xe9, 0xf0],
                background: [0xec, 0xef, 0xf4],
                foreground: [0x2e, 0x34, 0x40],
                muted: [0x59, 0x65, 0x79],
                accent: [0x4c, 0x6a, 0x87],
                selected: [0xd8, 0xde, 0xe9, 255],
                line: [0xcc, 0xd3, 0xdf, 255],
                red: [0xa3, 0x43, 0x4c],
                green: [0x50, 0x6b, 0x3e],
                yellow: [0x82, 0x63, 0x23],
                blue: [0x4c, 0x6a, 0x87],
                purple: [0x80, 0x55, 0x78],
                cyan: [0x37, 0x6d, 0x76],
                frame: [0x9c, 0xa9, 0xbb],
            },
            // Warm Sand: user-supplied palette; Paper is the compatibility identity.
            Self::Paper => ReviewedPalette {
                shell: [0xf5, 0xf4, 0xf2],
                background: [0xfd, 0xfd, 0xfc],
                foreground: [0x2f, 0x2e, 0x2b],
                muted: [0x73, 0x6e, 0x68],
                accent: [0xd9, 0x77, 0x06],
                selected: [0xd9, 0x77, 0x06, 26],
                line: [0xeb, 0xe9, 0xe6, 255],
                red: [0xa3, 0x47, 0x40],
                green: [0x57, 0x6d, 0x46],
                yellow: [0x89, 0x63, 0x27],
                blue: [0x50, 0x6d, 0x80],
                purple: [0x80, 0x5e, 0x78],
                cyan: [0x42, 0x6f, 0x6a],
                frame: [0xa8, 0xa2, 0x9e],
            },
            Self::LimestoneLight => ReviewedPalette {
                shell: [0xf0, 0xef, 0xeb],
                background: [0xff, 0xff, 0xff],
                foreground: [0x24, 0x29, 0x2f],
                muted: [0x70, 0x6c, 0x63],
                accent: [0x58, 0x55, 0x4c],
                selected: [0xe6, 0xe3, 0xdc, 255],
                line: [0xde, 0xdb, 0xd2, 255],
                red: [0xcf, 0x22, 0x2e],
                green: [0x1a, 0x7f, 0x37],
                yellow: [0x9a, 0x67, 0x00],
                blue: [0x09, 0x69, 0xda],
                purple: [0x82, 0x50, 0xdf],
                cyan: [0x1b, 0x7c, 0x83],
                frame: [0xaa, 0xa4, 0x96],
            },
            Self::LinenLight => ReviewedPalette {
                shell: [0xf2, 0xf2, 0xec],
                background: [0xff, 0xff, 0xff],
                foreground: [0x24, 0x29, 0x2f],
                muted: [0x6b, 0x70, 0x66],
                accent: [0x5f, 0x63, 0x5f],
                selected: [0xe5, 0xe7, 0xde, 255],
                line: [0xdc, 0xdf, 0xd3, 255],
                red: [0xcf, 0x22, 0x2e],
                green: [0x1a, 0x7f, 0x37],
                yellow: [0x9a, 0x67, 0x00],
                blue: [0x09, 0x69, 0xda],
                purple: [0x82, 0x50, 0xdf],
                cyan: [0x1b, 0x7c, 0x83],
                frame: [0xa4, 0xaa, 0x9b],
            },
            Self::CatppuccinMocha => ReviewedPalette {
                shell: [0x18, 0x18, 0x25],
                background: [0x1e, 0x1e, 0x2e],
                foreground: [0xcd, 0xd6, 0xf4],
                muted: [0xa6, 0xad, 0xc8],
                accent: [0xb4, 0xbe, 0xfe],
                selected: [0x31, 0x32, 0x44, 255],
                line: [0x45, 0x47, 0x5a, 255],
                red: [0xf3, 0x8b, 0xa8],
                green: [0xa6, 0xe3, 0xa1],
                yellow: [0xf9, 0xe2, 0xaf],
                blue: [0x89, 0xb4, 0xfa],
                purple: [0xcb, 0xa6, 0xf7],
                cyan: [0x94, 0xe2, 0xd5],
                frame: [0x6c, 0x70, 0x86],
            },
            Self::CatppuccinLatte => ReviewedPalette {
                shell: [0xe6, 0xe9, 0xef],
                background: [0xef, 0xf1, 0xf5],
                foreground: [0x4c, 0x4f, 0x69],
                muted: [0x5c, 0x5f, 0x77],
                accent: [0x72, 0x87, 0xfd],
                selected: [0xcc, 0xd0, 0xda, 255],
                line: [0xbc, 0xc0, 0xcc, 255],
                red: [0xd2, 0x0f, 0x39],
                green: [0x40, 0xa0, 0x2b],
                yellow: [0xdf, 0x8e, 0x1d],
                blue: [0x1e, 0x66, 0xf5],
                purple: [0x88, 0x39, 0xef],
                cyan: [0x17, 0x92, 0x99],
                frame: [0x9c, 0xa0, 0xb0],
            },
            // Catppuccin/palette (MIT): https://github.com/catppuccin/palette
            // Base/Mantle surfaces, Text/Subtext 1 ink, Lavender accent.
            Self::CatppuccinFrappe => ReviewedPalette {
                shell: [0x29, 0x2c, 0x3c],
                background: [0x30, 0x34, 0x46],
                foreground: [0xc6, 0xd0, 0xf5],
                muted: [0xb5, 0xbf, 0xe2],
                accent: [0xba, 0xbb, 0xf1],
                selected: [0x41, 0x45, 0x59, 255],
                line: [0x51, 0x57, 0x6d, 255],
                red: [0xe7, 0x82, 0x84],
                green: [0xa6, 0xd1, 0x89],
                yellow: [0xe5, 0xc8, 0x90],
                blue: [0x8c, 0xaa, 0xee],
                purple: [0xca, 0x9e, 0xe6],
                cyan: [0x81, 0xc8, 0xbe],
                frame: [0x73, 0x79, 0x94],
            },
            Self::CatppuccinMacchiato => ReviewedPalette {
                shell: [0x1e, 0x20, 0x30],
                background: [0x24, 0x27, 0x3a],
                foreground: [0xca, 0xd3, 0xf5],
                muted: [0xb8, 0xc0, 0xe0],
                accent: [0xb7, 0xbd, 0xf8],
                selected: [0x36, 0x3a, 0x4f, 255],
                line: [0x49, 0x4d, 0x64, 255],
                red: [0xed, 0x87, 0x96],
                green: [0xa6, 0xda, 0x95],
                yellow: [0xee, 0xd4, 0x9f],
                blue: [0x8a, 0xad, 0xf4],
                purple: [0xc6, 0xa0, 0xf6],
                cyan: [0x8b, 0xd5, 0xca],
                frame: [0x6e, 0x73, 0x8d],
            },
            // Slate Light: user-supplied cool-gray palette. The Rust identity
            // stays GlassLight so existing saved preferences remain readable.
            Self::GlassLight => ReviewedPalette {
                shell: [0xf8, 0xfa, 0xfc],
                background: [0xff, 0xff, 0xff],
                foreground: [0x0f, 0x17, 0x2a],
                muted: [0x47, 0x55, 0x69],
                accent: [0x63, 0x66, 0xf1],
                selected: [0x63, 0x66, 0xf1, 26],
                line: [0xe2, 0xe8, 0xf0, 255],
                red: [0xb9, 0x1c, 0x1c],
                green: [0x15, 0x80, 0x3d],
                yellow: [0x92, 0x60, 0x0a],
                blue: [0x4f, 0x46, 0xe5],
                purple: [0x7e, 0x22, 0xce],
                cyan: [0x0e, 0x74, 0x90],
                frame: [0x94, 0xa3, 0xb8],
            },
            Self::GlassDark => ReviewedPalette {
                shell: [0x44, 0x44, 0x45],
                background: [0x40, 0x43, 0x4b],
                foreground: [0xf7, 0xf8, 0xff],
                muted: [0xc4, 0xca, 0xd6],
                accent: [0xbb, 0xc9, 0xed],
                selected: [0xff, 0xff, 0xff, 0x15],
                line: [0x6b, 0x72, 0x86, 0x40],
                red: [0xff, 0x8a, 0x8a],
                green: [0xa8, 0xd4, 0x6f],
                yellow: [0xe8, 0xc7, 0x78],
                blue: [0x8d, 0xb7, 0xff],
                purple: [0xd1, 0xa3, 0xff],
                cyan: [0x7f, 0xd6, 0xc2],
                frame: [0x7e, 0x8b, 0x9d],
            },
            _ => panic!("retired themes map to the active catalog"),
        }
    }
}

pub(crate) fn fresh_terminal(name: ThemeName) -> TermTheme {
    let palette = name.fresh_palette().expect("fresh theme");
    let ansi = match name {
        ThemeName::NordLight => [
            0x2e3440, 0xa3434c, 0x506b3e, 0x826323, 0x4c6a87, 0x805578, 0x376d76, 0x4c566a,
            0x596579, 0x933e47, 0x466034, 0x74571c, 0x405d79, 0x71496a, 0x2d626b, 0x3b4252,
        ],
        ThemeName::Paper => [
            0x2f2e2b, 0xa34740, 0x576d46, 0x896327, 0x506d80, 0x805e78, 0x426f6a, 0x62594f,
            0x736e68, 0x943c36, 0x4a613b, 0x7b581f, 0x456172, 0x73516b, 0x37635e, 0x4a433b,
        ],
        ThemeName::CatppuccinMocha => [
            0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
            0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
        ],
        ThemeName::CatppuccinLatte => [
            0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be,
            0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
        ],
        // Official ANSI 0–15, including the distinct bright colors (2026-09-12).
        ThemeName::CatppuccinFrappe => [
            0x51576d, 0xe78284, 0xa6d189, 0xe5c890, 0x8caaee, 0xf4b8e4, 0x81c8be, 0xa5adce,
            0x626880, 0xe67172, 0x8ec772, 0xd9ba73, 0x7b9ef0, 0xf2a4db, 0x5abfb5, 0xb5bfe2,
        ],
        ThemeName::CatppuccinMacchiato => [
            0x494d64, 0xed8796, 0xa6da95, 0xeed49f, 0x8aadf4, 0xf5bde6, 0x8bd5ca, 0xa5adcb,
            0x5b6078, 0xec7486, 0x8ccf7f, 0xe1c682, 0x78a1f6, 0xf2a9dd, 0x63cbc0, 0xb8c0e0,
        ],
        ThemeName::GlassLight => [
            0x0f172a, 0xb91c1c, 0x15803d, 0x92600a, 0x4f46e5, 0x7e22ce, 0x0e7490, 0x475569,
            0x64748b, 0x991b1b, 0x166534, 0x854d0e, 0x4338ca, 0x6b21a8, 0x155e75, 0x334155,
        ],
        ThemeName::GlassDark => [
            0x252a35, 0xff8a8a, 0xa8d46f, 0xe8c778, 0x8db7ff, 0xd1a3ff, 0x7fd6c2, 0xe3e6f0,
            0x747b8e, 0xffb0a8, 0xc8ea90, 0xf2da9a, 0xb2ccff, 0xe0c2ff, 0xa3e6d8, 0xffffff,
        ],
        _ if palette.is_light => [
            0x26384a, 0xa34248, 0x28745e, 0x88621f, 0x306592, 0x77569b, 0x247782, 0x526375,
            0x667788, 0xb13d48, 0x247052, 0x826013, 0x295e9b, 0x80529a, 0x19717d, 0x34485c,
        ],
        _ => [
            0x263b49, 0xe49a9d, 0x9bc5a6, 0xe0c38b, 0x91bcdf, 0xc5addd, 0x91cecf, 0xdfeaf1,
            0x90a5b5, 0xf0b2b3, 0xb1dcc0, 0xeed4a5, 0xb0d1ed, 0xd9c4ed, 0xb0e1dc, 0xf2f7fa,
        ],
    }
    .map(rgb);
    // Preserve Paper's user-provided cursor and selection when reading old
    // preferences. New explicit theme choices still use the shared UI palette.
    let inherit_terminal_marks = name == ThemeName::Paper;
    TermTheme {
        background: palette.surface,
        is_light: palette.is_light,
        exact: Some(ExactTermColors {
            foreground: palette.foreground,
            ansi,
            cursor: (!inherit_terminal_marks).then_some(palette.accent),
            cursor_text: (!inherit_terminal_marks).then_some(palette.surface),
            cursor_stroke: (!inherit_terminal_marks).then_some(palette.accent),
            selection_foreground: (!inherit_terminal_marks).then_some(palette.foreground),
            selection_background: (!inherit_terminal_marks).then_some(palette.shell),
        }),
        powerline: [
            palette.accent,
            palette.surface,
            palette.shell,
            palette.foreground,
            palette.shell,
            palette.accent,
            palette.surface,
            palette.muted,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_light_themes_accept_old_preferences_and_use_canonical_names() {
        for (old, current, expected) in [
            ("Paper", "WarmSand", ThemeName::Paper),
            ("GlassLight", "SlateLight", ThemeName::GlassLight),
        ] {
            for value in [old, current] {
                let raw = crate::RawSettings::from_text(&format!("theme={value}\ncustom=keep\n"));
                assert_eq!(crate::RuntimeSettings::from_raw(&raw).theme, expected);
                assert_eq!(ThemeName::from_prompt_name(value), Some(expected));
            }
            assert_eq!(expected.prompt_name(), current);
            assert!(ThemeName::BUILTIN_NAMES.contains(&current));
            assert!(!ThemeName::BUILTIN_NAMES.contains(&old));
        }
        assert_eq!(ThemeName::default(), ThemeName::Nord);
        assert!(ThemeName::BUILTIN.contains(&ThemeName::NordLight));
        assert!(ThemeName::NordLight.term_theme().is_light);
    }

    #[test]
    fn fresh_themes_round_trip_and_keep_readable_foreground() {
        for name in [
            ThemeName::NordLight,
            ThemeName::Paper,
            ThemeName::GlassLight,
            ThemeName::BreezeLight,
            ThemeName::BreezeDark,
            ThemeName::MintLight,
            ThemeName::MintDark,
            ThemeName::CatppuccinFrappe,
            ThemeName::CatppuccinMacchiato,
        ] {
            assert_eq!(ThemeName::from_prompt_name(name.prompt_name()), Some(name));
            let palette = name.fresh_palette().unwrap();
            let terminal = name.term_theme();
            assert_eq!(terminal.background, palette.surface);
            assert_eq!(terminal.is_light, palette.is_light);
            let luma = |color: Rgb8| {
                color
                    .into_iter()
                    .zip([0.2126, 0.7152, 0.0722])
                    .map(|(c, weight)| {
                        let c = f64::from(c) / 255.0;
                        (if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) })
                            * weight
                    })
                    .sum::<f64>()
            };
            let a = luma(palette.foreground);
            let b = luma(palette.surface);
            assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 7.0, "{}", name.prompt_name());
        }
    }
}
