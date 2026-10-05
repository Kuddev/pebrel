use super::*;

#[gpui::test]
fn split_shell_picker_is_searchable_clickable_live_and_resettable(cx: &mut gpui::TestAppContext) {
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[("split_shell_picker", "0".into())]).unwrap();
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
                .update(cx, |input, cx| input.replace_all("split picker", window, cx));
        })
    });
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(pane.read_with(window, |pane, _| pane.active_section), 2);
    let bounds = window
        .debug_bounds("nebula-switch-split_shell_picker")
        .expect("terminal split picker switch");
    assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));
    window.simulate_click(bounds.center(), gpui::Modifiers::default());
    window.run_until_parked();
    assert!(pane.read_with(window, |pane, _| pane.runtime.split_shell_picker));
    assert!(nebula_settings::RuntimeSettings::load().split_shell_picker);
    window.update(|_, cx| {
        assert!(cx.global::<crate::gpui_shell::config::Settings>().split_shell_picker)
    });
    assert_eq!(
        pane.read_with(window, |pane, _| pane.setting_override("split_shell_picker")),
        Some((true, "0".into()))
    );
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| pane.toggle("split_shell_picker", false, window, cx))
    });
    assert!(!pane.read_with(window, |pane, _| pane.runtime.split_shell_picker));
    assert_eq!(
        pane.read_with(window, |pane, _| pane.setting_override("split_shell_picker")),
        Some((false, "0".into()))
    );
}
