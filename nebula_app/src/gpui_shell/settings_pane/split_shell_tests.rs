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
    let saved_settings = std::fs::read(nebula_settings::settings_path()).unwrap();
    let details_selector = "settings-details-Split shell source";
    assert!(window.debug_bounds(details_selector).is_none(), "details start collapsed");
    for expanded in [true, false] {
        let button = window.debug_bounds("settings-help-Split shell source").unwrap();
        assert_eq!(button.size, gpui::size(px(32.0), px(32.0)));
        window.simulate_click(
            gpui::point(button.right() - px(5.0), button.center().y),
            gpui::Modifiers::default(),
        );
        draw(window);
        assert_eq!(window.debug_bounds(details_selector).is_some(), expanded);
        let mut previous_bottom = None;
        for selector in [
            "settings-detail-Split shell source-0",
            "settings-detail-Split shell source-1",
            "settings-detail-Split shell source-2",
            "settings-detail-Split shell source-3",
        ] {
            let line = window.debug_bounds(selector);
            assert_eq!(line.is_some(), expanded);
            if let Some(line) = line {
                assert!(line.size.height > px(0.0));
                if let Some(bottom) = previous_bottom {
                    assert!(line.top() >= bottom, "detail lines must not overlap");
                }
                previous_bottom = Some(line.bottom());
            }
        }
        assert_eq!(
            pane.read_with(window, |pane, _| {
                pane.expanded_setting_help.contains("Split shell source")
            }),
            expanded
        );
        assert_source(&pane, window, SplitShellSource::Default);
        assert_eq!(std::fs::read(nebula_settings::settings_path()).unwrap(), saved_settings);
    }
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

struct SplitSourceHost {
    pane: Entity<SettingsPane>,
    show_help: bool,
}

impl Render for SplitSourceHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.pane.read(cx).focus_handle.clone();
        let language = crate::gpui_shell::config::ui_language(cx);
        let label = language.text(crate::i18n::Message::SettingsSplitShellSource);
        let description = if self.show_help {
            help("split_shell_source", language)
        } else {
            SettingHelp::from("")
        };
        div().size_full().track_focus(&focus).child(self.pane.update(cx, |pane, cx| {
            pane.select_row("split_shell_source", label, description, cx).into_any_element()
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
    let mut host_entity = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        entity = Some(pane.clone());
        let host = cx.new(|cx| {
            cx.observe(&pane, |_, _, cx| cx.notify()).detach();
            SplitSourceHost { pane, show_help: false }
        });
        host_entity = Some(host.clone());
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
    let mut previous_right = track.left();
    for source in [SplitShellSource::Default, SplitShellSource::Focused, SplitShellSource::Ask] {
        let slot = window.debug_bounds(source_selector(source)).unwrap();
        assert!(slot.left() >= previous_right && slot.right() <= track.right());
        assert!(slot.size.height >= px(28.0));
        previous_right = slot.right();
    }
    // Use the existing buttons' Tab/Enter handling, including key-up activation.
    for (source, tabs) in [
        (SplitShellSource::Focused, "tab tab"),
        (SplitShellSource::Ask, "tab tab tab"),
        (SplitShellSource::Default, "tab"),
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
    // Add the real help control and reach it with Tab, without inspecting widget internals.
    // The isolated host has no settings-page scroll container; leave room for expanded prose.
    window.simulate_resize(gpui::size(px(500.0), px(1000.0)));
    window.update(|window, cx| {
        host_entity.unwrap().update(cx, |host, cx| {
            host.show_help = true;
            cx.notify();
        });
        let focus = pane.read(cx).focus_handle.clone();
        focus.focus(window, cx);
    });
    draw(window);
    let details_selector = "settings-details-分屏 Shell 来源";
    let saved_settings = std::fs::read(nebula_settings::settings_path()).unwrap();
    assert!(window.debug_bounds("settings-help-分屏 Shell 来源").is_some());
    assert!(window.debug_bounds(details_selector).is_none());
    window.simulate_keystrokes("tab");
    for expanded in [true, false] {
        window.simulate_keystrokes("enter");
        window.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
        });
        draw(window);
        assert_eq!(window.debug_bounds(details_selector).is_some(), expanded);
        assert_source(&pane, window, SplitShellSource::Default);
        assert_eq!(std::fs::read(nebula_settings::settings_path()).unwrap(), saved_settings);
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
        let description = help("split_shell_source", language);
        assert_eq!(
            description.summary,
            language.text(crate::i18n::Message::SettingsSplitShellSourceDescription)
        );
        assert_eq!(
            description.details,
            Some(language.text(crate::i18n::Message::SettingsSplitShellSourceDetails))
        );
        assert_ne!(Some(description.summary), description.details);
        let lines: Vec<_> = description.details.unwrap().lines().collect();
        assert_eq!(lines.len(), 4, "three source rows and one shared immediate-apply note");
        let prefixes = if language == crate::display::UiLanguage::ZhCn {
            assert_eq!(description.summary, "决定分屏使用的 Shell，默认用设置中的“默认 Shell”。");
            ["使用默认：", "跟随焦点：", "每次选择："]
        } else {
            ["Default:", "Focused:", "Ask:"]
        };
        for (line, prefix) in lines.iter().zip(prefixes) {
            assert!(line.starts_with(prefix), "source explanations must follow capsule order");
        }
        let labels =
            localized_select_labels("split_shell_source", SplitShellSource::VALUES, language);
        assert_eq!(labels.len(), 3);
        assert_eq!(
            labels[0].as_ref(),
            language.text(crate::i18n::Message::SettingsSplitShellSourceDefault)
        );
        assert_eq!(
            labels[1].as_ref(),
            language.text(crate::i18n::Message::SettingsSplitShellSourceFocused)
        );
        assert_eq!(
            labels[2].as_ref(),
            language.text(crate::i18n::Message::SettingsSplitShellSourceAsk)
        );
    }
}
