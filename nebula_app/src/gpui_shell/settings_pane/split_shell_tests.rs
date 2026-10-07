use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
use gpui::VisualTestContext;
use nebula_settings::SplitShellSource;

fn draw(window: &mut VisualTestContext) {
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn source_selector(source: SplitShellSource) -> &'static str {
    match source {
        SplitShellSource::Focused => "settings-choice-split_shell_source-focused",
        SplitShellSource::Default => "settings-choice-split_shell_source-default",
        SplitShellSource::Ask => "settings-choice-split_shell_source-ask",
    }
}

fn assert_source(
    pane: &Entity<SettingsPane>,
    window: &mut VisualTestContext,
    source: SplitShellSource,
) {
    draw(window);
    pane.read_with(window, |pane, cx| {
        assert_eq!(pane.runtime.split_shell_source, source);
        let row =
            pane.select_of("split_shell_source").unwrap().read(cx).selected_index(cx).unwrap().row;
        assert_eq!(SplitShellSource::VALUES[row], source.settings_value());
        assert_eq!(
            pane.setting_override("split_shell_source"),
            Some((source != SplitShellSource::Default, "default".into()))
        );
        assert_eq!(cx.global::<crate::gpui_shell::config::Settings>().split_shell_source, source);
    });
    assert!(window.debug_bounds("settings-select-split_shell_source").is_none());
    let track = window.debug_bounds("settings-choices-split_shell_source").unwrap();
    let thumb = window.debug_bounds("settings-indicator-split_shell_source").unwrap();
    let slot = window.debug_bounds(source_selector(source)).unwrap();
    assert!((f32::from(thumb.left() - slot.left())).abs() < 0.1);
    assert!((f32::from(thumb.size.width - slot.size.width)).abs() < 0.1);
    assert_eq!(thumb.top(), slot.top());
    assert_eq!(thumb.size.height, slot.size.height);
    assert_eq!(thumb.top() - track.top(), px(2.0));
}

fn click_source(window: &mut VisualTestContext, source: SplitShellSource) {
    draw(window);
    let slot =
        window.debug_bounds(source_selector(source)).expect("real split source capsule button");
    // The whole button, including blank padding, must be clickable.
    window.simulate_click(
        gpui::point(slot.right() - px(5.0), slot.center().y),
        gpui::Modifiers::default(),
    );
    draw(window);
}

fn click_reset(window: &mut VisualTestContext, selector: &'static str) {
    draw(window);
    let reset = window.debug_bounds(selector).expect("real restore-default action");
    window.simulate_mouse_move(reset.center(), None, gpui::Modifiers::default());
    draw(window);
    let reset = window.debug_bounds(selector).unwrap();
    window.simulate_click(reset.center(), gpui::Modifiers::default());
    draw(window);
}

#[gpui::test]
fn split_shell_source_is_searchable_keyboard_accessible_live_and_resettable(
    cx: &mut gpui::TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[
        ("split_shell_source", "default".into()),
        ("language", "en-US".into()),
    ])
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut entity = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        entity = Some(pane.clone());
        gpui_component::Root::new(pane, window, cx)
    });
    let pane = entity.unwrap();
    window.simulate_resize(gpui::size(px(1280.0), px(1400.0)));
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.settings_search_input
                .update(cx, |input, cx| input.replace_all("split shell", window, cx));
        })
    });
    draw(window);
    assert_eq!(pane.read_with(window, |pane, _| pane.active_section), 2);
    assert_source(&pane, window, SplitShellSource::Default);
    for source in [SplitShellSource::Focused, SplitShellSource::Ask, SplitShellSource::Default] {
        click_source(window, source);
        assert_source(&pane, window, source);
        assert_eq!(RuntimeSettings::load().split_shell_source, source);
    }
    click_source(window, SplitShellSource::Ask);
    click_reset(window, "setting-reset-Split shell source");
    assert_source(&pane, window, SplitShellSource::Default);
    assert_eq!(RuntimeSettings::load().split_shell_source, SplitShellSource::Default);
}

struct SplitSourceHost(Entity<SettingsPane>);

impl Render for SplitSourceHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.0.read(cx).focus_handle.clone();
        let label = crate::gpui_shell::config::ui_language(cx)
            .text(crate::i18n::Message::SettingsSplitShellSource);
        div().size_full().track_focus(&focus).child(self.0.update(cx, |pane, cx| {
            pane.select_row("split_shell_source", label, "", cx).into_any_element()
        }))
    }
}

/// A directory at the settings-file path forces real persistence to fail on all hosts.
/// Restore bytes even when a UI assertion panics; the outer fixture restores user settings.
struct BlockedSettingsFile {
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}

impl BlockedSettingsFile {
    fn new() -> Self {
        let path = nebula_settings::settings_path();
        let guard = Self { bytes: std::fs::read(&path).unwrap(), path };
        std::fs::remove_file(&guard.path).unwrap();
        std::fs::create_dir(&guard.path).unwrap();
        guard
    }
}

impl Drop for BlockedSettingsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
        let _ = std::fs::write(&self.path, &self.bytes);
    }
}

#[gpui::test]
fn split_source_capsule_keeps_keyboard_rollback_reset_and_text_sized_layout(
    cx: &mut gpui::TestAppContext,
) {
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[
        ("split_shell_source", "default".into()),
        ("language", "zh-CN".into()),
    ])
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        gpui_component::Theme::global_mut(cx).font_size = px(16.0);
    });
    let mut entity = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        entity = Some(pane.clone());
        let host = cx.new(|cx| {
            cx.observe(&pane, |_, _, cx| cx.notify()).detach();
            SplitSourceHost(pane)
        });
        gpui_component::Root::new(host, window, cx)
    });
    let pane = entity.unwrap();
    window.simulate_resize(gpui::size(px(500.0), px(400.0)));
    draw(window);
    let small_width =
        window.debug_bounds("settings-choices-split_shell_source").unwrap().size.width;
    window.update(|_, cx| {
        gpui_component::Theme::global_mut(cx).font_size = px(28.0);
        cx.refresh_windows();
    });
    draw(window);
    let track = window.debug_bounds("settings-choices-split_shell_source").unwrap();
    assert!(track.size.width > small_width);
    assert!(track.size.width > px(SETTINGS_SELECT_WIDTH));
    assert!(track.left() >= px(0.0) && track.right() <= px(500.0));
    for source in [SplitShellSource::Focused, SplitShellSource::Default, SplitShellSource::Ask] {
        let slot = window.debug_bounds(source_selector(source)).unwrap();
        assert!(slot.left() >= track.left() && slot.right() <= track.right());
        assert!(slot.size.height >= px(28.0));
    }
    // Use the existing buttons' Tab/Enter handling, including key-up activation.
    for (source, tabs) in [
        (SplitShellSource::Focused, "tab"),
        (SplitShellSource::Ask, "tab tab tab"),
        (SplitShellSource::Default, "tab tab"),
    ] {
        window.update(|window, cx| {
            let focus = pane.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        });
        window.simulate_keystrokes(tabs);
        window.simulate_keystrokes("enter");
        window.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
        });
        assert_source(&pane, window, source);
        assert_eq!(RuntimeSettings::load().split_shell_source, source);
    }
    // A failed choice must not change the runtime, global preference, index or thumb.
    let blocked = BlockedSettingsFile::new();
    click_source(window, SplitShellSource::Ask);
    assert_source(&pane, window, SplitShellSource::Default);
    drop(blocked);
    assert_eq!(RuntimeSettings::load().split_shell_source, SplitShellSource::Default);

    click_source(window, SplitShellSource::Ask);
    let blocked = BlockedSettingsFile::new();
    click_reset(window, "setting-reset-分屏 Shell 来源");
    assert_source(&pane, window, SplitShellSource::Ask);
    drop(blocked);
    assert_eq!(RuntimeSettings::load().split_shell_source, SplitShellSource::Ask);
    click_reset(window, "setting-reset-分屏 Shell 来源");
    assert_source(&pane, window, SplitShellSource::Default);
    assert_eq!(RuntimeSettings::load().split_shell_source, SplitShellSource::Default);
}

#[test]
fn split_source_choices_use_localized_labels_and_canonical_values() {
    assert!(segmented::supports("split_shell_source"));
    for language in [crate::display::UiLanguage::EnUs, crate::display::UiLanguage::ZhCn] {
        let labels =
            localized_select_labels("split_shell_source", SplitShellSource::VALUES, language);
        assert_eq!(labels.len(), 3);
        assert_eq!(
            labels[0].as_ref(),
            language.text(crate::i18n::Message::SettingsSplitShellSourceFocused)
        );
        assert_eq!(
            labels[1].as_ref(),
            language.text(crate::i18n::Message::SettingsSplitShellSourceDefault)
        );
        assert_eq!(
            labels[2].as_ref(),
            language.text(crate::i18n::Message::SettingsSplitShellSourceAsk)
        );
    }
}
