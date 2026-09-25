use super::*;
use gpui::{Modifiers, TestAppContext};

fn reveal_cursor_motion(window: &mut gpui::VisualTestContext) {
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    window.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(px(500.0), px(400.0)),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-500.0))),
        touch_phase: gpui::TouchPhase::Moved,
        modifiers: Default::default(),
    });
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = window.debug_bounds("settings-select-cursor_motion").unwrap();
    assert!(bounds.top() > px(0.0) && bounds.bottom() < px(800.0));
}

#[gpui::test]
fn cursor_motion_control_is_searchable_keyboard_accessible_and_available(cx: &mut TestAppContext) {
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
            pane.sync_select("cursor_motion", "off", window, cx);
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(1000.0), px(800.0)));
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.settings_search_input
                .update(cx, |input, cx| input.replace_all("平滑", window, cx));
        })
    });
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(pane.read_with(window, |pane, _| pane.active_section), 1);
    reveal_cursor_motion(window);
    let bounds =
        window.debug_bounds("settings-select-cursor_motion").expect("cursor motion select");
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
        assert!(!pane.runtime.ai_toasts, "cursor motion must not change notification delivery");
        let select = pane.select_of("cursor_motion").unwrap();
        assert_eq!(select.read(cx).selected_index(cx).unwrap().row, 0);
        assert_eq!(pane.setting_override("cursor_motion"), Some((false, "off".into())));
    });
}

#[test]
fn cursor_motion_options_have_localized_labels_and_stable_persisted_values() {
    for language in [crate::display::UiLanguage::EnUs, crate::display::UiLanguage::ZhCn] {
        let labels = localized_select_labels(
            "cursor_motion",
            nebula_settings::CursorMotion::VALUES,
            language,
        );
        assert_eq!(labels.len(), 2);
        assert_eq!(
            labels.first().unwrap().as_ref(),
            language.text(crate::i18n::Message::SettingsCursorMotionOff)
        );
        assert_eq!(
            labels.last().unwrap().as_ref(),
            language.text(crate::i18n::Message::SettingsCursorMotionSmooth)
        );
    }
}

/// This test writes settings, so it is opt-in and refuses a nonempty fixture.
/// Run it alone with both QA/config variables pointing to the same fresh directory.
#[gpui::test]
#[ignore = "requires a fresh, isolated PEBREL_CURSOR_QA_DIR and matching PEBREL_CONFIG_DIR"]
fn choosing_cursor_motion_persists_it_and_a_failed_save_restores_the_visible_selection(
    cx: &mut TestAppContext,
) {
    let directory = std::path::PathBuf::from(
        std::env::var_os("PEBREL_CURSOR_QA_DIR").expect("isolated QA directory"),
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
        view.update(cx, |pane, _| pane.active_section = 1);
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(1000.0), px(800.0)));
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    reveal_cursor_motion(window);
    let bounds =
        window.debug_bounds("settings-select-cursor_motion").expect("cursor motion select");
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    for key in ["down", "enter"] {
        window.simulate_keystrokes(key);
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
    assert_eq!(RuntimeSettings::load().cursor_motion, nebula_settings::CursorMotion::Smooth);
    assert!(!RuntimeSettings::load().ai_toasts);
    window.update(|_, cx| {
        assert_eq!(
            cx.global::<crate::gpui_shell::config::Settings>().cursor_motion,
            nebula_settings::CursorMotion::Smooth
        );
    });
    assert!(std::fs::read_to_string(&path).unwrap().contains("custom_data=keep"));

    // A directory in place of the settings file deterministically rejects a save,
    // without changing ACLs or touching the user's real configuration.
    let backup = directory.join("settings-before-error.txt");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    let bounds = window.debug_bounds("settings-select-cursor_motion").unwrap();
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
    for key in ["up", "enter"] {
        window.simulate_keystrokes(key);
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }
    pane.read_with(window, |pane, cx| {
        assert_eq!(pane.runtime.cursor_motion, nebula_settings::CursorMotion::Smooth);
        assert_eq!(
            pane.select_of("cursor_motion").unwrap().read(cx).selected_index(cx).unwrap().row,
            1
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
    assert_eq!(RuntimeSettings::load().cursor_motion, nebula_settings::CursorMotion::Smooth);
}
