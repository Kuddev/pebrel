//! Terminal font construction and cell metrics.
//!
//! The terminal view and its element must use one device-pixel rounding rule.
//! Keeping shaping, width, height, and theme line-height policy here prevents
//! the startup grid from drifting away from the first prepainted frame.

use gpui::{
    App, Font, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, SharedString, TextRun, Window,
    WindowTextSystem, px,
};
use nebula_settings::CellWidthModeName;

use super::Settings;
use crate::font_install::{REQUIRED_FONT_FAMILY, gpui_font_with_fallbacks};

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

/// Set every ligature feature explicitly, including the off case: platform
/// defaults differ, and the Windows backend ignores an empty feature list.
pub(super) fn mono_font(
    family: &str,
    weight: FontWeight,
    style: FontStyle,
    ligatures: bool,
) -> Font {
    let enabled = u32::from(ligatures);
    Font {
        weight,
        style,
        features: FontFeatures(std::sync::Arc::new(vec![
            ("calt".to_owned(), enabled),
            ("liga".to_owned(), enabled),
            ("clig".to_owned(), enabled),
            ("kern".to_owned(), 0),
        ])),
        ..gpui_font_with_fallbacks(family)
    }
}

/// Apply the legacy device-pixel rounding contract to one shaped cell width.
pub(super) fn effective_cell_width(
    raw_width: f32,
    mode: CellWidthModeName,
    scale: f32,
    offset_x: f32,
) -> Pixels {
    let scale = scale.max(0.5);
    let device_width = raw_width * scale + offset_x;
    let device_width = match mode {
        CellWidthModeName::Compact => device_width.floor(),
        CellWidthModeName::Relaxed => device_width.round(),
    };
    px(device_width.max(1.0) / scale)
}

/// Convert a logical line height plus an explicit device-pixel adjustment.
pub(super) fn effective_line_height(height: f32, offset_y: f32, scale: f32) -> Pixels {
    let scale = scale.max(0.5);
    px(((height * scale + offset_y).floor().max(1.0)) / scale)
}

/// A terminal row defaults to 1.2em, independently of a fallback font's hhea
/// padding. With the bundled 0.6em monospace advance this gives a 1:2 grid
/// before pixel rounding, as expected by block and Braille character art.
/// This changes row spacing, not font size or glyph outlines. Text keeps its
/// shaped ascent/descent for baseline placement and is not clipped per cell.
/// Explicit theme line height and font.offset remain authoritative.
pub(super) fn effective_line_height_with_theme(
    font_size: f32,
    line_height_multiplier: Option<f32>,
    offset_y: f32,
    scale: f32,
) -> Pixels {
    let target = (font_size * line_height_multiplier.unwrap_or(1.2)).max(1.0);
    effective_line_height(target, offset_y, scale)
}

fn measure_cell_metrics(
    text_system: &WindowTextSystem,
    scale: f32,
    family: &str,
    font_size: Pixels,
    mode: CellWidthModeName,
    offset_x: f32,
    offset_y: f32,
    line_height_multiplier: Option<f32>,
) -> (Pixels, Pixels) {
    let font = mono_font(family, FontWeight::NORMAL, FontStyle::Normal, false);
    let sample = text_system.shape_line(
        SharedString::new_static("M"),
        font_size,
        &[TextRun {
            len: 1,
            font,
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    (
        effective_cell_width(sample.width.as_f32(), mode, scale, offset_x),
        effective_line_height_with_theme(
            font_size.as_f32(),
            line_height_multiplier,
            offset_y,
            scale,
        ),
    )
}

pub(super) fn cell_metrics(window: &Window, cx: &App) -> (Pixels, Pixels) {
    let (family, font_size, mode, offset_x, offset_y, line_height_multiplier) =
        match cx.try_global::<Settings>() {
            Some(settings) => (
                settings.font_family.as_str(),
                settings.font_size_px,
                settings.cell_width_mode,
                settings.font_offset_x,
                settings.font_offset_y,
                settings.theme_line_height,
            ),
            None => (REQUIRED_FONT_FAMILY, 15.0, CellWidthModeName::Compact, 0.0, 0.0, None),
        };
    measure_cell_metrics(
        window.text_system(),
        window.scale_factor(),
        family,
        px(font_size),
        mode,
        offset_x,
        offset_y,
        line_height_multiplier,
    )
}

/// Startup sizing uses the configured base font, while a persisted terminal
/// zoom only affects the live grid after the window has been created.
pub(super) fn startup_cell_metrics(window: &Window, cx: &App) -> (Pixels, Pixels) {
    measure_startup_cell_metrics(window.text_system(), window.scale_factor(), cx)
}

/// The same font system and rounding rules can size a window before its HWND exists.
pub(super) fn startup_cell_metrics_at_scale(scale: f32, cx: &App) -> (Pixels, Pixels) {
    let text_system = WindowTextSystem::new(cx.text_system().clone());
    measure_startup_cell_metrics(&text_system, scale, cx)
}

fn measure_startup_cell_metrics(
    text_system: &WindowTextSystem,
    scale: f32,
    cx: &App,
) -> (Pixels, Pixels) {
    let (family, font_size, mode, offset_x, offset_y, line_height_multiplier) =
        match cx.try_global::<Settings>() {
            Some(settings) => (
                settings.font_family.as_str(),
                settings.base_font_size_px,
                settings.cell_width_mode,
                settings.font_offset_x,
                settings.font_offset_y,
                settings.theme_line_height,
            ),
            None => (REQUIRED_FONT_FAMILY, 15.0, CellWidthModeName::Compact, 0.0, 0.0, None),
        };
    measure_cell_metrics(
        text_system,
        scale,
        family,
        px(font_size),
        mode,
        offset_x,
        offset_y,
        line_height_multiplier,
    )
}
