use super::*;

#[gpui::test]
fn cursor_blink_language_changes_keep_the_selected_style(cx: &mut TestAppContext) {
    use crate::gpui_shell::config::Settings;
    use crate::i18n::{Message, UiLanguage};
    let _lock = lock_theme_studio();
    let _bytes = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, cx| {
        pane.active_section = 1;
        cx.notify();
    });
    window.simulate_resize(size(px(1200.0), px(2400.0)));
    draw(&mut window);
    click("settings-select-cursor_blink_style", &mut window);
    press("down", &mut window);
    press("enter", &mut window);
    for language in UiLanguage::ALL {
        pane.update(&mut window, |pane, cx| {
            pane.persist(&[("language", language.code().to_owned())], cx);
        });
        pane.update_in(&mut window, |pane, window, cx| {
            pane.refresh_localized_controls(window, cx);
        });
        draw(&mut window);
        click("settings-select-cursor_blink_style", &mut window);
        press("enter", &mut window);
        assert_eq!(
            RuntimeSettings::load().cursor_blink_style,
            nebula_settings::CursorBlinkStyle::Smooth
        );
        pane.read_with(&window, |pane, cx| {
            assert_eq!(cx.global::<Settings>().ui_language, *language);
            let select = pane.select_of("cursor_blink_style").unwrap().read(cx);
            assert_eq!(select.selected_index(cx).unwrap().row, 1);
            assert_eq!(
                select.selected_value().unwrap().as_ref(),
                language.text(Message::SettingsCursorBlinkSmooth)
            );
        });
    }
}

#[gpui::test]
fn cursor_blink_style_persists_keyboard_choices_reopens_and_cancels(cx: &mut TestAppContext) {
    use crate::gpui_shell::config::Settings;
    let _lock = lock_theme_studio();
    let _bytes = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), TEST_SETTINGS).unwrap();
    let (pane, mut window) = open_settings(cx);
    pane.update(&mut window, |pane, cx| {
        pane.active_section = 1;
        cx.notify();
    });
    window.simulate_resize(size(px(1200.0), px(2400.0)));
    let values = nebula_settings::CursorBlinkStyle::VALUES;
    for (row, &value) in values.iter().enumerate().skip(1) {
        draw(&mut window);
        click("settings-select-cursor_blink_style", &mut window);
        press("down", &mut window);
        press("enter", &mut window);
        assert_eq!(RuntimeSettings::load().cursor_blink_style.settings_value(), value);
        window.read(|cx| {
            let settings = cx.global::<Settings>();
            assert_eq!(settings.cursor_blink_style.settings_value(), value);
            if value == "solid" {
                assert!(!settings.term_config().default_cursor_style.blinking);
            }
        });
        let reopened = window.update(|window, cx| cx.new(|cx| SettingsPane::new(window, cx)));
        reopened.read_with(&mut window, |pane, cx| {
            assert_eq!(
                pane.select_of("cursor_blink_style")
                    .unwrap()
                    .read(cx)
                    .selected_index(cx)
                    .unwrap()
                    .row,
                row
            );
        });
    }
    click("settings-select-cursor_blink_style", &mut window);
    press("up", &mut window);
    press("escape", &mut window);
    pane.read_with(&mut window, |pane, _| {
        assert_eq!(
            pane.runtime.cursor_blink_style.settings_value(),
            *values.last().unwrap(),
            "Escape cancels"
        );
    });
}
