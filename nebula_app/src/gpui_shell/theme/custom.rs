//! Runtime projection of a validated custom theme.
//!
//! Theme files and format adapters stay outside the render path.  This module
//! owns the small amount of derivation needed by GPUI after a snapshot has
//! already been loaded and validated.

use nebula_settings::{Rgb8, RuntimeSettings, ThemeCardGeometry, ThemeDefinition, ThemeName};
use std::sync::{Mutex, OnceLock};

use crate::display::color::Rgb;
use crate::display::ui::theme::{NebulaPalette, NebulaTheme, Skin};
use crate::renderer::ui::Rgba;

/// The immutable theme snapshot consumed by configuration and chrome helpers.
///
/// `custom` is a complete value, never a reference to a base theme.  `base`
/// remains available for compatibility features whose native representation is
/// still the built-in enum (syntax fallback and system appearance pairing).
#[derive(Clone, Debug)]
pub(crate) struct ResolvedTheme {
    base: ThemeName,
    custom: Option<ThemeDefinition>,
    foreground_override: Option<Rgb8>,
    pub(crate) notice: Option<String>,
    /// All chrome derivation happens while a snapshot is created. Renderers
    /// only copy these small value objects on the frame path.
    palette: NebulaPalette,
    skin: Skin,
}

impl ResolvedTheme {
    /// Resolve the persisted runtime selection once per settings load. The
    /// render path retains the returned snapshot and never reopens the theme
    /// store. `custom_theme` accepts both a library id and the JSON snapshot
    /// written by older editor builds.
    pub(crate) fn from_runtime(runtime: &RuntimeSettings, fallback: ThemeName) -> Self {
        let Some(reference) =
            runtime.custom_theme.as_deref().filter(|value| !value.trim().is_empty())
        else {
            return Self::builtin(fallback, runtime.theme_foreground);
        };

        match load_definition(reference) {
            Ok(definition) => {
                remember_definition(reference, &definition);
                Self::custom(definition, runtime.theme_foreground, None)
            },
            Err(error) => {
                let notice = format!("Custom theme could not be loaded: {error}");
                if let Some(previous) = previous_definition(reference) {
                    return Self::custom(previous, runtime.theme_foreground, Some(notice));
                }
                let mut builtin = Self::builtin(fallback, runtime.theme_foreground);
                builtin.notice = Some(notice);
                builtin
            },
        }
    }

    pub(crate) fn builtin(base: ThemeName, foreground_override: Option<Rgb8>) -> Self {
        Self {
            base,
            custom: None,
            foreground_override,
            notice: None,
            palette: crate::gpui_shell::theme::chrome_theme(base).palette(),
            skin: crate::gpui_shell::theme::chrome_theme(base).skin(),
        }
    }

    pub(crate) fn custom(
        definition: ThemeDefinition,
        foreground_override: Option<Rgb8>,
        notice: Option<String>,
    ) -> Self {
        let palette = custom_palette(&definition);
        let skin = custom_skin(&definition);
        Self {
            base: definition.base,
            custom: Some(definition),
            foreground_override,
            notice,
            palette,
            skin,
        }
    }

    pub(crate) fn base_name(&self) -> ThemeName {
        self.base
    }

    pub(crate) fn definition(&self) -> Option<&ThemeDefinition> {
        self.custom.as_ref()
    }

    pub(crate) fn is_custom(&self) -> bool {
        self.custom.is_some()
    }

    pub(crate) fn foreground_override(&self) -> Option<Rgb8> {
        self.foreground_override
    }

    pub(crate) fn is_light(&self) -> bool {
        self.custom.as_ref().map_or_else(
            || self.base.term_theme().is_light,
            |definition| definition.appearance.is_light(),
        )
    }

    pub(crate) fn terminal_background(&self) -> Rgb8 {
        self.custom
            .as_ref()
            .map(|definition| definition.terminal.background)
            .unwrap_or(self.base.term_theme().background)
    }

    pub(crate) fn terminal_foreground(&self) -> Rgb8 {
        self.foreground_override.unwrap_or_else(|| {
            self.custom
                .as_ref()
                .map(|definition| definition.terminal.foreground)
                .unwrap_or_else(|| builtin_foreground(self.base))
        })
    }

    pub(crate) fn terminal_cursor(&self) -> Option<Rgb8> {
        self.custom.as_ref().map_or_else(
            || self.base.term_theme().exact.and_then(|colors| colors.cursor),
            |definition| definition.terminal.cursor,
        )
    }

    pub(crate) fn terminal_cursor_text(&self) -> Option<Rgb8> {
        self.custom.as_ref().map_or_else(
            || self.base.term_theme().exact.and_then(|colors| colors.cursor_text),
            |definition| definition.terminal.cursor_text,
        )
    }

    pub(crate) fn terminal_cursor_stroke(&self) -> Option<Rgb8> {
        self.custom.as_ref().map_or_else(
            || self.base.term_theme().exact.and_then(|colors| colors.cursor_stroke),
            |definition| definition.terminal.cursor_stroke,
        )
    }

    pub(crate) fn terminal_selection(&self) -> (Option<Rgb8>, Option<Rgb8>) {
        self.custom.as_ref().map_or_else(
            || {
                self.base
                    .term_theme()
                    .exact
                    .map(|colors| (colors.selection_background, colors.selection_foreground))
                    .unwrap_or((None, None))
            },
            |definition| {
                (definition.terminal.selection_background, definition.terminal.selection_foreground)
            },
        )
    }

    pub(crate) fn ansi(&self) -> [Rgb8; 16] {
        if let Some(definition) = &self.custom {
            return definition.terminal.palette.ansi_colors();
        }
        self.base.term_theme().exact.map(|colors| colors.ansi).unwrap_or_else(|| {
            if self.base.term_theme().is_light {
                nebula_settings::LIGHT_ANSI
            } else {
                // A dark theme without an explicit palette keeps the
                // existing user/config palette.  This value is used only
                // by callers that need a complete snapshot.
                nebula_settings::ThemeDefinition::from_builtin(self.base)
                    .terminal
                    .palette
                    .ansi_colors()
            }
        })
    }

    pub(crate) fn indexed(&self, index: usize) -> Option<Rgb8> {
        if let Some(definition) = &self.custom {
            return definition.terminal.palette.get(index);
        }
        self.base.term_theme().powerline.get(index.checked_sub(16)?).copied()
    }

    pub(crate) fn card_geometry(&self) -> ThemeCardGeometry {
        self.custom
            .as_ref()
            .map(|definition| definition.layout.card)
            .unwrap_or_else(|| self.base.card_geometry())
    }

    pub(crate) fn typography(&self) -> Option<&nebula_settings::ThemeTypography> {
        self.custom.as_ref().map(|definition| &definition.typography)
    }

    pub(crate) fn effects(&self) -> Option<&nebula_settings::ThemeEffects> {
        self.custom.as_ref().map(|definition| &definition.effects)
    }

    pub(crate) fn effect_opacity(&self) -> Option<f32> {
        self.effects().and_then(|effects| effects.opacity)
    }

    pub(crate) fn effect_blur(&self) -> Option<nebula_settings::BlurModeName> {
        self.effects().and_then(|effects| effects.blur)
    }

    /// Resolve a material value once for the current runtime snapshot. An
    /// explicit user value remains authoritative, including `1.0` opacity
    /// and `none` blur; an absent value may inherit the custom theme default.
    pub(crate) fn effective_opacity(&self, runtime: &RuntimeSettings) -> f32 {
        if runtime.opacity_is_explicit() {
            runtime.opacity
        } else {
            self.effect_opacity().unwrap_or(runtime.opacity)
        }
    }

    pub(crate) fn effective_blur(
        &self,
        runtime: &RuntimeSettings,
    ) -> nebula_settings::BlurModeName {
        if runtime.blur_is_explicit() {
            runtime.blur
        } else {
            self.effect_blur().unwrap_or(runtime.blur)
        }
    }

    pub(crate) fn chrome_palette(&self) -> NebulaPalette {
        self.palette
    }

    pub(crate) fn skin(&self) -> Skin {
        self.skin
    }

    pub(crate) fn chrome_theme(&self) -> NebulaTheme {
        crate::gpui_shell::theme::chrome_theme(self.base)
    }
}

fn load_definition(reference: &str) -> Result<ThemeDefinition, String> {
    let reference = reference.trim();
    let document = if reference.starts_with('{') {
        crate::theme_library::ThemeDocument::from_json_bytes(reference.as_bytes())
            .map_err(|error| error.to_string())?
    } else {
        let root = crate::platform::dirs::data_dir().join("themes");
        crate::theme_library::ThemeLibraryStore::new(root)
            .load(reference)
            .map_err(|error| error.to_string())?
    };
    document.definition().map_err(|error| error.to_string())
}

#[derive(Clone)]
struct CachedTheme {
    reference: String,
    definition: ThemeDefinition,
}

fn previous_definition(reference: &str) -> Option<ThemeDefinition> {
    last_valid_theme().lock().ok().and_then(|value| {
        value
            .as_ref()
            .filter(|cached| cached.reference == reference)
            .map(|cached| cached.definition.clone())
    })
}

fn remember_definition(reference: &str, definition: &ThemeDefinition) {
    if let Ok(mut value) = last_valid_theme().lock() {
        *value =
            Some(CachedTheme { reference: reference.to_owned(), definition: definition.clone() });
    }
}

fn last_valid_theme() -> &'static Mutex<Option<CachedTheme>> {
    static LAST_VALID: OnceLock<Mutex<Option<CachedTheme>>> = OnceLock::new();
    LAST_VALID.get_or_init(|| Mutex::new(None))
}

fn custom_palette(definition: &ThemeDefinition) -> NebulaPalette {
    let ui = definition.resolved_ui();
    let line = ui.line_rgb();
    let selection = ui.selection_rgb();
    NebulaPalette {
        panel: rgba(ui.background, 255),
        pill: rgba(ui.background, 255),
        tab_stroke_l: rgba(line, ui.line[3]),
        tab_bg_l: rgba(selection, ui.selection[3]),
        tab_bg_r: rgba(selection, ui.selection[3]),
        edge_l: rgba(ui.accent, 255),
        edge_r: rgba(ui.accent, 255),
        edge_glow_l: rgba(ui.accent, 0),
        glow_l: rgba(ui.accent, 0),
        glow_r: rgba(ui.accent, 0),
        is_light: definition.appearance.is_light(),
        term_bg: rgb(definition.terminal.background),
        shell_bg: rgb(ui.shell),
    }
}

fn custom_skin(definition: &ThemeDefinition) -> Skin {
    let ui = definition.resolved_ui();
    let light = definition.appearance.is_light();
    let background = ui.background;
    let input = blend(background, definition.terminal.background, 0.22);
    let muted = ui.muted;
    let faint = blend(muted, background, 0.42);
    let ink_on_accent = if contrast(ui.accent, [255; 3]) >= contrast(ui.accent, [0; 3]) {
        [255; 3]
    } else {
        [0; 3]
    };
    let soft = ui.selection_rgb();
    let line = ui.line_rgb();
    let selection_alpha = ui.selection[3];
    let line_alpha = ui.line[3];
    let track_off =
        if light { blend(line, background, 0.34) } else { blend(line, background, 0.62) };
    let knob_off =
        if light { blend(ui.muted, [255; 3], 0.45) } else { blend(ui.muted, [0; 3], 0.35) };
    Skin {
        panel: rgba(background, 255),
        input: rgba(input, 255),
        card: rgba(ui.shell, 255),
        veil: if light { rgba([15, 23, 42], 44) } else { rgba([2, 6, 23], 92) },
        ink: rgb(ui.foreground),
        ink_dim: rgb(muted),
        ink_strong: rgb(ui.foreground),
        ink_faint: rgb(faint),
        ink_ignored: rgb(faint),
        ink_on_accent: rgb(ink_on_accent),
        icon: rgb(muted),
        icon_hover: rgb(ui.foreground),
        accent: rgb(ui.accent),
        accent_soft: rgba(soft, selection_alpha),
        danger: rgba(ui.error, 255),
        ok: rgba(ui.success, 255),
        warn: rgba(ui.warning, 255),
        hairline: rgba(line, line_alpha),
        surface: rgba(soft, scaled_alpha(selection_alpha, if light { 0.56 } else { 0.38 })),
        hover: rgba(soft, scaled_alpha(selection_alpha, if light { 0.78 } else { 0.70 })),
        hover_strong: rgba(soft, scaled_alpha(selection_alpha, if light { 1.0 } else { 0.92 })),
        track_off: rgba(track_off, 180),
        toggle_track_off: rgba(track_off, 255),
        toggle_track_on: rgba(ui.accent, 255),
        toggle_border_off: rgba(line, 255),
        toggle_border_on: rgba(ui.accent, 255),
        knob_off: rgba(knob_off, 255),
        knob_on: rgba(ink_on_accent, 255),
        scrollbar_thumb: rgba(muted, 96),
        is_light: light,
    }
}

fn scaled_alpha(alpha: u8, factor: f32) -> u8 {
    (f32::from(alpha) * factor).round().clamp(0.0, 255.0) as u8
}

fn builtin_foreground(name: ThemeName) -> Rgb8 {
    let term = name.term_theme();
    term.exact.map(|colors| colors.foreground).unwrap_or_else(|| {
        if term.is_light {
            nebula_settings::LIGHT_FOREGROUND
        } else {
            let p = name.reviewed_palette();
            p.foreground
        }
    })
}

fn rgb([r, g, b]: Rgb8) -> Rgb {
    Rgb::new(r, g, b)
}

fn rgba([r, g, b]: Rgb8, a: u8) -> Rgba {
    Rgba::new(r, g, b, a)
}

fn contrast(left: Rgb8, right: Rgb8) -> f32 {
    let luma = |color: Rgb8| {
        0.2126 * f32::from(color[0]) + 0.7152 * f32::from(color[1]) + 0.0722 * f32::from(color[2])
    };
    (luma(left) - luma(right)).abs()
}

fn blend(left: Rgb8, right: Rgb8, amount: f32) -> Rgb8 {
    [0, 1, 2].map(|index| {
        (f32::from(left[index]) * (1.0 - amount) + f32::from(right[index]) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}
