use super::*;
use nebula_settings::SplitShellSource;

#[gpui::test]
fn split_shell_source_is_searchable_keyboard_accessible_live_and_resettable(
    cx: &mut gpui::TestAppContext,
) {
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[("split_shell_source", "default".into())]).unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
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
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(pane.read_with(window, |pane, _| pane.active_section), 2);
    assert_eq!(
        pane.read_with(window, |pane, _| pane.runtime.split_shell_source),
        SplitShellSource::Default
    );

    for (source, keys) in [
        (SplitShellSource::Focused, vec!["up", "enter"]),
        (SplitShellSource::Ask, vec!["down", "down", "enter"]),
        (SplitShellSource::Default, vec!["up", "enter"]),
    ] {
        let bounds = window
            .debug_bounds("settings-select-split_shell_source")
            .expect("real split source dropdown");
        assert_eq!(bounds.size.width, px(SETTINGS_SELECT_WIDTH));
        assert!(bounds.size.height > px(0.0));
        window.simulate_click(bounds.center(), gpui::Modifiers::default());
        window.run_until_parked();
        for key in keys {
            window.simulate_keystrokes(key);
            window.run_until_parked();
            window.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }
        assert_eq!(pane.read_with(window, |pane, _| pane.runtime.split_shell_source), source);
        assert_eq!(RuntimeSettings::load().split_shell_source, source);
        window.update(|_, cx| {
            assert_eq!(
                cx.global::<crate::gpui_shell::config::Settings>().split_shell_source,
                source
            );
        });
        assert_eq!(
            pane.read_with(window, |pane, _| pane.setting_override("split_shell_source")),
            Some((source != SplitShellSource::Default, "default".into()))
        );
    }
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.set_split_shell_source("ask", window, cx);
            pane.sync_select("split_shell_source", "ask", window, cx);
            let (_, factory) = pane.setting_override("split_shell_source").unwrap();
            pane.set_split_shell_source(&factory, window, cx);
            pane.sync_select("split_shell_source", &factory, window, cx);
        })
    });
    assert_eq!(RuntimeSettings::load().split_shell_source, SplitShellSource::Default);
    pane.read_with(window, |pane, cx| {
        assert_eq!(pane.runtime.split_shell_source, SplitShellSource::Default);
        assert_eq!(
            pane.select_of("split_shell_source").unwrap().read(cx).selected_index(cx).unwrap().row,
            1
        );
        assert_eq!(pane.setting_override("split_shell_source"), Some((false, "default".into())));
    });
}

#[test]
fn split_source_choices_use_localized_labels_and_canonical_values() {
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
