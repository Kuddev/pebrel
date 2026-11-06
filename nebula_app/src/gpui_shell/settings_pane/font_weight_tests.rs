//! Real settings controls share the existing serialized settings fixture group.

use super::*;
use crate::gpui_shell::config::Settings;
use nebula_settings::TerminalFontWeight;

const SELECTOR: &str = "settings-select-font_weight";

fn open_weight_settings(cx: &mut TestAppContext) -> (Entity<SettingsPane>, VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, mut window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| pane.active_section = 1);
        pane = Some(view.clone());
        Root::new(view, window, cx)
    });
    window.simulate_resize(size(px(1100.0), px(1000.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    (pane.unwrap(), (*window).clone())
}

fn write_fixture(weight: &str) {
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(
        nebula_settings::settings_path(),
        format!("{TEST_SETTINGS}language=en-US\nfont_weight={weight}\ncustom_data=keep\n"),
    )
    .unwrap();
}

fn assert_weight(pane: &Entity<SettingsPane>, window: &mut VisualTestContext, value: &str) {
    let expected = TerminalFontWeight::from_settings(value).unwrap();
    pane.read_with(window, |pane, cx| {
        assert_eq!(pane.runtime.font_weight, expected);
        assert_eq!(cx.global::<Settings>().font_weight, expected);
        let row = TerminalFontWeight::VALUES.iter().position(|item| *item == value).unwrap();
        let select = pane.select_of("font_weight").unwrap().read(cx);
        assert_eq!(select.selected_index(cx).unwrap().row, row);
        let labels = localized_select_labels(
            "font_weight",
            TerminalFontWeight::VALUES,
            crate::gpui_shell::config::ui_language(cx),
        );
        assert_eq!(select.selected_value(), Some(&labels[row]));
    });
}

fn click_reset(window: &mut VisualTestContext, selector: &'static str) {
    let bounds = window.debug_bounds(selector).expect("changed weight has a reset control");
    window.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    draw(window);
    click(selector, window);
}

#[gpui::test]
fn font_weight_menu_persists_cancels_reloads_localizes_and_resets(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    write_fixture("400");
    let (pane, mut window) = open_weight_settings(cx);
    assert_weight(&pane, &mut window, "400");
    pane.update(&mut window, |pane, cx| {
        pane.active_section = 0;
        cx.notify();
    });
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.settings_search_input
                .update(cx, |input, cx| input.replace_all("粗细", window, cx));
        });
    });
    draw(&mut window);
    assert_eq!(pane.read_with(&mut window, |pane, _| pane.active_section), 1);
    let bounds = window.debug_bounds(SELECTOR).expect("font weight is searchable");
    assert!(bounds.top() >= px(0.0) && bounds.bottom() <= px(1000.0));

    for value in ["500", "600"] {
        click(SELECTOR, &mut window);
        press("down", &mut window);
        press("enter", &mut window);
        assert_eq!(RuntimeSettings::load().font_weight.settings_value(), value);
        assert_weight(&pane, &mut window, value);
    }
    assert!(
        std::fs::read_to_string(nebula_settings::settings_path())
            .unwrap()
            .contains("custom_data=keep")
    );
    click(SELECTOR, &mut window);
    press("down", &mut window);
    press("escape", &mut window);
    assert_eq!(RuntimeSettings::load().font_weight.settings_value(), "600");
    assert_weight(&pane, &mut window, "600");
    let reopened = window.update(|window, cx| cx.new(|cx| SettingsPane::new(window, cx)));
    assert_weight(&reopened, &mut window, "600");

    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.persist(&[("language", "zh-CN".into())], cx);
            pane.refresh_localized_controls(window, cx);
        });
    });
    draw(&mut window);
    pane.read_with(&mut window, |pane, cx| {
        assert_eq!(
            pane.select_of("font_weight").unwrap().read(cx).selected_value().unwrap().as_ref(),
            "600 · 半粗"
        );
        assert_eq!(pane.setting_override("font_weight"), Some((true, "400".into())));
    });
    click_reset(&mut window, "setting-reset-终端字体粗细");
    assert_weight(&pane, &mut window, "400");
    assert_eq!(RuntimeSettings::load().font_weight, TerminalFontWeight::default());
    assert_eq!(
        pane.read_with(&mut window, |pane, _| pane.setting_override("font_weight")),
        Some((false, "400".into()))
    );
}

/// Reject saves deterministically and restore the settings bytes even if a test panics.
struct BlockedSettings {
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}

impl BlockedSettings {
    fn new() -> Self {
        let path = nebula_settings::settings_path();
        let guard = Self { bytes: std::fs::read(&path).unwrap(), path };
        std::fs::remove_file(&guard.path).unwrap();
        std::fs::create_dir(&guard.path).unwrap();
        guard
    }
}

impl Drop for BlockedSettings {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
        let _ = std::fs::write(&self.path, &self.bytes);
    }
}

#[gpui::test]
fn font_weight_failed_selection_keeps_the_saved_value(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    write_fixture("600");
    let (pane, mut window) = open_weight_settings(cx);
    let blocked = BlockedSettings::new();
    click(SELECTOR, &mut window);
    press("down", &mut window);
    press("enter", &mut window);
    assert_weight(&pane, &mut window, "600");
    assert_save_failed(&mut window);
    drop(blocked);
    assert_eq!(RuntimeSettings::load().font_weight.settings_value(), "600");
}

#[gpui::test]
fn font_weight_failed_reset_keeps_the_saved_value(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    write_fixture("600");
    let (pane, mut window) = open_weight_settings(cx);
    let blocked = BlockedSettings::new();
    click_reset(&mut window, "setting-reset-Terminal font weight");
    assert_weight(&pane, &mut window, "600");
    assert_save_failed(&mut window);
    drop(blocked);
    assert_eq!(RuntimeSettings::load().font_weight.settings_value(), "600");
}

fn assert_save_failed(window: &mut VisualTestContext) {
    window.update(|window, cx| {
        let root = window.root::<Root>().flatten().unwrap();
        assert!(
            !root.read(cx).notification.read(cx).notifications().is_empty(),
            "a rejected save must show feedback"
        );
    });
}

#[test]
fn font_weight_labels_follow_the_shared_order_and_fallback() {
    assert_eq!(
        localized_select_labels(
            "font_weight",
            TerminalFontWeight::VALUES,
            crate::display::UiLanguage::FrFr
        ),
        localized_select_labels(
            "font_weight",
            TerminalFontWeight::VALUES,
            crate::display::UiLanguage::EnUs
        ),
        "untranslated weights fall back to English"
    );
    for language in [crate::display::UiLanguage::EnUs, crate::display::UiLanguage::ZhCn] {
        let labels = localized_select_labels("font_weight", TerminalFontWeight::VALUES, language);
        assert_eq!(labels.len(), TerminalFontWeight::VALUES.len());
        for (label, value) in labels.iter().zip(TerminalFontWeight::VALUES) {
            assert!(label.starts_with(*value));
        }
        assert_eq!(
            labels[3].as_ref(),
            language.text(crate::i18n::Message::SettingsFontWeightNormal)
        );
    }
}
