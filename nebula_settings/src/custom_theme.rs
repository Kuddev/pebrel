//! Value model for user-authored themes.
//!
//! This module deliberately contains no file format or serialization code.  An
//! importer/exporter can translate a file into [`ThemeDefinition`] at the
//! application boundary, while the settings crate remains a small, dependency
//! free authority for resolved theme values.

use core::fmt;

use crate::{
    BlurModeName, CursorShapeName, ExactTermColors, LIGHT_ANSI, LIGHT_FOREGROUND,
    MAX_PANE_CARD_DIVIDER, MAX_PANE_CARD_GUTTER, MAX_PANE_CARD_RADIUS, Rgb8, Rgba8,
    ThemeCardGeometry, ThemeName, format_hex_rgb, parse_hex_rgb,
};

/// A complete user theme.  `base` records the built-in palette used as the
/// starting point; it is metadata and does not create a runtime inheritance
/// chain.  A cloned definition owns all of its strings and arrays.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeDefinition {
    pub name: String,
    pub base: ThemeName,
    pub appearance: ThemeAppearance,
    pub terminal: TerminalThemeColors,
    pub ui: ThemeUiColors,
    pub typography: ThemeTypography,
    pub layout: ThemeLayout,
    pub effects: ThemeEffects,
}

impl ThemeDefinition {
    /// Resolve one of Nebula's existing themes into an editable snapshot.
    ///
    /// The snapshot is intentionally concrete: changing the built-in theme
    /// later cannot mutate an already imported or edited custom theme.
    pub fn from_builtin(base: ThemeName) -> Self {
        let term = base.term_theme();
        let palette = base.reviewed_palette();
        let exact = term.exact.unwrap_or_else(|| {
            if term.is_light {
                ExactTermColors {
                    foreground: LIGHT_FOREGROUND,
                    ansi: LIGHT_ANSI,
                    cursor: None,
                    cursor_text: None,
                    cursor_stroke: None,
                    selection_foreground: None,
                    selection_background: None,
                }
            } else {
                ExactTermColors {
                    foreground: palette.foreground,
                    ansi: fallback_ansi(palette),
                    cursor: Some(palette.accent),
                    cursor_text: Some(term.background),
                    cursor_stroke: Some(palette.accent),
                    selection_foreground: Some(palette.foreground),
                    selection_background: Some(palette.shell),
                }
            }
        });

        let mut indexed = IndexedPalette::from_ansi(exact.ansi);
        // These slots are part of Nebula's existing prompt contract.  Keeping
        // them in the snapshot means a custom theme recolors prompt chips and
        // the rest of the terminal from one value object.
        for (offset, color) in term.powerline.into_iter().enumerate() {
            indexed.colors[16 + offset] = color;
        }

        Self {
            name: base.prompt_name().to_owned(),
            base,
            appearance: if term.is_light { ThemeAppearance::Light } else { ThemeAppearance::Dark },
            terminal: TerminalThemeColors {
                background: term.background,
                foreground: exact.foreground,
                cursor: exact.cursor,
                cursor_text: exact.cursor_text,
                cursor_stroke: exact.cursor_stroke,
                selection_foreground: exact.selection_foreground,
                selection_background: exact.selection_background,
                palette: indexed,
            },
            ui: ThemeUiColors::from_palette(palette),
            typography: ThemeTypography::default(),
            layout: ThemeLayout { card: base.card_geometry(), padding_x: None, padding_y: None },
            effects: ThemeEffects::default(),
        }
    }

    /// Alias useful to adapters that call a built-in value a snapshot.
    pub fn snapshot(base: ThemeName) -> Self {
        Self::from_builtin(base)
    }

    /// Validate user-editable scalar values before applying or exporting them.
    pub fn validate(&self) -> Result<(), ThemeValidationError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(ThemeValidationError::EmptyName);
        }
        if name.len() > MAX_THEME_NAME_BYTES {
            return Err(ThemeValidationError::NameTooLong);
        }
        if self.name.chars().any(char::is_control) {
            return Err(ThemeValidationError::NameContainsControlCharacter);
        }
        self.typography.validate()?;
        self.layout.validate()?;
        self.effects.validate()?;
        Ok(())
    }

    /// Derive UI anchors from the resolved terminal values using the theme's
    /// declared light/dark appearance.  This is the one shared rule for
    /// `ui.derive`; adapters should call it instead of implementing their own
    /// background mixing or ANSI fallback.
    pub fn derived_ui(&self) -> ThemeUiColors {
        ThemeUiColors::from_terminal(&self.terminal, self.appearance)
    }

    /// Return the explicit UI values or the authoritative derived values,
    /// depending on the theme's `ui.derive` flag.
    pub fn resolved_ui(&self) -> ThemeUiColors {
        if self.ui.derive { self.derived_ui() } else { self.ui }
    }

    /// Return a copy with the terminal's default text color changed.
    pub fn with_terminal_foreground(&self, foreground: Rgb8) -> Self {
        let mut theme = self.clone();
        theme.terminal.foreground = foreground;
        theme
    }

    /// Return a copy with cursor colors changed.  `None` preserves the host
    /// cursor behavior when the theme does not declare a cursor override.
    pub fn with_cursor(
        &self,
        cursor: Option<Rgb8>,
        cursor_text: Option<Rgb8>,
        cursor_stroke: Option<Rgb8>,
    ) -> Self {
        let mut theme = self.clone();
        theme.terminal.cursor = cursor;
        theme.terminal.cursor_text = cursor_text;
        theme.terminal.cursor_stroke = cursor_stroke;
        theme
    }

    /// Return a copy with selection colors changed.
    pub fn with_selection(&self, foreground: Option<Rgb8>, background: Option<Rgb8>) -> Self {
        let mut theme = self.clone();
        theme.terminal.selection_foreground = foreground;
        theme.terminal.selection_background = background;
        theme
    }

    /// Return a copy with one ANSI slot changed.  ANSI slots stay mirrored in
    /// the first 16 indexed entries, so exporters cannot accidentally produce
    /// two different values for the same terminal color.
    pub fn with_ansi(&self, index: usize, color: Rgb8) -> Option<Self> {
        if index >= 16 {
            return None;
        }
        let mut theme = self.clone();
        theme.terminal.palette.set(index, color);
        Some(theme)
    }

    /// Return a copy with one indexed color changed.  Indices 0..15 are valid
    /// too and are treated as ANSI edits for consistency.
    pub fn with_indexed(&self, index: usize, color: Rgb8) -> Option<Self> {
        if index >= 256 {
            return None;
        }
        let mut theme = self.clone();
        theme.terminal.palette.set(index, color);
        Some(theme)
    }

    /// Parse a color in the same form used by `nebula_settings.txt`.
    pub fn parse_color(value: &str) -> Option<Rgb8> {
        parse_hex_rgb(value)
    }

    /// Format a color for a text or JSON adapter.
    pub fn format_color(color: Rgb8) -> String {
        format_hex_rgb(color)
    }
}

impl Default for ThemeDefinition {
    fn default() -> Self {
        Self::from_builtin(ThemeName::default())
    }
}

/// The appearance slot stored by a theme document.  System following remains
/// a separate runtime preference, so a theme snapshot always has a concrete
/// light or dark semantic when it is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeAppearance {
    Light,
    #[default]
    Dark,
}

impl ThemeAppearance {
    pub const fn is_light(self) -> bool {
        matches!(self, Self::Light)
    }

    pub const fn settings_value(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_settings(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// Terminal colors that are independent from the UI chrome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalThemeColors {
    pub background: Rgb8,
    pub foreground: Rgb8,
    pub cursor: Option<Rgb8>,
    pub cursor_text: Option<Rgb8>,
    pub cursor_stroke: Option<Rgb8>,
    pub selection_foreground: Option<Rgb8>,
    pub selection_background: Option<Rgb8>,
    /// ANSI 0..15 and indexed 16..255 in one fixed-size palette.
    pub palette: IndexedPalette,
}

impl TerminalThemeColors {
    pub fn ansi(&self, index: usize) -> Option<Rgb8> {
        self.palette.get(index)
    }

    pub fn indexed(&self, index: usize) -> Option<Rgb8> {
        self.palette.get(index)
    }
}

/// Complete xterm-style 256-color table.  The first 16 entries are the
/// theme's ANSI colors, while entries 16..255 are always present as well.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedPalette {
    pub colors: [Rgb8; 256],
}

impl IndexedPalette {
    pub fn from_ansi(ansi: [Rgb8; 16]) -> Self {
        let mut colors = [[0; 3]; 256];
        colors[..16].copy_from_slice(&ansi);

        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        for index in 16..232 {
            let cube = index - 16;
            colors[index] = [LEVELS[cube / 36], LEVELS[(cube / 6) % 6], LEVELS[cube % 6]];
        }
        for index in 232..256 {
            let level = 8 + ((index - 232) * 10) as u8;
            colors[index] = [level; 3];
        }
        Self { colors }
    }

    pub fn get(&self, index: usize) -> Option<Rgb8> {
        self.colors.get(index).copied()
    }

    pub fn set(&mut self, index: usize, color: Rgb8) -> bool {
        let Some(slot) = self.colors.get_mut(index) else { return false };
        *slot = color;
        true
    }

    pub fn ansi_colors(&self) -> [Rgb8; 16] {
        let mut ansi = [[0; 3]; 16];
        ansi.copy_from_slice(&self.colors[..16]);
        ansi
    }

    /// The complete table already includes every indexed value.
    pub fn indexed_colors(&self) -> &[Rgb8; 256] {
        &self.colors
    }
}

/// UI chrome semantic colors.  This list intentionally stays small; adapters
/// can derive additional disabled/hover colors from these stable anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeUiColors {
    /// Mirrors the native document's `ui.derive` flag.
    pub derive: bool,
    pub shell: Rgb8,
    pub background: Rgb8,
    pub foreground: Rgb8,
    pub muted: Rgb8,
    pub accent: Rgb8,
    pub selection: Rgba8,
    pub line: Rgba8,
    pub frame: Rgb8,
    pub error: Rgb8,
    pub success: Rgb8,
    pub warning: Rgb8,
    pub info: Rgb8,
}

impl ThemeUiColors {
    /// Build UI anchors from the reviewed built-in palette.
    pub fn from_palette(palette: crate::ReviewedPalette) -> Self {
        Self {
            derive: false,
            shell: palette.shell,
            background: palette.background,
            foreground: palette.foreground,
            muted: palette.muted,
            accent: palette.accent,
            selection: palette.selected,
            line: palette.line,
            frame: palette.frame,
            error: palette.red,
            success: palette.green,
            warning: palette.yellow,
            info: palette.blue,
        }
    }

    /// Build UI anchors from a complete terminal theme.  The terminal
    /// foreground/background remain byte-for-byte untouched; only UI semantic
    /// fallbacks are derived.  `appearance` controls the small selection
    /// surface blend used when the terminal has no explicit selection color.
    pub fn from_terminal(terminal: &TerminalThemeColors, appearance: ThemeAppearance) -> Self {
        let background = terminal.background;
        let foreground = terminal.foreground;
        let muted = terminal.palette.colors[8];
        let accent =
            terminal.cursor.or(terminal.cursor_stroke).unwrap_or(terminal.palette.colors[14]);
        let selection = terminal.selection_background.map_or_else(
            || {
                let weight = if appearance.is_light() { 0.13 } else { 0.18 };
                let rgb = mix_rgb(background, foreground, weight);
                [rgb[0], rgb[1], rgb[2], 255]
            },
            |rgb| [rgb[0], rgb[1], rgb[2], 255],
        );
        let shell = match appearance {
            ThemeAppearance::Light => mix_rgb(background, [0, 0, 0], 0.06),
            ThemeAppearance::Dark => mix_rgb(background, [255, 255, 255], 0.06),
        };
        Self {
            derive: true,
            shell,
            background,
            foreground,
            muted,
            accent,
            selection,
            line: [muted[0], muted[1], muted[2], 255],
            frame: muted,
            error: terminal.palette.colors[9],
            success: terminal.palette.colors[10],
            warning: terminal.palette.colors[11],
            info: terminal.palette.colors[14],
        }
    }

    pub fn with_derive(mut self, derive: bool) -> Self {
        self.derive = derive;
        self
    }

    /// RGB components for backends that apply alpha themselves.
    pub const fn selection_rgb(self) -> Rgb8 {
        [self.selection[0], self.selection[1], self.selection[2]]
    }

    pub const fn line_rgb(self) -> Rgb8 {
        [self.line[0], self.line[1], self.line[2]]
    }
}

/// Optional typography overrides; `None` retains the user's global setting.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeTypography {
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub line_height: Option<f32>,
    pub letter_spacing: Option<f32>,
    pub font_weight: Option<u16>,
    /// Whether terminal text shaping may use ligatures.  Built-in themes keep
    /// the existing enabled default; imported documents can explicitly turn
    /// it off without losing that choice at the JSON boundary.
    pub ligatures: bool,
    /// Optional UI/chrome font override.  The GPUI adapter may choose to
    /// consume this later, but the shared model must retain it now so an
    /// import/export cycle is lossless.
    pub ui_font_family: Option<String>,
    pub ui_font_size: Option<f32>,
}

impl Default for ThemeTypography {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: None,
            line_height: None,
            letter_spacing: None,
            font_weight: None,
            ligatures: true,
            ui_font_family: None,
            ui_font_size: None,
        }
    }
}

impl ThemeTypography {
    fn validate(&self) -> Result<(), ThemeValidationError> {
        if self.font_family.as_deref().is_some_and(|family| {
            family.trim().is_empty()
                || family.len() > MAX_FONT_FAMILY_BYTES
                || family.chars().any(char::is_control)
        }) {
            return Err(ThemeValidationError::InvalidFontFamily);
        }
        if self.ui_font_family.as_deref().is_some_and(|family| {
            family.trim().is_empty()
                || family.len() > MAX_FONT_FAMILY_BYTES
                || family.chars().any(char::is_control)
        }) {
            return Err(ThemeValidationError::InvalidUiFontFamily);
        }
        if let Some(size) = self.font_size {
            if !size.is_finite() || !(4.0..=96.0).contains(&size) {
                return Err(ThemeValidationError::InvalidFontSize);
            }
        }
        if let Some(line_height) = self.line_height {
            if !line_height.is_finite() || !(0.5..=3.0).contains(&line_height) {
                return Err(ThemeValidationError::InvalidLineHeight);
            }
        }
        if let Some(letter_spacing) = self.letter_spacing {
            if !letter_spacing.is_finite() || !(0.0..=8.0).contains(&letter_spacing) {
                return Err(ThemeValidationError::InvalidLetterSpacing);
            }
        }
        if let Some(weight) = self.font_weight {
            if !(100..=900).contains(&weight) {
                return Err(ThemeValidationError::InvalidFontWeight);
            }
        }
        if let Some(size) = self.ui_font_size {
            if !size.is_finite() || !(10.0..=20.0).contains(&size) {
                return Err(ThemeValidationError::InvalidUiFontSize);
            }
        }
        Ok(())
    }
}

/// Layout values that belong to a theme rather than to a user's window size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeLayout {
    pub card: ThemeCardGeometry,
    pub padding_x: Option<f32>,
    pub padding_y: Option<f32>,
}

impl ThemeLayout {
    fn validate(&self) -> Result<(), ThemeValidationError> {
        let card = self.card;
        if !card.radius.is_finite() || !(0.0..=MAX_PANE_CARD_RADIUS).contains(&card.radius) {
            return Err(ThemeValidationError::InvalidLayoutRadius);
        }
        if !card.gutter.is_finite() || !(0.0..=MAX_PANE_CARD_GUTTER).contains(&card.gutter) {
            return Err(ThemeValidationError::InvalidLayoutGutter);
        }
        if !card.divider.is_finite() || !(0.0..=MAX_PANE_CARD_DIVIDER).contains(&card.divider) {
            return Err(ThemeValidationError::InvalidLayoutDivider);
        }
        if self
            .padding_x
            .is_some_and(|padding| !padding.is_finite() || !(0.0..=48.0).contains(&padding))
        {
            return Err(ThemeValidationError::InvalidLayoutPaddingX);
        }
        if self
            .padding_y
            .is_some_and(|padding| !padding.is_finite() || !(0.0..=40.0).contains(&padding))
        {
            return Err(ThemeValidationError::InvalidLayoutPaddingY);
        }
        Ok(())
    }
}

/// Optional visual effects; paths and fit/alignment are renderer-owned values.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeEffects {
    pub opacity: Option<f32>,
    pub blur: Option<BlurModeName>,
    pub cursor_shape: Option<CursorShapeName>,
    pub background_image: Option<String>,
    pub background_image_opacity: Option<f32>,
    pub background_image_fit: Option<String>,
    pub background_image_alignment: Option<String>,
    pub background_image_cover_chrome: Option<bool>,
}

impl Default for ThemeEffects {
    fn default() -> Self {
        Self {
            opacity: None,
            blur: None,
            cursor_shape: None,
            background_image: None,
            background_image_opacity: None,
            background_image_fit: None,
            background_image_alignment: None,
            background_image_cover_chrome: None,
        }
    }
}

impl ThemeEffects {
    fn validate(&self) -> Result<(), ThemeValidationError> {
        if let Some(opacity) = self.opacity {
            if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                return Err(ThemeValidationError::InvalidOpacity);
            }
        }
        if let Some(opacity) = self.background_image_opacity {
            if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                return Err(ThemeValidationError::InvalidBackgroundImageOpacity);
            }
        }
        for value in [
            self.background_image.as_deref(),
            self.background_image_fit.as_deref(),
            self.background_image_alignment.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.chars().any(char::is_control) {
                return Err(ThemeValidationError::InvalidEffectText);
            }
        }
        Ok(())
    }
}

const MAX_THEME_NAME_BYTES: usize = 128;
const MAX_FONT_FAMILY_BYTES: usize = 256;

/// Reasons a theme cannot safely be applied or exported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeValidationError {
    EmptyName,
    NameTooLong,
    NameContainsControlCharacter,
    InvalidFontFamily,
    InvalidFontSize,
    InvalidLineHeight,
    InvalidLetterSpacing,
    InvalidFontWeight,
    InvalidUiFontFamily,
    InvalidUiFontSize,
    InvalidLayoutRadius,
    InvalidLayoutGutter,
    InvalidLayoutDivider,
    InvalidLayoutPaddingX,
    InvalidLayoutPaddingY,
    InvalidOpacity,
    InvalidBackgroundImageOpacity,
    InvalidEffectText,
}

impl fmt::Display for ThemeValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyName => "theme name is empty",
            Self::NameTooLong => "theme name is too long",
            Self::NameContainsControlCharacter => "theme name contains a control character",
            Self::InvalidFontFamily => {
                "font family is empty, too long, or contains a control character"
            },
            Self::InvalidFontSize => "font size must be finite and between 4 and 96",
            Self::InvalidLineHeight => "line height must be finite and between 0.5 and 3",
            Self::InvalidLetterSpacing => "letter spacing must be finite and between 0 and 8",
            Self::InvalidFontWeight => "font weight must be between 100 and 900",
            Self::InvalidUiFontFamily => {
                "UI font family is empty, too long, or contains a control character"
            },
            Self::InvalidUiFontSize => "UI font size must be finite and between 10 and 20",
            Self::InvalidLayoutRadius => "card radius is outside the shared pane range",
            Self::InvalidLayoutGutter => "card gutter is outside the shared pane range",
            Self::InvalidLayoutDivider => "card divider is outside the shared pane range",
            Self::InvalidLayoutPaddingX => "horizontal layout padding is outside the theme range",
            Self::InvalidLayoutPaddingY => "vertical layout padding is outside the theme range",
            Self::InvalidOpacity => "opacity must be finite and between 0 and 1",
            Self::InvalidBackgroundImageOpacity => {
                "background image opacity must be finite and between 0 and 1"
            },
            Self::InvalidEffectText => "effect text contains a control character",
        };
        f.write_str(message)
    }
}

impl std::error::Error for ThemeValidationError {}

fn fallback_ansi(palette: crate::ReviewedPalette) -> [Rgb8; 16] {
    [
        palette.shell,
        palette.red,
        palette.green,
        palette.yellow,
        palette.blue,
        palette.purple,
        palette.cyan,
        palette.foreground,
        palette.frame,
        palette.red,
        palette.green,
        palette.yellow,
        palette.blue,
        palette.purple,
        palette.cyan,
        palette.foreground,
    ]
}

fn mix_rgb(left: Rgb8, right: Rgb8, weight: f32) -> Rgb8 {
    [0, 1, 2].map(|index| {
        (f32::from(left[index]) * (1.0 - weight) + f32::from(right[index]) * weight).round() as u8
    })
}

/// Return the original foreground plus cool and warm readable suggestions.
///
/// The first entry is always `original`, even when it is low contrast.  The
/// second and third entries are checked against the supplied opaque background
/// with WCAG 2 AA's 4.5:1 normal-text threshold.  This is a default foreground
/// and background reference; composited or translucent surfaces still require
/// the caller to verify the actual rendered result, and arbitrary user colors
/// are never silently rewritten.
pub fn foreground_recommendations(background: Rgb8, original: Rgb8) -> [Rgb8; 3] {
    [
        original,
        readable_tint(background, [100, 178, 224]),
        readable_tint(background, [235, 164, 76]),
    ]
}

/// WCAG relative contrast ratio for two opaque sRGB colors.
pub fn wcag_contrast_ratio(foreground: Rgb8, background: Rgb8) -> f64 {
    let a = relative_luminance(foreground);
    let b = relative_luminance(background);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Whether an opaque foreground/background pair reaches WCAG AA normal text.
pub fn meets_wcag_aa(foreground: Rgb8, background: Rgb8) -> bool {
    wcag_contrast_ratio(foreground, background) >= 4.5
}

fn readable_tint(background: Rgb8, tint: Rgb8) -> Rgb8 {
    let white = [255; 3];
    let black = [0; 3];
    let endpoint =
        if wcag_contrast_ratio(white, background) >= wcag_contrast_ratio(black, background) {
            white
        } else {
            black
        };

    if meets_wcag_aa(tint, background) {
        return tint;
    }
    // Move toward the better contrast endpoint while retaining as much of the
    // requested cool/warm hue as the contrast floor permits.
    for step in 1..=255u16 {
        let candidate = blend_rgb(tint, endpoint, step as u8);
        if meets_wcag_aa(candidate, background) {
            return candidate;
        }
    }
    endpoint
}

fn blend_rgb(from: Rgb8, to: Rgb8, amount: u8) -> Rgb8 {
    let amount = u16::from(amount);
    let mut result = [0; 3];
    for index in 0..3 {
        let from = u16::from(from[index]);
        let to = u16::from(to[index]);
        result[index] = ((from * (255 - amount) + to * amount + 127) / 255) as u8;
    }
    result
}

fn relative_luminance(color: Rgb8) -> f64 {
    let linear = |channel: u8| {
        let channel = f64::from(channel) / 255.0;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    linear(color[0]) * 0.2126 + linear(color[1]) * 0.7152 + linear(color[2]) * 0.0722
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_snapshot_keeps_declared_exact_colors() {
        let nord = ThemeDefinition::from_builtin(ThemeName::Nord);
        let exact = ThemeName::Nord.term_theme().exact.unwrap();
        assert_eq!(nord.appearance, ThemeAppearance::Dark);
        assert_eq!(nord.terminal.foreground, exact.foreground);
        assert_eq!(nord.terminal.cursor, exact.cursor);
        assert_eq!(nord.terminal.selection_background, exact.selection_background);
        assert_eq!(nord.terminal.palette.ansi_colors(), exact.ansi);
        assert_eq!(nord.terminal.palette.colors[16], ThemeName::Nord.term_theme().powerline[0]);
        assert_eq!(nord.ui.selection[3], 255);
        assert_eq!(nord.ui.line[3], 255);

        for name in ThemeName::BUILTIN {
            let Some(exact) = name.term_theme().exact else { continue };
            let snapshot = ThemeDefinition::from_builtin(name);
            assert_eq!(snapshot.terminal.foreground, exact.foreground, "{}", name.prompt_name());
            assert_eq!(
                snapshot.terminal.palette.ansi_colors(),
                exact.ansi,
                "{}",
                name.prompt_name()
            );
            assert_eq!(snapshot.terminal.cursor, exact.cursor, "{}", name.prompt_name());
            assert_eq!(
                snapshot.terminal.selection_background,
                exact.selection_background,
                "{}",
                name.prompt_name()
            );
        }
    }

    #[test]
    fn ui_derivation_and_explicit_alpha_are_available_to_adapters() {
        let mut theme = ThemeDefinition::from_builtin(ThemeName::Paper);
        assert_eq!(theme.appearance, ThemeAppearance::Light);
        theme.ui.derive = true;
        assert!(theme.resolved_ui().derive);
        assert_eq!(theme.resolved_ui().selection[3], 255);

        let palette = ThemeName::Nord.reviewed_palette();
        assert_eq!(ThemeUiColors::from_palette(palette).selection, palette.selected);
        assert_eq!(ThemeUiColors::from_palette(palette).line, palette.line);
    }

    #[test]
    fn indexed_palette_is_complete_and_ansi_is_mirrored() {
        let theme = ThemeDefinition::from_builtin(ThemeName::CatppuccinMocha);
        let ansi = theme.terminal.palette.ansi_colors();
        assert_eq!(&ansi[..], &theme.terminal.palette.colors[..16]);
        assert!(theme.terminal.palette.get(255).is_some());
        assert_eq!(theme.terminal.palette.colors[232], [8; 3]);
        assert_eq!(theme.terminal.palette.colors[255], [238; 3]);
    }

    #[test]
    fn edits_clone_without_mutating_the_source() {
        let source = ThemeDefinition::from_builtin(ThemeName::Nord);
        let edited = source.with_terminal_foreground([1, 2, 3]);
        assert_eq!(source.terminal.foreground, [0xf1, 0xf6, 0xff]);
        assert_eq!(edited.terminal.foreground, [1, 2, 3]);
        let edited = source.with_ansi(0, [9, 8, 7]).unwrap();
        assert_eq!(source.terminal.palette.colors[0], [0x3b, 0x42, 0x52]);
        assert_eq!(edited.terminal.palette.colors[0], [9, 8, 7]);
        assert!(source.with_ansi(16, [0, 0, 0]).is_none());
        assert!(source.with_indexed(256, [0, 0, 0]).is_none());
        let mut palette = source.terminal.palette.clone();
        assert!(!palette.set(256, [0, 0, 0]));
    }

    #[test]
    fn validate_rejects_bad_scalars() {
        let mut theme = ThemeDefinition::from_builtin(ThemeName::Nord);
        theme.effects.opacity = Some(f32::NAN);
        assert_eq!(theme.validate(), Err(ThemeValidationError::InvalidOpacity));
        theme.effects.opacity = None;
        theme.typography.font_size = Some(2.0);
        assert_eq!(theme.validate(), Err(ThemeValidationError::InvalidFontSize));
    }

    #[test]
    fn recommendations_keep_original_and_meet_wcag_for_solid_backgrounds() {
        for background in [[0, 0, 0], [15, 17, 26], [255, 255, 255], [127, 127, 127]] {
            let recommendations = foreground_recommendations(background, [1, 2, 3]);
            assert_eq!(recommendations[0], [1, 2, 3]);
            assert!(meets_wcag_aa(recommendations[1], background));
            assert!(meets_wcag_aa(recommendations[2], background));
        }
    }
}
