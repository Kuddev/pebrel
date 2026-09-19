//! Terminal font construction and cell metrics.
//!
//! The terminal view and its element must use one device-pixel rounding rule.
//! Keeping shaping, width, height, and theme line-height policy here prevents
//! the startup grid from drifting away from the first prepainted frame.

use gpui::{
    App, Font, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, SharedString, TextRun, Window, px,
};
use nebula_settings::CellWidthModeName;

use super::Settings;
use crate::font_install::{REQUIRED_FONT_FAMILY, gpui_font_with_fallbacks};

/// GPUI's Windows backend skips font feature setup for an empty feature list.
/// Explicitly enabling `calt` keeps Maple's contextual ligatures consistent
/// across the platforms where the bundled face is used.
pub(super) fn mono_font(family: &str, weight: FontWeight, style: FontStyle) -> Font {
    Font {
        weight,
        style,
        features: FontFeatures(std::sync::Arc::new(vec![("calt".to_owned(), 1)])),
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

/// Keep the shaped font metrics unless a theme explicitly supplies a line
/// height. The offset is applied and floored in device pixels in both cases.
pub(super) fn effective_line_height(natural_height: f32, offset_y: f32, scale: f32) -> Pixels {
    let scale = scale.max(0.5);
    px(((natural_height * scale + offset_y).floor().max(1.0)) / scale)
}

pub(super) fn effective_line_height_with_theme(
    natural_height: f32,
    font_size: f32,
    line_height_multiplier: Option<f32>,
    offset_y: f32,
    scale: f32,
) -> Pixels {
    let target = line_height_multiplier
        .map(|multiplier| (font_size * multiplier).max(1.0))
        .unwrap_or(natural_height);
    effective_line_height(target, offset_y, scale)
}

pub(super) fn line_height_for_view(
    font_size: Pixels,
    line_height_multiplier: Option<f32>,
    natural_height: f32,
    offset_y: f32,
    scale: f32,
) -> Pixels {
    effective_line_height_with_theme(
        natural_height,
        font_size.as_f32(),
        line_height_multiplier,
        offset_y,
        scale,
    )
}

fn measure_cell_metrics(
    window: &Window,
    family: &str,
    font_size: Pixels,
    mode: CellWidthModeName,
    offset_x: f32,
    offset_y: f32,
    line_height_multiplier: Option<f32>,
) -> (Pixels, Pixels) {
    let font = mono_font(family, FontWeight::NORMAL, FontStyle::Normal);
    let sample = window.text_system().shape_line(
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
        effective_cell_width(sample.width.as_f32(), mode, window.scale_factor(), offset_x),
        effective_line_height_with_theme(
            sample.ascent.as_f32() + sample.descent.as_f32(),
            font_size.as_f32(),
            line_height_multiplier,
            offset_y,
            window.scale_factor(),
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
        window,
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
        window,
        family,
        px(font_size),
        mode,
        offset_x,
        offset_y,
        line_height_multiplier,
    )
}
