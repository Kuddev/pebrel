//! Application-parsed defaults. This crate never reads or executes a script.

use std::sync::OnceLock;

use crate::RawSettings;

static DEFAULTS: OnceLock<RawSettings> = OnceLock::new();

/// Install once, before starting UI/background consumers. Persisted preferences
/// remain authoritative; clearing a saved key reveals its configuration default.
pub fn install_config_defaults(defaults: RawSettings) -> Result<(), RawSettings> {
    DEFAULTS.set(defaults)
}

pub(crate) fn apply(saved: RawSettings) -> RawSettings {
    let Some(defaults) = DEFAULTS.get() else { return saved };
    let mut values = defaults.values.clone();
    // Keep explicitly empty saved values: they clear optional defaults too.
    values.extend(saved.values);
    RawSettings { values }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiValueKind {
    Boolean,
    Number,
    String,
}

/// Script field shapes only. RuntimeSettings::from_raw remains the authority for
/// ranges, enum aliases, defaults and theme inheritance.
pub fn ui_value_kind(key: &str) -> Option<UiValueKind> {
    use UiValueKind::*;
    Some(match key {
        "follow_system_theme"
        | "ctrl_wheel_font_zoom"
        | "cursor_blink"
        | "copy_on_select"
        | "focus_follows_mouse"
        | "dim_inactive_panes"
        | "multiline_paste_confirm"
        | "tab_close_visible"
        | "powerline"
        | "ghost"
        | "prompt_suggestions_enabled"
        | "cjk_bold_regular"
        | "ai_toasts"
        | "fetch"
        | "tray"
        | "silent_start"
        | "background_image_cover_chrome"
        | "panel_resize"
        | "pane_card_shadow" => Boolean,
        "font_size"
        | "ui_font_size"
        | "scrollback_lines"
        | "scroll_speed"
        | "opacity"
        | "tab_bar_opacity"
        | "background_image_opacity"
        | "sidebar_width"
        | "sidebar_w"
        | "quick_terminal_width"
        | "quick_terminal_height"
        | "pane_card_radius"
        | "pane_card_gutter"
        | "pane_card_divider" => Number,
        "language"
        | "theme"
        | "app_icon"
        | "font_family"
        | "font_family_cjk"
        | "ui_font_family"
        | "scrollbar_visibility"
        | "ligatures"
        | "cursor_shape"
        | "cursor_motion"
        | "accept"
        | "completion_style"
        | "tabs_position"
        | "tab_reveal"
        | "density"
        | "new_tab_position"
        | "windowing_behavior"
        | "cell_width_mode"
        | "vcs_display"
        | "bell"
        | "notification_duration"
        | "blur"
        | "tab_bar_blur"
        | "background"
        | "theme_foreground"
        | "custom_theme"
        | "background_image"
        | "background_media_kind"
        | "background_image_fit"
        | "background_image_alignment"
        | "quick_terminal_mode"
        | "quick_terminal_hotkey" => String,
        _ => return None,
    })
}
