use super::*;

#[gpui::test]
fn tab_style_persists_keyboard_choices_reopens_and_cancels(cx: &mut TestAppContext) {
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
    let values = nebula_settings::TabStyle::VALUES;
    for (row, &value) in values.iter().enumerate().skip(1) {
        draw(&mut window);
        click("settings-select-tab_style", &mut window);
        press("down", &mut window);
        press("enter", &mut window);
        assert_eq!(RuntimeSettings::load().tab_style.settings_value(), value);
        window.read(|cx| {
            let settings = cx.global::<Settings>();
            assert_eq!(settings.tab_style.settings_value(), value);
        });
        let reopened = window.update(|window, cx| cx.new(|cx| SettingsPane::new(window, cx)));
        reopened.read_with(&mut window, |pane, cx| {
            assert_eq!(
                pane.select_of("tab_style").unwrap().read(cx).selected_index(cx).unwrap().row,
                row
            );
        });
    }
    click("settings-select-tab_style", &mut window);
    press("up", &mut window);
    press("escape", &mut window);
    pane.read_with(&mut window, |pane, _| {
        assert_eq!(
            pane.runtime.tab_style.settings_value(),
            *values.last().unwrap(),
            "Escape cancels"
        );
    });
}
