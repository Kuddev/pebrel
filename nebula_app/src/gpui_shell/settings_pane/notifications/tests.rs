use super::*;
use gpui::{Modifiers, TestAppContext};

#[gpui::test]
fn duration_control_is_searchable_keyboard_accessible_and_keeps_the_delivery_switch_independent(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_component::init(cx);
        let mut settings = crate::gpui_shell::config::Settings::load(ThemeName::Nord);
        settings.ui_language = crate::display::UiLanguage::EnUs;
        settings.ai_toasts = false;
        cx.set_global(settings);
    });
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, cx| {
            pane.runtime =
                RuntimeSettings::from_raw(&nebula_settings::RawSettings::from_text("ai_toasts=0"));
            pane.sync_select("notification_duration", "default", window, cx);
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(1000.0), px(1100.0)));
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.settings_search_input
                .update(cx, |input, cx| input.replace_all("自动关闭", window, cx));
        })
    });
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(pane.read_with(window, |pane, _| pane.active_section), 2);
    let bounds =
        window.debug_bounds("settings-select-notification_duration").expect("duration select");
    assert_eq!(bounds.size.width, px(SETTINGS_SELECT_WIDTH));
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    window.simulate_keystrokes("escape");
    window.run_until_parked();
    for key in ["enter", "escape"] {
        window.simulate_keystrokes(key);
        window.run_until_parked();
    }
    pane.read_with(window, |pane, cx| {
        assert!(!pane.runtime.ai_toasts, "duration must remain available while reminders are off");
        let select = pane.select_of("notification_duration").unwrap();
        assert_eq!(select.read(cx).selected_index(cx).unwrap().row, 0);
        assert_eq!(pane.setting_override("notification_duration"), Some((false, "default".into())));
    });
}

#[test]
fn duration_options_have_localized_labels_and_stable_persisted_values() {
    for language in [crate::display::UiLanguage::EnUs, crate::display::UiLanguage::ZhCn] {
        let labels = localized_select_labels(
            "notification_duration",
            nebula_settings::NotificationDuration::VALUES,
            language,
        );
        assert_eq!(labels.len(), 6);
        assert_eq!(
            labels.first().unwrap().as_ref(),
            language.text(crate::i18n::Message::SettingsNotificationsDurationDefault)
        );
        assert_eq!(
            labels.last().unwrap().as_ref(),
            language.text(crate::i18n::Message::SettingsNotificationsDurationPersistent)
        );
    }
}

/// This test writes settings, so it is opt-in and refuses a nonempty fixture.
/// Run it alone with both QA/config variables pointing to the same fresh directory.
#[gpui::test]
#[ignore = "requires a fresh, isolated PEBREL_NOTIFICATION_QA_DIR and matching PEBREL_CONFIG_DIR"]
fn choosing_a_duration_persists_it_and_a_failed_save_restores_the_visible_selection(
    cx: &mut TestAppContext,
) {
    let directory = std::path::PathBuf::from(
        std::env::var_os("PEBREL_NOTIFICATION_QA_DIR").expect("isolated QA directory"),
    );
    assert!(directory.is_absolute());
    assert_eq!(Some(directory.as_os_str()), std::env::var_os("PEBREL_CONFIG_DIR").as_deref());
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("pebrel_settings.txt");
    assert!(!path.exists(), "never overwrite an existing configuration");
    std::fs::write(&path, "ai_toasts=0\nlanguage=en-US\ncustom_data=keep\n").unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| pane.active_section = 2);
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(1000.0), px(1100.0)));
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds =
        window.debug_bounds("settings-select-notification_duration").expect("duration select");
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    for key in ["down", "down", "down", "down", "down", "enter"] {
        window.simulate_keystrokes(key);
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
    assert_eq!(
        RuntimeSettings::load().notification_duration,
        nebula_settings::NotificationDuration::Persistent
    );
    assert!(!RuntimeSettings::load().ai_toasts);
    window.update(|_, cx| {
        assert_eq!(
            cx.global::<crate::gpui_shell::config::Settings>().notification_duration,
            nebula_settings::NotificationDuration::Persistent
        );
    });
    assert!(std::fs::read_to_string(&path).unwrap().contains("custom_data=keep"));

    // A directory in place of the settings file deterministically rejects a save,
    // without changing ACLs or touching the user's real configuration.
    let backup = directory.join("settings-before-error.txt");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    let bounds = window.debug_bounds("settings-select-notification_duration").unwrap();
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    for key in ["up", "up", "up", "up", "up", "enter"] {
        window.simulate_keystrokes(key);
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
    pane.read_with(window, |pane, cx| {
        assert_eq!(
            pane.runtime.notification_duration,
            nebula_settings::NotificationDuration::Persistent
        );
        assert_eq!(
            pane.select_of("notification_duration")
                .unwrap()
                .read(cx)
                .selected_index(cx)
                .unwrap()
                .row,
            5
        );
    });
    window.update(|window, cx| {
        let root = window.root::<gpui_component::Root>().flatten().unwrap();
        assert!(
            !root.read(cx).notification.read(cx).notifications().is_empty(),
            "save failure needs visible feedback"
        );
    });
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&backup, &path).unwrap();
    assert_eq!(
        RuntimeSettings::load().notification_duration,
        nebula_settings::NotificationDuration::Persistent
    );
}
