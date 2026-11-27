use super::*;

#[gpui::test]
fn residency_row_keeps_ordinary_instances_interactive(cx: &mut gpui::TestAppContext) {
    use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};

    struct OrdinaryResidencyRow {
        pane: gpui::Entity<SettingsPane>,
        _subscription: gpui::Subscription,
    }
    impl gpui::Render for OrdinaryResidencyRow {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let focus = self.pane.read(cx).focus_handle.clone();
            div()
                .size_full()
                .track_focus(&focus)
                .child(self.pane.update(cx, |pane, cx| pane.keep_session_row(true, cx)))
        }
    }

    let _lock = lock_theme_studio();
    let _guard = SettingsBytesGuard::capture();
    pebrel_settings::persist_keys(&[("keep_session", "1".into())]).unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane_out = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        pane_out = Some(pane.clone());
        let row = cx.new(|cx| {
            let subscription = cx.observe(&pane, |_, _, cx| cx.notify());
            OrdinaryResidencyRow { pane, _subscription: subscription }
        });
        gpui_component::Root::new(row, window, cx)
    });
    cx.simulate_resize(gpui::size(px(760.0), px(200.0)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx.debug_bounds("nebula-switch-keep_session").unwrap();
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(!RuntimeSettings::load().keep_session);
    // Button deliberately avoids taking keyboard focus on a pointer click.
    cx.update(|window, cx| {
        let focus = pane_out.as_ref().unwrap().read(cx).focus_handle.clone();
        focus.focus(window, cx);
        let _ = window.draw(cx);
    });
    // The help button precedes the switch in the row's tab order.
    cx.simulate_keystrokes("tab tab");
    for (key, expected) in [("space", true), ("enter", false)] {
        cx.simulate_keystrokes(key);
        cx.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse(key).unwrap() });
        cx.run_until_parked();
        assert_eq!(RuntimeSettings::load().keep_session, expected);
    }
}
