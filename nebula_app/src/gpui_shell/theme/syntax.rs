//! Adapt reviewed theme colors to code highlighting once when the theme changes.
use std::sync::Arc;

use gpui_component::{Theme, highlighter::SyntaxColors};

use crate::display::color::Rgb;
use crate::display::terminal_color::ensure_contrast;

pub(super) fn apply(theme: &mut Theme, chrome: &super::ResolvedTheme) {
    let colors = chrome.base_name().reviewed_palette();
    let background = chrome.terminal_background();
    let foreground = chrome.terminal_foreground();
    let mut highlighted = (*theme.highlight_theme).clone();
    let color = |[r, g, b]: [u8; 3]| super::to_hsla(r, g, b);
    highlighted.style.editor_background = Some(color(background));
    highlighted.style.editor_foreground = Some(color(foreground));
    highlighted.style.editor_line_number = Some(color(colors.muted));
    highlighted.style.editor_active_line_number = Some(color(foreground));
    // Keep the caret row subtle; inherited opaque black/white rows can hide
    // the wallpaper and make otherwise invisible search matches look visible.
    highlighted.style.editor_active_line = Some(color(foreground).opacity(0.06));
    highlighted.style.editor_gutter_background = None;
    let mut syntax =
        serde_json::to_value(&highlighted.style.syntax).expect("syntax theme is serializable");
    if let Some(entries) = syntax.as_object_mut() {
        for (name, style) in entries {
            let source = match name.as_str() {
                "comment" | "comment_doc" | "comment.doc" | "hint" => colors.muted,
                "keyword" | "boolean" | "enum" | "preproc" => colors.purple,
                "function" | "constructor" | "link_text" | "link_uri" => colors.blue,
                "number" | "constant" => colors.yellow,
                "type" | "attribute" | "tag" => colors.cyan,
                name if name.starts_with("string") => colors.green,
                _ => foreground,
            };
            let rgb = |[r, g, b]: [u8; 3]| Rgb::new(r, g, b);
            let resolved = ensure_contrast(
                rgb(source),
                rgb(background),
                rgb(foreground),
                rgb(background),
                4.5,
            );
            if style.is_null() {
                *style = serde_json::json!({});
            }
            if let Some(style) = style.as_object_mut() {
                style.insert(
                    "color".to_owned(),
                    serde_json::json!(format!(
                        "#{:02x}{:02x}{:02x}",
                        resolved.r, resolved.g, resolved.b,
                    )),
                );
            }
        }
    }
    highlighted.style.syntax = serde_json::from_value::<SyntaxColors>(syntax)
        .expect("reviewed syntax colors keep the schema");
    highlighted.name = format!("Pebrel {}", theme.is_dark());
    theme.highlight_theme = Arc::new(highlighted);
}
